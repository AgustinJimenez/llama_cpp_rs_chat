//! OCR, UI Automation, screenshot, and clipboard image tools.

use serde_json::Value;

use super::NativeToolResult;
use super::{parse_int, tool_click_screen};

#[cfg(windows)]
use super::win32;

// ─── Screenshot tools ─────────────────────────────────────────────────────────

/// Capture a screenshot of a specific screen region.
pub fn tool_screenshot_region(args: &Value) -> NativeToolResult {
    let x = match args.get("x").and_then(parse_int) {
        Some(v) => v as u32,
        None => return NativeToolResult::text_only("Error: 'x' is required".to_string()),
    };
    let y = match args.get("y").and_then(parse_int) {
        Some(v) => v as u32,
        None => return NativeToolResult::text_only("Error: 'y' is required".to_string()),
    };
    let w = match args.get("width").and_then(parse_int) {
        Some(v) => v as u32,
        None => return NativeToolResult::text_only("Error: 'width' is required".to_string()),
    };
    let h = match args.get("height").and_then(parse_int) {
        Some(v) => v as u32,
        None => return NativeToolResult::text_only("Error: 'height' is required".to_string()),
    };
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
    };
    if monitor_idx >= monitors.len() {
        return NativeToolResult::text_only(format!(
            "Error: monitor index {monitor_idx} out of range (0..{})", monitors.len()
        ));
    }

    let img = match monitors[monitor_idx].capture_image() {
        Ok(i) => i,
        Err(e) => return NativeToolResult::text_only(format!("Error capturing screen: {e}")),
    };

    // Crop the image manually using the image crate
    let img_w = img.width();
    let img_h = img.height();
    if x + w > img_w || y + h > img_h {
        return NativeToolResult::text_only(format!(
            "Error: region ({x},{y} {w}x{h}) exceeds screen size ({img_w}x{img_h})"
        ));
    }
    let cropped: image::RgbaImage = image::imageops::crop_imm(&img, x, y, w, h).to_image();

    // Encode to PNG
    let mut png_buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut png_buf);
    if let Err(e) = cropped.write_to(&mut cursor, image::ImageFormat::Png) {
        return NativeToolResult::text_only(format!("Error encoding PNG: {e}"));
    }

    NativeToolResult::with_image(
        format!("Screenshot region: ({x},{y}) {w}x{h} from monitor {monitor_idx}"),
        png_buf,
    )
}

/// Compare current screen to a saved baseline, reporting changed regions.
pub fn tool_screenshot_diff(args: &Value) -> NativeToolResult {
    use std::sync::Mutex;
    lazy_static::lazy_static! {
        static ref BASELINE: Mutex<Option<Vec<u8>>> = Mutex::new(None);
    }

    let save_baseline = args.get("save_baseline").map(|v| super::parse_bool(v, false)).unwrap_or(false);
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
    };
    if monitor_idx >= monitors.len() {
        return NativeToolResult::text_only(format!(
            "Error: monitor index {monitor_idx} out of range (0..{})", monitors.len()
        ));
    }

    let img = match monitors[monitor_idx].capture_image() {
        Ok(i) => i,
        Err(e) => return NativeToolResult::text_only(format!("Error capturing screen: {e}")),
    };
    let current_bytes = img.as_raw().clone();

    let w = img.width();
    let h = img.height();

    if save_baseline {
        let mut lock = BASELINE.lock().unwrap();
        *lock = Some(current_bytes);
        return NativeToolResult::text_only(format!(
            "Baseline saved: {w}x{h} ({} bytes). Call again without save_baseline to compare.",
            lock.as_ref().map(|b| b.len()).unwrap_or(0)
        ));
    }

    // Compare with baseline
    let lock = BASELINE.lock().unwrap();
    let baseline = match lock.as_ref() {
        Some(b) => b,
        None => return NativeToolResult::text_only(
            "Error: no baseline saved. Call with save_baseline=true first.".to_string()
        ),
    };

    if current_bytes.len() != baseline.len() {
        return NativeToolResult::text_only(format!(
            "Error: screen resolution changed since baseline (baseline {} bytes, current {} bytes)",
            baseline.len(), current_bytes.len()
        ));
    }
    let mut changed_pixels = 0u64;
    let total_pixels = (w as u64) * (h as u64);
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let threshold: i16 = 10;

    // RGBA: 4 bytes per pixel
    for py in 0..h {
        for px in 0..w {
            let idx = ((py * w + px) * 4) as usize;
            let dr = (current_bytes[idx] as i16 - baseline[idx] as i16).abs();
            let dg = (current_bytes[idx + 1] as i16 - baseline[idx + 1] as i16).abs();
            let db = (current_bytes[idx + 2] as i16 - baseline[idx + 2] as i16).abs();
            if dr > threshold || dg > threshold || db > threshold {
                changed_pixels += 1;
                min_x = min_x.min(px);
                min_y = min_y.min(py);
                max_x = max_x.max(px);
                max_y = max_y.max(py);
            }
        }
    }

    if changed_pixels == 0 {
        return NativeToolResult::text_only("No changes detected — screen matches baseline.".to_string());
    }

    let pct = (changed_pixels as f64 / total_pixels as f64) * 100.0;
    NativeToolResult::text_only(format!(
        "Screen diff: {:.2}% pixels changed ({changed_pixels}/{total_pixels}). Changed region: ({min_x},{min_y}) to ({max_x},{max_y}) = {}x{}",
        pct, max_x - min_x + 1, max_y - min_y + 1
    ))
}

/// Capture a screenshot of a specific window by title.
pub fn tool_window_screenshot(args: &Value) -> NativeToolResult {
    let title = match args.get("title").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return NativeToolResult::text_only("Error: 'title' argument is required".to_string()),
    };

    let windows = match xcap::Window::all() {
        Ok(w) => w,
        Err(e) => return NativeToolResult::text_only(format!("Error listing windows: {e}")),
    };

    let title_lower = title.to_lowercase();
    let target = windows.into_iter().find(|w| {
        w.title().unwrap_or_default().to_lowercase().contains(&title_lower)
            || w.app_name().unwrap_or_default().to_lowercase().contains(&title_lower)
    });

    let window = match target {
        Some(w) => w,
        None => return NativeToolResult::text_only(format!("No window matches '{title}'")),
    };

    let capture = match window.capture_image() {
        Ok(img) => img,
        Err(e) => return NativeToolResult::text_only(format!("Error capturing window: {e}")),
    };

    // Encode to PNG
    let mut png_data = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_data);
    if let Err(e) = image::ImageEncoder::write_image(
        encoder,
        capture.as_raw(),
        capture.width(),
        capture.height(),
        image::ExtendedColorType::Rgba8,
    ) {
        return NativeToolResult::text_only(format!("Error encoding PNG: {e}"));
    }

    let window_title = window.title().unwrap_or_default();
    NativeToolResult::with_image(
        format!("Window screenshot of '{window_title}' ({}x{})", capture.width(), capture.height()),
        png_data,
    )
}

/// Wait for a screen region to change (pixel comparison polling).
pub fn tool_wait_for_screen_change(args: &Value) -> NativeToolResult {
    let x = args.get("x").and_then(parse_int).unwrap_or(0) as u32;
    let y = args.get("y").and_then(parse_int).unwrap_or(0) as u32;
    let w = args.get("width").and_then(parse_int).unwrap_or(200) as u32;
    let h = args.get("height").and_then(parse_int).unwrap_or(200) as u32;
    let timeout_ms = args.get("timeout_ms").and_then(parse_int).unwrap_or(10000).min(30000) as u64;
    let threshold = args.get("threshold").and_then(parse_int).unwrap_or(5) as f64;
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    // Capture baseline region
    let baseline = match capture_region(monitor_idx, x, y, w, h) {
        Ok(img) => img,
        Err(e) => return NativeToolResult::text_only(format!("Error capturing baseline: {e}")),
    };

    let start = std::time::Instant::now();
    let poll_interval = std::time::Duration::from_millis(200);

    loop {
        if start.elapsed().as_millis() >= timeout_ms as u128 {
            return NativeToolResult::text_only(format!(
                "Timeout: no change detected in region ({x},{y} {w}x{h}) after {timeout_ms}ms"
            ));
        }

        std::thread::sleep(poll_interval);

        let current = match capture_region(monitor_idx, x, y, w, h) {
            Ok(img) => img,
            Err(_) => continue,
        };

        // Compare pixels
        let total = (w * h) as f64;
        let mut changed = 0u64;
        for (bp, cp) in baseline.as_raw().chunks_exact(4).zip(current.as_raw().chunks_exact(4)) {
            let dr = (bp[0] as i32 - cp[0] as i32).unsigned_abs();
            let dg = (bp[1] as i32 - cp[1] as i32).unsigned_abs();
            let db = (bp[2] as i32 - cp[2] as i32).unsigned_abs();
            if dr > 10 || dg > 10 || db > 10 {
                changed += 1;
            }
        }

        let pct = changed as f64 / total * 100.0;
        if pct >= threshold {
            return NativeToolResult::text_only(format!(
                "Screen change detected in region ({x},{y} {w}x{h}): {pct:.1}% changed after {}ms",
                start.elapsed().as_millis()
            ));
        }
    }
}

/// Helper: capture a screen region as RgbaImage
fn capture_region(monitor_idx: usize, x: u32, y: u32, w: u32, h: u32) -> Result<image::RgbaImage, String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("Monitor::all: {e}"))?;
    let monitor = monitors.get(monitor_idx).ok_or("Monitor index out of range")?;
    let full = monitor.capture_image().map_err(|e| format!("capture: {e}"))?;
    // Clamp region to image bounds
    let cx = x.min(full.width().saturating_sub(1));
    let cy = y.min(full.height().saturating_sub(1));
    let cw = w.min(full.width().saturating_sub(cx));
    let ch = h.min(full.height().saturating_sub(cy));
    Ok(image::imageops::crop_imm(&full, cx, cy, cw, ch).to_image())
}

// ─── OCR tools ────────────────────────────────────────────────────────────────

/// OCR: extract text from the screen using Windows.Media.Ocr.
#[cfg(windows)]
pub fn tool_ocr_screen(args: &Value) -> NativeToolResult {
    // Region params (optional — full screen if omitted)
    let region_x = args.get("x").and_then(parse_int).map(|v| v as u32);
    let region_y = args.get("y").and_then(parse_int).map(|v| v as u32);
    let region_w = args.get("width").and_then(parse_int).map(|v| v as u32);
    let region_h = args.get("height").and_then(parse_int).map(|v| v as u32);
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    // Capture screen
    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
    };
    if monitor_idx >= monitors.len() {
        return NativeToolResult::text_only(format!(
            "Error: monitor {monitor_idx} out of range (0..{})", monitors.len()
        ));
    }
    let img = match monitors[monitor_idx].capture_image() {
        Ok(i) => i,
        Err(e) => return NativeToolResult::text_only(format!("Error capturing: {e}")),
    };

    // Crop if region specified
    let full_w = img.width();
    let full_h = img.height();
    let (work_img, crop_x, crop_y) = if let (Some(rx), Some(ry), Some(rw), Some(rh)) = (region_x, region_y, region_w, region_h) {
        if rx + rw > full_w || ry + rh > full_h {
            return NativeToolResult::text_only(format!(
                "Error: region ({rx},{ry} {rw}x{rh}) exceeds screen ({full_w}x{full_h})"
            ));
        }
        let cropped: image::RgbaImage = image::imageops::crop_imm(&img, rx, ry, rw, rh).to_image();
        (cropped, rx, ry)
    } else {
        (img, 0u32, 0u32)
    };

    let work_w = work_img.width();
    let work_h = work_img.height();

    // Run OCR on a temporary STA thread (WinRT requires STA)
    let result = std::thread::spawn(move || {
        ocr_image_winrt(&work_img)
    }).join().unwrap_or_else(|_| Err("OCR thread panicked".to_string()));

    match result {
        Ok(text) => {
            let region_info = if region_x.is_some() {
                format!(" (region {crop_x},{crop_y} {work_w}x{work_h})")
            } else {
                format!(" ({full_w}x{full_h})")
            };
            if text.is_empty() {
                NativeToolResult::text_only(format!("OCR{region_info}: no text detected"))
            } else {
                let line_count = text.lines().count();
                NativeToolResult::text_only(format!(
                    "OCR{region_info}: {line_count} lines\n{text}"
                ))
            }
        }
        Err(e) => NativeToolResult::text_only(format!("OCR error: {e}")),
    }
}

/// Internal: run OCR via Windows.Media.Ocr WinRT API. Must be called from STA thread.
#[cfg(windows)]
fn ocr_image_winrt(img: &image::RgbaImage) -> Result<String, String> {
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
    pump_sta_messages(100); // in-memory, completes in <1ms
    store_op.GetResults().map_err(|e| format!("StoreAsync result: {e}"))?;

    let flush_op = writer.FlushAsync()
        .map_err(|e| format!("FlushAsync: {e}"))?;
    pump_sta_messages(100);
    flush_op.GetResults().map_err(|e| format!("FlushAsync result: {e}"))?;

    // Read back as IBuffer
    stream.Seek(0).map_err(|e| format!("Seek: {e}"))?;
    let reader = windows::Storage::Streams::DataReader::CreateDataReader(&stream)
        .map_err(|e| format!("DataReader: {e}"))?;
    let load_op = reader.LoadAsync(bgra.len() as u32)
        .map_err(|e| format!("LoadAsync: {e}"))?;
    pump_sta_messages(100);
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
    pump_sta_messages(5000); // OCR can take a few seconds for large images
    let ocr_result = recognize_op.GetResults()
        .map_err(|e| format!("RecognizeAsync result: {e}"))?;

    let text = ocr_result.Text()
        .map_err(|e| format!("Text: {e}"))?
        .to_string();

    Ok(text)
}

/// Pump STA message loop for a duration (ms). Required for WinRT async ops to complete on STA.
#[cfg(windows)]
fn pump_sta_messages(duration_ms: u64) {
    #[repr(C)]
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
        unsafe {
            while PeekMessageW(&mut msg, 0, 0, 0, 1) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[cfg(not(windows))]
pub fn tool_ocr_screen(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: ocr_screen is only available on Windows".to_string())
}

/// OCR the screen (or region) and search for specific text, returning its bounding box coordinates.
#[cfg(windows)]
pub fn tool_ocr_find_text(args: &Value) -> NativeToolResult {
    let search_text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return NativeToolResult::text_only("Error: 'text' argument is required".to_string()),
    };
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    // Capture screen
    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return NativeToolResult::text_only(format!("Error listing monitors: {e}")),
    };
    let monitor = match monitors.get(monitor_idx) {
        Some(m) => m,
        None => return NativeToolResult::text_only(format!("Error: monitor index {monitor_idx} out of range (have {})", monitors.len())),
    };
    let capture = match monitor.capture_image() {
        Ok(img) => img,
        Err(e) => return NativeToolResult::text_only(format!("Error capturing screen: {e}")),
    };

    // Optional region crop
    let work_img = if let (Some(rx), Some(ry), Some(rw), Some(rh)) = (
        args.get("x").and_then(parse_int),
        args.get("y").and_then(parse_int),
        args.get("width").and_then(parse_int),
        args.get("height").and_then(parse_int),
    ) {
        image::imageops::crop_imm(&capture, rx as u32, ry as u32, rw as u32, rh as u32).to_image()
    } else {
        capture
    };

    // Region offset for coordinate correction
    let offset_x = args.get("x").and_then(parse_int).unwrap_or(0) as f64;
    let offset_y = args.get("y").and_then(parse_int).unwrap_or(0) as f64;

    // Run OCR with bounding boxes on STA thread
    let search = search_text.to_lowercase();
    let result = std::thread::spawn(move || {
        ocr_find_text_winrt(&work_img, &search, offset_x, offset_y)
    }).join().unwrap_or_else(|_| Err("OCR thread panicked".to_string()));

    match result {
        Ok(text) => NativeToolResult::text_only(text),
        Err(e) => NativeToolResult::text_only(format!("OCR error: {e}")),
    }
}

#[cfg(windows)]
fn ocr_find_text_winrt(img: &image::RgbaImage, search: &str, offset_x: f64, offset_y: f64) -> Result<String, String> {
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
    pump_sta_messages(100);
    store_op.GetResults().map_err(|e| format!("StoreAsync result: {e}"))?;
    let flush_op = writer.FlushAsync().map_err(|e| format!("FlushAsync: {e}"))?;
    pump_sta_messages(100);
    flush_op.GetResults().map_err(|e| format!("FlushAsync result: {e}"))?;

    stream.Seek(0).map_err(|e| format!("Seek: {e}"))?;
    let reader = windows::Storage::Streams::DataReader::CreateDataReader(&stream)
        .map_err(|e| format!("DataReader: {e}"))?;
    let load_op = reader.LoadAsync(bgra.len() as u32).map_err(|e| format!("LoadAsync: {e}"))?;
    pump_sta_messages(100);
    load_op.GetResults().map_err(|e| format!("LoadAsync result: {e}"))?;
    let buffer = reader.ReadBuffer(bgra.len() as u32).map_err(|e| format!("ReadBuffer: {e}"))?;
    bitmap.CopyFromBuffer(&buffer).map_err(|e| format!("CopyFromBuffer: {e}"))?;

    let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|e| format!("OcrEngine: {e}"))?;
    let recognize_op = engine.RecognizeAsync(&bitmap).map_err(|e| format!("RecognizeAsync: {e}"))?;
    pump_sta_messages(5000);
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
            // Get bounding box spanning all words in this line
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

            matches.push(format!(
                "\"{}\" at ({:.0}, {:.0}) size ({:.0}x{:.0}) center ({:.0}, {:.0})",
                line_text,
                min_x + offset_x, min_y + offset_y,
                max_x - min_x, max_y - min_y,
                (min_x + max_x) / 2.0 + offset_x, (min_y + max_y) / 2.0 + offset_y,
            ));
        }

        // Also check individual words
        let words = line.Words().map_err(|e| format!("Words: {e}"))?;
        for j in 0..words.Size().unwrap_or(0) {
            let word = words.GetAt(j).map_err(|e| format!("GetAt word: {e}"))?;
            let word_text = word.Text().map(|s| s.to_string()).unwrap_or_default();
            if word_text.to_lowercase().contains(search) && !line_text.to_lowercase().contains(search) {
                let rect = word.BoundingRect().map_err(|e| format!("BoundingRect: {e}"))?;
                matches.push(format!(
                    "\"{}\" at ({:.0}, {:.0}) size ({:.0}x{:.0}) center ({:.0}, {:.0})",
                    word_text,
                    rect.X as f64 + offset_x, rect.Y as f64 + offset_y,
                    rect.Width, rect.Height,
                    rect.X as f64 + rect.Width as f64 / 2.0 + offset_x,
                    rect.Y as f64 + rect.Height as f64 / 2.0 + offset_y,
                ));
            }
        }
    }

    if matches.is_empty() {
        Ok(format!("Text '{search}' not found on screen"))
    } else {
        Ok(format!("Found {} match(es):\n{}", matches.len(), matches.join("\n")))
    }
}

#[cfg(not(windows))]
pub fn tool_ocr_find_text(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: ocr_find_text is only available on Windows".to_string())
}

// ─── UI Automation tools ──────────────────────────────────────────────────────

/// Get the UI element tree of a window using UI Automation.
#[cfg(windows)]
pub fn tool_get_ui_tree(args: &Value) -> NativeToolResult {
    let title_filter = args.get("title").and_then(|v| v.as_str());
    let max_depth = args.get("depth").and_then(parse_int).unwrap_or(3).min(8) as usize;

    // Get target window HWND
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return NativeToolResult::text_only(format!("No window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return NativeToolResult::text_only("No active window".to_string()),
        }
    };

    // Run on STA thread (COM UI Automation requires it)
    let result = std::thread::spawn(move || {
        ui_tree_winrt(hwnd, max_depth)
    }).join().unwrap_or_else(|_| Err("UI tree thread panicked".to_string()));

    match result {
        Ok(tree) => NativeToolResult::text_only(tree),
        Err(e) => NativeToolResult::text_only(format!("UI tree error: {e}")),
    }
}

/// Internal: traverse UI Automation tree via COM.
#[cfg(windows)]
fn ui_tree_winrt(hwnd: isize, max_depth: usize) -> Result<String, String> {
    use windows::Win32::UI::Accessibility::*;
    use windows::Win32::System::Com::{CoInitializeEx, CoCreateInstance, COINIT_APARTMENTTHREADED, CLSCTX_INPROC_SERVER};
    use windows::Win32::Foundation::HWND as WIN32_HWND;
    use windows::core::HRESULT;

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() && hr != HRESULT(1) {
            return Err(format!("CoInitializeEx failed: {hr:?}"));
        }
    }

    let automation: IUIAutomation = unsafe {
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
    }.map_err(|e| format!("CoCreateInstance UIAutomation: {e}"))?;

    let root = unsafe {
        automation.ElementFromHandle(WIN32_HWND(hwnd as *mut _))
    }.map_err(|e| format!("ElementFromHandle: {e}"))?;

    let walker = unsafe {
        automation.ControlViewWalker()
    }.map_err(|e| format!("ControlViewWalker: {e}"))?;

    let mut output = String::new();
    let mut total_chars = 0usize;
    const MAX_CHARS: usize = 50_000;

    // Get root element info
    if let Ok(info) = get_element_info(&root) {
        output.push_str(&info);
        output.push('\n');
        total_chars += info.len() + 1;
    }

    // Recursive traversal
    fn traverse(
        walker: &IUIAutomationTreeWalker,
        parent: &IUIAutomationElement,
        depth: usize,
        max_depth: usize,
        output: &mut String,
        total_chars: &mut usize,
    ) {
        if depth >= max_depth || *total_chars >= MAX_CHARS {
            return;
        }
        let first_child = unsafe { walker.GetFirstChildElement(parent) };
        let mut current = match first_child {
            Ok(c) => c,
            Err(_) => return,
        };
        loop {
            let indent = "  ".repeat(depth);
            if let Ok(info) = get_element_info(&current) {
                let line = format!("{indent}{info}\n");
                *total_chars += line.len();
                output.push_str(&line);
                if *total_chars >= MAX_CHARS {
                    output.push_str("... (truncated at 50KB)\n");
                    return;
                }
            }
            traverse(walker, &current, depth + 1, max_depth, output, total_chars);
            match unsafe { walker.GetNextSiblingElement(&current) } {
                Ok(next) => current = next,
                Err(_) => break,
            }
        }
    }

    traverse(&walker, &root, 1, max_depth, &mut output, &mut total_chars);

    if output.is_empty() {
        Ok("(empty UI tree)".to_string())
    } else {
        Ok(output)
    }
}

#[cfg(windows)]
fn get_element_info(elem: &windows::Win32::UI::Accessibility::IUIAutomationElement) -> Result<String, String> {
    let name = unsafe { elem.CurrentName() }
        .map(|s| s.to_string())
        .unwrap_or_default();
    let control_type = unsafe { elem.CurrentControlType() }
        .map(|ct| control_type_name(ct.0))
        .unwrap_or_else(|_| "Unknown".to_string());

    if name.is_empty() {
        Ok(format!("[{control_type}]"))
    } else {
        // Truncate long names
        let display_name = if name.len() > 80 {
            format!("{}...", &name[..80])
        } else {
            name
        };
        Ok(format!("[{control_type}] \"{display_name}\""))
    }
}

#[cfg(windows)]
fn control_type_name(id: i32) -> String {
    match id {
        50000 => "Button",
        50001 => "Calendar",
        50002 => "CheckBox",
        50003 => "ComboBox",
        50004 => "Edit",
        50005 => "Hyperlink",
        50006 => "Image",
        50007 => "ListItem",
        50008 => "List",
        50009 => "Menu",
        50010 => "MenuBar",
        50011 => "MenuItem",
        50012 => "ProgressBar",
        50013 => "RadioButton",
        50014 => "ScrollBar",
        50015 => "Slider",
        50016 => "Spinner",
        50017 => "StatusBar",
        50018 => "Tab",
        50019 => "TabItem",
        50020 => "Text",
        50021 => "ToolBar",
        50022 => "ToolTip",
        50023 => "Tree",
        50024 => "TreeItem",
        50025 => "Custom",
        50026 => "Group",
        50027 => "Thumb",
        50028 => "DataGrid",
        50029 => "DataItem",
        50030 => "Document",
        50031 => "SplitButton",
        50032 => "Window",
        50033 => "Pane",
        50034 => "Header",
        50035 => "HeaderItem",
        50036 => "Table",
        50037 => "TitleBar",
        50038 => "Separator",
        _ => return format!("UIA_{id}"),
    }.to_string()
}

#[cfg(not(windows))]
pub fn tool_get_ui_tree(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: get_ui_tree is only available on Windows".to_string())
}

/// Find a UI Automation element by name or control type and click it.
#[cfg(windows)]
pub fn tool_click_ui_element(args: &Value) -> NativeToolResult {
    let name_filter = args.get("name").and_then(|v| v.as_str());
    let type_filter = args.get("control_type").and_then(|v| v.as_str());

    if name_filter.is_none() && type_filter.is_none() {
        return NativeToolResult::text_only("Error: at least 'name' or 'control_type' is required".to_string());
    }

    let title_filter = args.get("title").and_then(|v| v.as_str());
    let delay_ms = args.get("delay_ms").and_then(parse_int).unwrap_or(500) as u64;

    // Get target window HWND
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return NativeToolResult::text_only(format!("No window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return NativeToolResult::text_only("No active window".to_string()),
        }
    };

    let name_owned = name_filter.map(|s| s.to_lowercase());
    let type_owned = type_filter.map(|s| s.to_lowercase());

    // Find and click the element on STA thread
    let result = std::thread::spawn(move || {
        find_and_click_element(hwnd, name_owned.as_deref(), type_owned.as_deref())
    }).join().unwrap_or_else(|_| Err("UI Automation thread panicked".to_string()));

    match result {
        Ok((x, y, element_desc)) => {
            // Click at the element's center coordinates
            let click_args = serde_json::json!({
                "x": x,
                "y": y,
                "button": "left",
                "delay_ms": delay_ms,
            });
            let mut result = tool_click_screen(&click_args);
            result.text = format!("Clicked UI element {element_desc} at ({x}, {y}). {}", result.text);
            result
        }
        Err(e) => NativeToolResult::text_only(format!("Error: {e}")),
    }
}

/// Shared UI element info returned by find_ui_element / find_ui_elements_all
#[cfg(windows)]
struct UiElementInfo {
    cx: i32,
    cy: i32,
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    name: String,
    control_type: String,
}

#[cfg(windows)]
impl UiElementInfo {
    fn desc(&self) -> String {
        if self.name.is_empty() {
            format!("[{}]", self.control_type)
        } else {
            format!("[{}] \"{}\"", self.control_type, self.name)
        }
    }
}

/// Initialize COM + UI Automation and find the first matching element in a window.
#[cfg(windows)]
fn find_ui_element(hwnd: isize, name_filter: Option<&str>, type_filter: Option<&str>) -> Result<UiElementInfo, String> {
    let results = find_ui_elements_all(hwnd, name_filter, type_filter, 1)?;
    results.into_iter().next().ok_or_else(|| {
        let filter_desc = match (name_filter, type_filter) {
            (Some(n), Some(t)) => format!("name='{n}', type='{t}'"),
            (Some(n), None) => format!("name='{n}'"),
            (None, Some(t)) => format!("type='{t}'"),
            (None, None) => "no filter".to_string(),
        };
        format!("No UI element found matching {filter_desc}")
    })
}

/// Initialize COM + UI Automation and find ALL matching elements (up to max_results).
#[cfg(windows)]
fn find_ui_elements_all(hwnd: isize, name_filter: Option<&str>, type_filter: Option<&str>, max_results: usize) -> Result<Vec<UiElementInfo>, String> {
    use windows::Win32::UI::Accessibility::*;
    use windows::Win32::System::Com::{CoInitializeEx, CoCreateInstance, COINIT_APARTMENTTHREADED, CLSCTX_INPROC_SERVER};
    use windows::Win32::Foundation::HWND as WIN32_HWND;
    use windows::core::HRESULT;

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() && hr != HRESULT(1) {
            return Err(format!("CoInitializeEx failed: {hr:?}"));
        }
    }

    let automation: IUIAutomation = unsafe {
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
    }.map_err(|e| format!("CoCreateInstance: {e}"))?;

    let root = unsafe {
        automation.ElementFromHandle(WIN32_HWND(hwnd as *mut _))
    }.map_err(|e| format!("ElementFromHandle: {e}"))?;

    let walker = unsafe {
        automation.ControlViewWalker()
    }.map_err(|e| format!("ControlViewWalker: {e}"))?;

    let mut results = Vec::new();

    fn search(
        walker: &IUIAutomationTreeWalker,
        parent: &IUIAutomationElement,
        name_filter: Option<&str>,
        type_filter: Option<&str>,
        depth: usize,
        results: &mut Vec<UiElementInfo>,
        max_results: usize,
    ) {
        if depth > 8 || results.len() >= max_results {
            return;
        }

        let name = unsafe { parent.CurrentName() }.map(|s| s.to_string()).unwrap_or_default();
        let control_type = unsafe { parent.CurrentControlType() }
            .map(|ct| control_type_name(ct.0))
            .unwrap_or_default();

        let name_match = name_filter.map_or(true, |f| name.to_lowercase().contains(f));
        let type_match = type_filter.map_or(true, |f| control_type.to_lowercase().contains(f));

        if name_match && type_match && (!name.is_empty() || type_filter.is_some()) {
            let rect = unsafe { parent.CurrentBoundingRectangle() };
            if let Ok(r) = rect {
                if r.right > r.left && r.bottom > r.top {
                    results.push(UiElementInfo {
                        cx: ((r.left + r.right) / 2) as i32,
                        cy: ((r.top + r.bottom) / 2) as i32,
                        left: r.left as i32,
                        top: r.top as i32,
                        width: (r.right - r.left) as i32,
                        height: (r.bottom - r.top) as i32,
                        name: name.clone(),
                        control_type: control_type.clone(),
                    });
                    if results.len() >= max_results {
                        return;
                    }
                }
            }
        }

        let first_child = unsafe { walker.GetFirstChildElement(parent) };
        let mut current = match first_child {
            Ok(c) => c,
            Err(_) => return,
        };
        loop {
            search(walker, &current, name_filter, type_filter, depth + 1, results, max_results);
            if results.len() >= max_results {
                return;
            }
            match unsafe { walker.GetNextSiblingElement(&current) } {
                Ok(next) => current = next,
                Err(_) => break,
            }
        }
    }

    search(&walker, &root, name_filter, type_filter, 0, &mut results, max_results);
    Ok(results)
}

/// Legacy wrapper: find first matching element and return center coordinates + description.
#[cfg(windows)]
fn find_and_click_element(hwnd: isize, name_filter: Option<&str>, type_filter: Option<&str>) -> Result<(i32, i32, String), String> {
    let info = find_ui_element(hwnd, name_filter, type_filter)?;
    Ok((info.cx, info.cy, info.desc()))
}

#[cfg(not(windows))]
pub fn tool_click_ui_element(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: click_ui_element is only available on Windows".to_string())
}

/// Invoke a UI Automation action (invoke/toggle/expand/collapse/select/set_value) on an element.
#[cfg(windows)]
pub fn tool_invoke_ui_action(args: &Value) -> NativeToolResult {
    let name_filter = args.get("name").and_then(|v| v.as_str());
    let type_filter = args.get("control_type").and_then(|v| v.as_str());
    let action = match args.get("action").and_then(|v| v.as_str()) {
        Some(a) => a.to_lowercase(),
        None => return NativeToolResult::text_only("Error: 'action' is required (invoke/toggle/expand/collapse/select/set_value)".to_string()),
    };
    let value = args.get("value").and_then(|v| v.as_str()).map(|s| s.to_string());

    if name_filter.is_none() && type_filter.is_none() {
        return NativeToolResult::text_only("Error: at least 'name' or 'control_type' is required".to_string());
    }

    let title_filter = args.get("title").and_then(|v| v.as_str());
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return NativeToolResult::text_only(format!("No window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return NativeToolResult::text_only("No active window".to_string()),
        }
    };

    let name_owned = name_filter.map(|s| s.to_lowercase());
    let type_owned = type_filter.map(|s| s.to_lowercase());

    let result = std::thread::spawn(move || {
        invoke_ui_action_inner(hwnd, name_owned.as_deref(), type_owned.as_deref(), &action, value.as_deref())
    }).join().unwrap_or_else(|_| Err("UI Automation thread panicked".to_string()));

    match result {
        Ok(msg) => NativeToolResult::text_only(msg),
        Err(e) => NativeToolResult::text_only(format!("Error: {e}")),
    }
}

#[cfg(windows)]
fn invoke_ui_action_inner(hwnd: isize, name_filter: Option<&str>, type_filter: Option<&str>, action: &str, value: Option<&str>) -> Result<String, String> {
    use windows::Win32::UI::Accessibility::*;
    use windows::Win32::System::Com::{CoInitializeEx, CoCreateInstance, COINIT_APARTMENTTHREADED, CLSCTX_INPROC_SERVER};
    use windows::Win32::Foundation::HWND as WIN32_HWND;
    use windows::core::{HRESULT, BSTR};

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() && hr != HRESULT(1) {
            return Err(format!("CoInitializeEx failed: {hr:?}"));
        }
    }

    let automation: IUIAutomation = unsafe {
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
    }.map_err(|e| format!("CoCreateInstance: {e}"))?;

    let root = unsafe {
        automation.ElementFromHandle(WIN32_HWND(hwnd as *mut _))
    }.map_err(|e| format!("ElementFromHandle: {e}"))?;

    let walker = unsafe {
        automation.ControlViewWalker()
    }.map_err(|e| format!("ControlViewWalker: {e}"))?;

    // Find the element first (reuse search logic from find_ui_elements_all but we need the raw element)
    let element = find_raw_ui_element(&walker, &root, name_filter, type_filter, 0)
        .ok_or_else(|| {
            let filter_desc = match (name_filter, type_filter) {
                (Some(n), Some(t)) => format!("name='{n}', type='{t}'"),
                (Some(n), None) => format!("name='{n}'"),
                (None, Some(t)) => format!("type='{t}'"),
                (None, None) => "no filter".to_string(),
            };
            format!("No UI element found matching {filter_desc}")
        })?;

    let elem_name = unsafe { element.CurrentName() }.map(|s| s.to_string()).unwrap_or_default();

    match action {
        "invoke" => {
            let pattern: IUIAutomationInvokePattern = unsafe {
                element.GetCurrentPatternAs(UIA_InvokePatternId)
            }.map_err(|e| format!("Element doesn't support Invoke pattern: {e}"))?;
            unsafe { pattern.Invoke() }.map_err(|e| format!("Invoke failed: {e}"))?;
            Ok(format!("Invoked element \"{elem_name}\""))
        }
        "toggle" => {
            let pattern: IUIAutomationTogglePattern = unsafe {
                element.GetCurrentPatternAs(UIA_TogglePatternId)
            }.map_err(|e| format!("Element doesn't support Toggle pattern: {e}"))?;
            unsafe { pattern.Toggle() }.map_err(|e| format!("Toggle failed: {e}"))?;
            let state = unsafe { pattern.CurrentToggleState() }
                .map(|s| format!("{s:?}"))
                .unwrap_or_else(|_| "unknown".to_string());
            Ok(format!("Toggled element \"{elem_name}\" → state: {state}"))
        }
        "expand" => {
            let pattern: IUIAutomationExpandCollapsePattern = unsafe {
                element.GetCurrentPatternAs(UIA_ExpandCollapsePatternId)
            }.map_err(|e| format!("Element doesn't support ExpandCollapse pattern: {e}"))?;
            unsafe { pattern.Expand() }.map_err(|e| format!("Expand failed: {e}"))?;
            Ok(format!("Expanded element \"{elem_name}\""))
        }
        "collapse" => {
            let pattern: IUIAutomationExpandCollapsePattern = unsafe {
                element.GetCurrentPatternAs(UIA_ExpandCollapsePatternId)
            }.map_err(|e| format!("Element doesn't support ExpandCollapse pattern: {e}"))?;
            unsafe { pattern.Collapse() }.map_err(|e| format!("Collapse failed: {e}"))?;
            Ok(format!("Collapsed element \"{elem_name}\""))
        }
        "select" => {
            let pattern: IUIAutomationSelectionItemPattern = unsafe {
                element.GetCurrentPatternAs(UIA_SelectionItemPatternId)
            }.map_err(|e| format!("Element doesn't support SelectionItem pattern: {e}"))?;
            unsafe { pattern.Select() }.map_err(|e| format!("Select failed: {e}"))?;
            Ok(format!("Selected element \"{elem_name}\""))
        }
        "set_value" => {
            let val = value.ok_or("'value' parameter is required for set_value action")?;
            let pattern: IUIAutomationValuePattern = unsafe {
                element.GetCurrentPatternAs(UIA_ValuePatternId)
            }.map_err(|e| format!("Element doesn't support Value pattern: {e}"))?;
            let bstr = BSTR::from(val);
            unsafe { pattern.SetValue(&bstr) }.map_err(|e| format!("SetValue failed: {e}"))?;
            Ok(format!("Set value of \"{elem_name}\" to \"{val}\""))
        }
        other => Err(format!("Unknown action '{other}'. Use: invoke, toggle, expand, collapse, select, set_value")),
    }
}

/// Recursive search returning the raw IUIAutomationElement (needed for pattern invocation).
#[cfg(windows)]
fn find_raw_ui_element(
    walker: &windows::Win32::UI::Accessibility::IUIAutomationTreeWalker,
    parent: &windows::Win32::UI::Accessibility::IUIAutomationElement,
    name_filter: Option<&str>,
    type_filter: Option<&str>,
    depth: usize,
) -> Option<windows::Win32::UI::Accessibility::IUIAutomationElement> {
    use windows::Win32::UI::Accessibility::IUIAutomationElement;

    if depth > 8 {
        return None;
    }

    let name = unsafe { parent.CurrentName() }.map(|s| s.to_string()).unwrap_or_default();
    let control_type = unsafe { parent.CurrentControlType() }
        .map(|ct| control_type_name(ct.0))
        .unwrap_or_default();

    let name_match = name_filter.map_or(true, |f| name.to_lowercase().contains(f));
    let type_match = type_filter.map_or(true, |f| control_type.to_lowercase().contains(f));

    if name_match && type_match && (!name.is_empty() || type_filter.is_some()) {
        let rect = unsafe { parent.CurrentBoundingRectangle() };
        if let Ok(r) = rect {
            if r.right > r.left && r.bottom > r.top {
                return Some(parent.clone());
            }
        }
    }

    let first_child = unsafe { walker.GetFirstChildElement(parent) };
    let mut current: IUIAutomationElement = match first_child {
        Ok(c) => c,
        Err(_) => return None,
    };
    loop {
        if let Some(found) = find_raw_ui_element(walker, &current, name_filter, type_filter, depth + 1) {
            return Some(found);
        }
        match unsafe { walker.GetNextSiblingElement(&current) } {
            Ok(next) => current = next,
            Err(_) => break,
        }
    }
    None
}

#[cfg(not(windows))]
pub fn tool_invoke_ui_action(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: invoke_ui_action is only available on Windows".to_string())
}

/// Read the current value or text of a UI element.
#[cfg(windows)]
pub fn tool_read_ui_element_value(args: &Value) -> NativeToolResult {
    let name_filter = args.get("name").and_then(|v| v.as_str());
    let type_filter = args.get("control_type").and_then(|v| v.as_str());

    if name_filter.is_none() && type_filter.is_none() {
        return NativeToolResult::text_only("Error: at least 'name' or 'control_type' is required".to_string());
    }

    let title_filter = args.get("title").and_then(|v| v.as_str());
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return NativeToolResult::text_only(format!("No window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return NativeToolResult::text_only("No active window".to_string()),
        }
    };

    let name_owned = name_filter.map(|s| s.to_lowercase());
    let type_owned = type_filter.map(|s| s.to_lowercase());

    let result = std::thread::spawn(move || {
        read_ui_element_value_inner(hwnd, name_owned.as_deref(), type_owned.as_deref())
    }).join().unwrap_or_else(|_| Err("UI Automation thread panicked".to_string()));

    match result {
        Ok(msg) => NativeToolResult::text_only(msg),
        Err(e) => NativeToolResult::text_only(format!("Error: {e}")),
    }
}

#[cfg(windows)]
fn read_ui_element_value_inner(hwnd: isize, name_filter: Option<&str>, type_filter: Option<&str>) -> Result<String, String> {
    use windows::Win32::UI::Accessibility::*;
    use windows::Win32::System::Com::{CoInitializeEx, CoCreateInstance, COINIT_APARTMENTTHREADED, CLSCTX_INPROC_SERVER};
    use windows::Win32::Foundation::HWND as WIN32_HWND;
    use windows::core::HRESULT;

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() && hr != HRESULT(1) {
            return Err(format!("CoInitializeEx failed: {hr:?}"));
        }
    }

    let automation: IUIAutomation = unsafe {
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
    }.map_err(|e| format!("CoCreateInstance: {e}"))?;

    let root = unsafe {
        automation.ElementFromHandle(WIN32_HWND(hwnd as *mut _))
    }.map_err(|e| format!("ElementFromHandle: {e}"))?;

    let walker = unsafe {
        automation.ControlViewWalker()
    }.map_err(|e| format!("ControlViewWalker: {e}"))?;

    let element = find_raw_ui_element(&walker, &root, name_filter, type_filter, 0)
        .ok_or_else(|| "No matching UI element found".to_string())?;

    let elem_name = unsafe { element.CurrentName() }.map(|s| s.to_string()).unwrap_or_default();
    let control_type = unsafe { element.CurrentControlType() }
        .map(|ct| control_type_name(ct.0))
        .unwrap_or_default();
    let rect = unsafe { element.CurrentBoundingRectangle() };
    let rect_str = rect.as_ref().map(|r| format!(" at ({}, {}) {}x{}", r.left, r.top, r.right - r.left, r.bottom - r.top)).unwrap_or_default();

    // Try ValuePattern first
    let value_result: Result<IUIAutomationValuePattern, _> = unsafe {
        element.GetCurrentPatternAs(UIA_ValuePatternId)
    };
    if let Ok(pattern) = value_result {
        if let Ok(val) = unsafe { pattern.CurrentValue() } {
            return Ok(format!("[{control_type}] \"{elem_name}\"{rect_str}\nValue: \"{val}\""));
        }
    }

    // Fallback to element name
    Ok(format!("[{control_type}] \"{elem_name}\"{rect_str}\nValue: (no ValuePattern, name shown above)"))
}

#[cfg(not(windows))]
pub fn tool_read_ui_element_value(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: read_ui_element_value is only available on Windows".to_string())
}

/// Poll until a UI element matching name/type appears.
#[cfg(windows)]
pub fn tool_wait_for_ui_element(args: &Value) -> NativeToolResult {
    let name_filter = args.get("name").and_then(|v| v.as_str());
    let type_filter = args.get("control_type").and_then(|v| v.as_str());
    let timeout_ms = args.get("timeout_ms").and_then(parse_int).unwrap_or(10000).min(30000) as u64;
    let poll_ms = args.get("poll_ms").and_then(parse_int).unwrap_or(500).max(100) as u64;

    if name_filter.is_none() && type_filter.is_none() {
        return NativeToolResult::text_only("Error: at least 'name' or 'control_type' is required".to_string());
    }

    let title_filter = args.get("title").and_then(|v| v.as_str());
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return NativeToolResult::text_only(format!("No window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return NativeToolResult::text_only("No active window".to_string()),
        }
    };

    let name_owned = name_filter.map(|s| s.to_lowercase());
    let type_owned = type_filter.map(|s| s.to_lowercase());

    let start = std::time::Instant::now();
    loop {
        let n = name_owned.clone();
        let t = type_owned.clone();
        let result = std::thread::spawn(move || {
            find_ui_element(hwnd, n.as_deref(), t.as_deref())
        }).join().unwrap_or_else(|_| Err("thread panicked".to_string()));

        if let Ok(info) = result {
            return NativeToolResult::text_only(format!(
                "Element appeared: {} at ({}, {}) after {}ms",
                info.desc(), info.cx, info.cy, start.elapsed().as_millis()
            ));
        }

        if start.elapsed().as_millis() >= timeout_ms as u128 {
            let filter_desc = match (name_owned.as_deref(), type_owned.as_deref()) {
                (Some(n), Some(t)) => format!("name='{n}', type='{t}'"),
                (Some(n), None) => format!("name='{n}'"),
                (None, Some(t)) => format!("type='{t}'"),
                (None, None) => "no filter".to_string(),
            };
            return NativeToolResult::text_only(format!(
                "Timeout: element matching {filter_desc} not found after {timeout_ms}ms"
            ));
        }

        std::thread::sleep(std::time::Duration::from_millis(poll_ms));
    }
}

#[cfg(not(windows))]
pub fn tool_wait_for_ui_element(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: wait_for_ui_element is only available on Windows".to_string())
}

// ─── Clipboard image tool ─────────────────────────────────────────────────────

/// Read or write an image from/to the clipboard.
#[cfg(windows)]
pub fn tool_clipboard_image(args: &Value) -> NativeToolResult {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("read");

    match action {
        "read" => clipboard_image_read(),
        "write" => {
            let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;
            clipboard_image_write(monitor_idx)
        }
        other => NativeToolResult::text_only(format!("Unknown action '{other}'. Use 'read' or 'write'.")),
    }
}

#[cfg(windows)]
fn clipboard_image_read() -> NativeToolResult {
    unsafe {
        if win32::OpenClipboard(0) == 0 {
            return NativeToolResult::text_only("Error: could not open clipboard".to_string());
        }

        let handle = win32::GetClipboardData(win32::CF_DIB);
        if handle == 0 {
            win32::CloseClipboard();
            return NativeToolResult::text_only("No image in clipboard (CF_DIB format)".to_string());
        }

        let size = win32::GlobalSize(handle);
        if size == 0 {
            win32::CloseClipboard();
            return NativeToolResult::text_only("Error: GlobalSize returned 0".to_string());
        }

        let ptr = win32::GlobalLock(handle);
        if ptr.is_null() {
            win32::CloseClipboard();
            return NativeToolResult::text_only("Error: GlobalLock failed".to_string());
        }

        let data = std::slice::from_raw_parts(ptr, size);

        // Parse BITMAPINFOHEADER (first 40 bytes minimum)
        if data.len() < 40 {
            win32::GlobalUnlock(handle);
            win32::CloseClipboard();
            return NativeToolResult::text_only("Error: DIB data too small".to_string());
        }

        let width = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let height_raw = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let bit_count = u16::from_le_bytes([data[14], data[15]]);
        let compression = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);

        if compression != 0 {
            // BI_RGB = 0
            win32::GlobalUnlock(handle);
            win32::CloseClipboard();
            return NativeToolResult::text_only(format!("Error: unsupported DIB compression type {compression} (only BI_RGB supported)"));
        }

        let height = height_raw.unsigned_abs() as u32;
        let width = width.unsigned_abs() as u32;
        let top_down = height_raw < 0;

        // Calculate pixel data offset (after header + optional color table)
        let header_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let pixel_offset = header_size;
        let bytes_per_pixel = (bit_count / 8) as usize;
        let row_size = ((width as usize * bytes_per_pixel + 3) / 4) * 4; // DWORD-aligned rows

        if pixel_offset + row_size * height as usize > data.len() {
            win32::GlobalUnlock(handle);
            win32::CloseClipboard();
            return NativeToolResult::text_only("Error: DIB pixel data exceeds buffer size".to_string());
        }

        // Convert BGR(A) to RGBA
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        for y in 0..height {
            let src_row = if top_down { y } else { height - 1 - y };
            let src_start = pixel_offset + src_row as usize * row_size;
            for x in 0..width {
                let src_idx = src_start + x as usize * bytes_per_pixel;
                let dst_idx = (y * width + x) as usize * 4;
                rgba[dst_idx] = data[src_idx + 2];     // R (from B)
                rgba[dst_idx + 1] = data[src_idx + 1]; // G
                rgba[dst_idx + 2] = data[src_idx];     // B (from R)
                rgba[dst_idx + 3] = if bytes_per_pixel == 4 { data[src_idx + 3] } else { 255 }; // A
            }
        }

        win32::GlobalUnlock(handle);
        win32::CloseClipboard();

        // Encode to PNG
        let mut png_data = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png_data);
        if let Err(e) = image::ImageEncoder::write_image(
            encoder,
            &rgba,
            width,
            height,
            image::ExtendedColorType::Rgba8,
        ) {
            return NativeToolResult::text_only(format!("Error encoding PNG: {e}"));
        }

        NativeToolResult::with_image(
            format!("Clipboard image: {width}x{height} ({bit_count}bpp)"),
            png_data,
        )
    }
}

#[cfg(windows)]
fn clipboard_image_write(monitor_idx: usize) -> NativeToolResult {
    // Capture the screen
    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return NativeToolResult::text_only(format!("Error listing monitors: {e}")),
    };
    let monitor = match monitors.get(monitor_idx) {
        Some(m) => m,
        None => return NativeToolResult::text_only(format!("Monitor {monitor_idx} not found")),
    };
    let img = match monitor.capture_image() {
        Ok(i) => i,
        Err(e) => return NativeToolResult::text_only(format!("Error capturing screen: {e}")),
    };

    let width = img.width();
    let height = img.height();
    let raw = img.into_raw();

    // Build DIB: BITMAPINFOHEADER (40 bytes) + pixel data (BGR, bottom-up, DWORD-aligned)
    let bytes_per_pixel = 3usize;
    let row_size = ((width as usize * bytes_per_pixel + 3) / 4) * 4;
    let pixel_data_size = row_size * height as usize;
    let total_size = 40 + pixel_data_size;

    unsafe {
        let hmem = win32::GlobalAlloc(win32::GMEM_MOVEABLE, total_size);
        if hmem == 0 {
            return NativeToolResult::text_only("Error: GlobalAlloc failed".to_string());
        }

        let ptr = win32::GlobalLock(hmem);
        if ptr.is_null() {
            return NativeToolResult::text_only("Error: GlobalLock failed".to_string());
        }

        let buf = std::slice::from_raw_parts_mut(ptr, total_size);

        // BITMAPINFOHEADER
        buf[0..4].copy_from_slice(&40u32.to_le_bytes());        // biSize
        buf[4..8].copy_from_slice(&(width as i32).to_le_bytes()); // biWidth
        buf[8..12].copy_from_slice(&(height as i32).to_le_bytes()); // biHeight (positive = bottom-up)
        buf[12..14].copy_from_slice(&1u16.to_le_bytes());       // biPlanes
        buf[14..16].copy_from_slice(&24u16.to_le_bytes());      // biBitCount
        buf[16..20].copy_from_slice(&0u32.to_le_bytes());       // biCompression = BI_RGB
        buf[20..24].copy_from_slice(&(pixel_data_size as u32).to_le_bytes()); // biSizeImage
        buf[24..40].fill(0); // remaining fields = 0

        // Convert RGBA top-down to BGR bottom-up
        for y in 0..height {
            let src_row = y as usize;
            let dst_row = (height - 1 - y) as usize;
            let dst_start = 40 + dst_row * row_size;
            for x in 0..width {
                let src_idx = (src_row * width as usize + x as usize) * 4;
                let dst_idx = dst_start + x as usize * bytes_per_pixel;
                buf[dst_idx] = raw[src_idx + 2];     // B
                buf[dst_idx + 1] = raw[src_idx + 1]; // G
                buf[dst_idx + 2] = raw[src_idx];     // R
            }
        }

        win32::GlobalUnlock(hmem);

        if win32::OpenClipboard(0) == 0 {
            return NativeToolResult::text_only("Error: could not open clipboard".to_string());
        }
        win32::EmptyClipboard();
        let result = win32::SetClipboardData(win32::CF_DIB, hmem);
        win32::CloseClipboard();

        if result == 0 {
            NativeToolResult::text_only("Error: SetClipboardData failed".to_string())
        } else {
            NativeToolResult::text_only(format!("Screenshot ({width}x{height}) copied to clipboard"))
        }
    }
}

#[cfg(not(windows))]
pub fn tool_clipboard_image(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: clipboard_image is only available on Windows".to_string())
}

// ─── Find UI elements tool ───────────────────────────────────────────────────

/// Find all UI elements matching name/type criteria, returning positions and details.
#[cfg(windows)]
pub fn tool_find_ui_elements(args: &Value) -> NativeToolResult {
    let name_filter = args.get("name").and_then(|v| v.as_str());
    let type_filter = args.get("control_type").and_then(|v| v.as_str());
    let max_results = args.get("max_results").and_then(parse_int).unwrap_or(10).min(50) as usize;

    if name_filter.is_none() && type_filter.is_none() {
        return NativeToolResult::text_only("Error: at least 'name' or 'control_type' is required".to_string());
    }

    let title_filter = args.get("title").and_then(|v| v.as_str());
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return NativeToolResult::text_only(format!("No window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return NativeToolResult::text_only("No active window".to_string()),
        }
    };

    let name_owned = name_filter.map(|s| s.to_lowercase());
    let type_owned = type_filter.map(|s| s.to_lowercase());

    let result = std::thread::spawn(move || {
        find_ui_elements_all(hwnd, name_owned.as_deref(), type_owned.as_deref(), max_results)
    }).join().unwrap_or_else(|_| Err("UI Automation thread panicked".to_string()));

    match result {
        Ok(elements) => {
            if elements.is_empty() {
                return NativeToolResult::text_only("No matching UI elements found".to_string());
            }
            let lines: Vec<String> = elements.iter().enumerate().map(|(i, e)| {
                format!("{}. {} at ({}, {}) size {}x{} center ({}, {})",
                    i + 1, e.desc(), e.left, e.top, e.width, e.height, e.cx, e.cy)
            }).collect();
            NativeToolResult::text_only(format!("Found {} element(s):\n{}", elements.len(), lines.join("\n")))
        }
        Err(e) => NativeToolResult::text_only(format!("Error: {e}")),
    }
}

#[cfg(not(windows))]
pub fn tool_find_ui_elements(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: find_ui_elements is only available on Windows".to_string())
}
