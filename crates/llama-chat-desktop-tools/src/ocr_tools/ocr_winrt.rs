//! Windows WinRT OCR backend: `ocr_image_winrt`, `ocr_find_text_winrt`, and STA helpers.

use super::ocr_common::OcrMatch;

/// Internal: run OCR via Windows.Media.Ocr WinRT API. Must be called from STA thread.
#[cfg(windows)]
pub(crate) fn ocr_image_winrt(img: &image::RgbaImage) -> Result<String, String> {
    use windows::Media::Ocr::OcrEngine;
    use windows::Graphics::Imaging::{SoftwareBitmap, BitmapPixelFormat, BitmapAlphaMode};
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::core::HRESULT;

    // Init COM as STA
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        // S_OK (0) or S_FALSE (1, already init) are fine
        if hr.is_err() && hr != HRESULT(1) {
            return Err(format!("CoInitializeEx failed: {hr:?}"));
        }
    }

    let (w, h) = (img.width(), img.height());

    // Convert RGBA → BGRA (Windows expects BGRA8)
    let mut bgra = img.as_raw().clone();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2); // R ↔ B
    }

    // Create SoftwareBitmap from BGRA pixel data (CreateWithAlpha for 4-arg version)
    let bitmap = SoftwareBitmap::CreateWithAlpha(
        BitmapPixelFormat::Bgra8,
        w as i32,
        h as i32,
        BitmapAlphaMode::Premultiplied,
    ).map_err(|e| format!("SoftwareBitmap::CreateWithAlpha: {e}"))?;

    // Write pixel data to IBuffer via DataWriter + synchronous wait
    let stream = InMemoryRandomAccessStream::new()
        .map_err(|e| format!("InMemoryRandomAccessStream: {e}"))?;
    let writer = DataWriter::CreateDataWriter(&stream)
        .map_err(|e| format!("DataWriter: {e}"))?;
    writer.WriteBytes(&bgra)
        .map_err(|e| format!("WriteBytes: {e}"))?;

    // Store + Flush: in-memory stream ops, pump STA messages to let them complete
    let store_op = writer.StoreAsync()
        .map_err(|e| format!("StoreAsync: {e}"))?;
    wait_for_winrt_async(
        "StoreAsync",
        100,
        || store_op.Status().map(|s| s.0).map_err(|e| format!("StoreAsync status: {e}")),
        || store_op.Cancel().map_err(|e| format!("StoreAsync cancel: {e}")),
    )?;
    store_op.GetResults().map_err(|e| format!("StoreAsync result: {e}"))?;

    let flush_op = writer.FlushAsync()
        .map_err(|e| format!("FlushAsync: {e}"))?;
    wait_for_winrt_async(
        "FlushAsync",
        100,
        || flush_op.Status().map(|s| s.0).map_err(|e| format!("FlushAsync status: {e}")),
        || flush_op.Cancel().map_err(|e| format!("FlushAsync cancel: {e}")),
    )?;
    flush_op.GetResults().map_err(|e| format!("FlushAsync result: {e}"))?;

    // Read back as IBuffer
    stream.Seek(0).map_err(|e| format!("Seek: {e}"))?;
    let reader = windows::Storage::Streams::DataReader::CreateDataReader(&stream)
        .map_err(|e| format!("DataReader: {e}"))?;
    let load_op = reader.LoadAsync(bgra.len() as u32)
        .map_err(|e| format!("LoadAsync: {e}"))?;
    wait_for_winrt_async(
        "LoadAsync",
        100,
        || load_op.Status().map(|s| s.0).map_err(|e| format!("LoadAsync status: {e}")),
        || load_op.Cancel().map_err(|e| format!("LoadAsync cancel: {e}")),
    )?;
    load_op.GetResults().map_err(|e| format!("LoadAsync result: {e}"))?;

    let buffer = reader.ReadBuffer(bgra.len() as u32)
        .map_err(|e| format!("ReadBuffer: {e}"))?;

    bitmap.CopyFromBuffer(&buffer)
        .map_err(|e| format!("CopyFromBuffer: {e}"))?;

    // Create OCR engine from user profile languages
    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|e| format!("OcrEngine: {e}"))?;

    // Run recognition — may take 50-500ms depending on image size
    let recognize_op = engine.RecognizeAsync(&bitmap)
        .map_err(|e| format!("RecognizeAsync: {e}"))?;
    wait_for_winrt_async(
        "RecognizeAsync",
        5000,
        || recognize_op.Status().map(|s| s.0).map_err(|e| format!("RecognizeAsync status: {e}")),
        || recognize_op.Cancel().map_err(|e| format!("RecognizeAsync cancel: {e}")),
    )?;
    let ocr_result = recognize_op.GetResults()
        .map_err(|e| format!("RecognizeAsync result: {e}"))?;

    let text = ocr_result.Text()
        .map_err(|e| format!("Text: {e}"))?
        .to_string();

    Ok(text)
}

/// OCR find text returning structured matches. Must be called from STA thread.
#[cfg(windows)]
pub(crate) fn ocr_find_text_winrt(img: &image::RgbaImage, search: &str, offset_x: f64, offset_y: f64) -> Result<Vec<OcrMatch>, String> {
    use windows::Media::Ocr::OcrEngine;
    use windows::Graphics::Imaging::{SoftwareBitmap, BitmapPixelFormat, BitmapAlphaMode};
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::core::HRESULT;

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() && hr != HRESULT(1) {
            return Err(format!("CoInitializeEx failed: {hr:?}"));
        }
    }

    let (w, h) = (img.width(), img.height());
    let mut bgra = img.as_raw().clone();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    let bitmap = SoftwareBitmap::CreateWithAlpha(
        BitmapPixelFormat::Bgra8, w as i32, h as i32, BitmapAlphaMode::Premultiplied,
    ).map_err(|e| format!("SoftwareBitmap: {e}"))?;

    let stream = InMemoryRandomAccessStream::new().map_err(|e| format!("Stream: {e}"))?;
    let writer = DataWriter::CreateDataWriter(&stream).map_err(|e| format!("DataWriter: {e}"))?;
    writer.WriteBytes(&bgra).map_err(|e| format!("WriteBytes: {e}"))?;

    let store_op = writer.StoreAsync().map_err(|e| format!("StoreAsync: {e}"))?;
    wait_for_winrt_async(
        "StoreAsync",
        100,
        || store_op.Status().map(|s| s.0).map_err(|e| format!("StoreAsync status: {e}")),
        || store_op.Cancel().map_err(|e| format!("StoreAsync cancel: {e}")),
    )?;
    store_op.GetResults().map_err(|e| format!("StoreAsync result: {e}"))?;
    let flush_op = writer.FlushAsync().map_err(|e| format!("FlushAsync: {e}"))?;
    wait_for_winrt_async(
        "FlushAsync",
        100,
        || flush_op.Status().map(|s| s.0).map_err(|e| format!("FlushAsync status: {e}")),
        || flush_op.Cancel().map_err(|e| format!("FlushAsync cancel: {e}")),
    )?;
    flush_op.GetResults().map_err(|e| format!("FlushAsync result: {e}"))?;

    stream.Seek(0).map_err(|e| format!("Seek: {e}"))?;
    let reader = windows::Storage::Streams::DataReader::CreateDataReader(&stream)
        .map_err(|e| format!("DataReader: {e}"))?;
    let load_op = reader.LoadAsync(bgra.len() as u32).map_err(|e| format!("LoadAsync: {e}"))?;
    wait_for_winrt_async(
        "LoadAsync",
        100,
        || load_op.Status().map(|s| s.0).map_err(|e| format!("LoadAsync status: {e}")),
        || load_op.Cancel().map_err(|e| format!("LoadAsync cancel: {e}")),
    )?;
    load_op.GetResults().map_err(|e| format!("LoadAsync result: {e}"))?;
    let buffer = reader.ReadBuffer(bgra.len() as u32).map_err(|e| format!("ReadBuffer: {e}"))?;
    bitmap.CopyFromBuffer(&buffer).map_err(|e| format!("CopyFromBuffer: {e}"))?;

    let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|e| format!("OcrEngine: {e}"))?;
    let recognize_op = engine.RecognizeAsync(&bitmap).map_err(|e| format!("RecognizeAsync: {e}"))?;
    wait_for_winrt_async(
        "RecognizeAsync",
        5000,
        || recognize_op.Status().map(|s| s.0).map_err(|e| format!("RecognizeAsync status: {e}")),
        || recognize_op.Cancel().map_err(|e| format!("RecognizeAsync cancel: {e}")),
    )?;
    let ocr_result = recognize_op.GetResults().map_err(|e| format!("RecognizeAsync result: {e}"))?;

    // Search through lines and words for matches
    let lines = ocr_result.Lines().map_err(|e| format!("Lines: {e}"))?;
    let mut matches = Vec::new();

    for i in 0..lines.Size().unwrap_or(0) {
        let line = lines.GetAt(i).map_err(|e| format!("GetAt line: {e}"))?;
        let line_text = line.Text().map(|s| s.to_string()).unwrap_or_default();

        // Check if the full line matches
        if line_text.to_lowercase().contains(search) {
            let words = line.Words().map_err(|e| format!("Words: {e}"))?;
            let mut min_x = f64::MAX;
            let mut min_y = f64::MAX;
            let mut max_x = 0.0f64;
            let mut max_y = 0.0f64;

            for j in 0..words.Size().unwrap_or(0) {
                let word = words.GetAt(j).map_err(|e| format!("GetAt word: {e}"))?;
                let rect = word.BoundingRect().map_err(|e| format!("BoundingRect: {e}"))?;
                min_x = min_x.min(rect.X as f64);
                min_y = min_y.min(rect.Y as f64);
                max_x = max_x.max((rect.X + rect.Width) as f64);
                max_y = max_y.max((rect.Y + rect.Height) as f64);
            }

            let w = max_x - min_x;
            let h = max_y - min_y;
            matches.push(OcrMatch {
                text: line_text.clone(),
                x: min_x + offset_x,
                y: min_y + offset_y,
                width: w,
                height: h,
                center_x: (min_x + max_x) / 2.0 + offset_x,
                center_y: (min_y + max_y) / 2.0 + offset_y,
                confidence: 1.0, // WinRT OCR doesn't expose per-word confidence
            });
        }

        // Also check individual words
        let words = line.Words().map_err(|e| format!("Words: {e}"))?;
        for j in 0..words.Size().unwrap_or(0) {
            let word = words.GetAt(j).map_err(|e| format!("GetAt word: {e}"))?;
            let word_text = word.Text().map(|s| s.to_string()).unwrap_or_default();
            if word_text.to_lowercase().contains(search) && !line_text.to_lowercase().contains(search) {
                let rect = word.BoundingRect().map_err(|e| format!("BoundingRect: {e}"))?;
                matches.push(OcrMatch {
                    text: word_text,
                    x: rect.X as f64 + offset_x,
                    y: rect.Y as f64 + offset_y,
                    width: rect.Width as f64,
                    height: rect.Height as f64,
                    center_x: rect.X as f64 + rect.Width as f64 / 2.0 + offset_x,
                    center_y: rect.Y as f64 + rect.Height as f64 / 2.0 + offset_y,
                    confidence: 1.0, // WinRT OCR doesn't expose per-word confidence
                });
            }
        }
    }

    Ok(matches)
}

/// Pump STA message loop for a duration (ms). Required for WinRT async ops to complete on STA.
#[cfg(windows)]
#[allow(dead_code)]
pub(crate) fn pump_sta_messages(duration_ms: u64) {
    #[repr(C)]
    #[allow(clippy::upper_case_acronyms)]
    struct MSG([u8; 48]); // sizeof(MSG) = 48 on x64

    #[link(name = "user32")]
    unsafe extern "system" {
        fn PeekMessageW(msg: *mut MSG, hwnd: isize, min: u32, max: u32, remove: u32) -> i32;
        fn TranslateMessage(msg: *const MSG) -> i32;
        fn DispatchMessageW(msg: *const MSG) -> isize;
    }

    let start = std::time::Instant::now();
    let mut msg = MSG([0u8; 48]);

    while start.elapsed().as_millis() < duration_ms as u128 {
        if crate::desktop_call_cancelled() {
            break;
        }
        unsafe {
            while PeekMessageW(&mut msg, 0, 0, 0, 1) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if crate::interruptible_sleep(std::time::Duration::from_millis(2)).is_err() {
            break;
        }
    }
}

#[cfg(windows)]
pub(crate) fn wait_for_winrt_async(
    op_name: &str,
    duration_ms: u64,
    mut status: impl FnMut() -> Result<i32, String>,
    mut cancel: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    #[repr(C)]
    #[allow(clippy::upper_case_acronyms)]
    struct MSG([u8; 48]); // sizeof(MSG) = 48 on x64

    #[link(name = "user32")]
    unsafe extern "system" {
        fn PeekMessageW(msg: *mut MSG, hwnd: isize, min: u32, max: u32, remove: u32) -> i32;
        fn TranslateMessage(msg: *const MSG) -> i32;
        fn DispatchMessageW(msg: *const MSG) -> isize;
    }

    let start = std::time::Instant::now();
    let mut msg = MSG([0u8; 48]);

    loop {
        if crate::desktop_call_cancelled() {
            let _ = cancel();
            return Err(crate::desktop_cancel_error());
        }

        unsafe {
            while PeekMessageW(&mut msg, 0, 0, 0, 1) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        match status()? {
            1 => return Ok(()),
            2 => return Err("Operation cancelled".to_string()),
            3 => return Err(format!("{op_name} failed")),
            0 => {}
            _ => {}
        }

        if start.elapsed().as_millis() >= duration_ms as u128 {
            let _ = cancel();
            return Err(format!("{op_name} timed out after {duration_ms}ms"));
        }

        crate::interruptible_sleep(std::time::Duration::from_millis(2))?;
    }
}
