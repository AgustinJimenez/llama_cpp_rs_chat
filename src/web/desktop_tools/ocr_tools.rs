//! OCR text recognition tools: Windows WinRT, macOS Vision, Linux tesseract.

use serde_json::Value;

use super::NativeToolResult;
use super::parse_int;

#[cfg(windows)]
use super::win32;
#[cfg(target_os = "macos")]
use super::macos as win32;
#[cfg(target_os = "linux")]
use super::linux as win32;

/// Structured OCR match result with bounding box info.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub(super) struct OcrMatch {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub center_x: f64,
    pub center_y: f64,
    pub confidence: f64,
}

// ─── Unified OCR dispatcher ──────────────────────────────────────────────────

/// Cross-platform OCR find text dispatcher.
/// Windows: WinRT, macOS: Vision (tesseract fallback), Linux: tesseract.
#[cfg(windows)]
pub(super) fn ocr_find_text(img: &image::RgbaImage, search: &str, offset_x: f64, offset_y: f64) -> Result<Vec<OcrMatch>, String> {
    ocr_find_text_winrt(img, search, offset_x, offset_y)
}

#[cfg(target_os = "macos")]
pub(super) fn ocr_find_text(img: &image::RgbaImage, search: &str, offset_x: f64, offset_y: f64) -> Result<Vec<OcrMatch>, String> {
    // Try Vision framework first, fall back to tesseract if swift is unavailable
    match ocr_find_text_vision(img, search, offset_x, offset_y, None) {
        Ok(matches) => Ok(matches),
        Err(_) => ocr_find_text_tesseract(img, search, offset_x, offset_y),
    }
}

#[cfg(target_os = "linux")]
pub(super) fn ocr_find_text(img: &image::RgbaImage, search: &str, offset_x: f64, offset_y: f64) -> Result<Vec<OcrMatch>, String> {
    ocr_find_text_tesseract(img, search, offset_x, offset_y)
}

// ─── OCR tools ────────────────────────────────────────────────────────────────

/// OCR: extract text from the screen using Windows.Media.Ocr (WinRT).
/// Supports optional `window` param to auto-crop to a window's rect.
#[cfg(windows)]
pub fn tool_ocr_screen(args: &Value) -> NativeToolResult {
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    // Capture screen
    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return super::tool_error("ocr_screen", e),
    };
    if monitor_idx >= monitors.len() {
        return super::tool_error("ocr_screen", format!(
            "monitor {monitor_idx} out of range (0..{})", monitors.len()
        ));
    }
    let img = match monitors[monitor_idx].capture_image() {
        Ok(i) => i,
        Err(e) => return super::tool_error("ocr_screen", format!("capturing: {e}")),
    };

    let full_w = img.width();
    let full_h = img.height();

    // Determine crop region: window param takes priority, then explicit x/y/width/height
    let (work_img, region_desc) = if let Some(window_filter) = args.get("window").and_then(|v| v.as_str()) {
        match win32::find_window_by_filter(window_filter) {
            Some((hwnd, winfo)) => {
                let rect = win32::get_window_rect(hwnd);
                let rx = rect.left.max(0) as u32;
                let ry = rect.top.max(0) as u32;
                let rw = ((rect.right - rect.left).max(1) as u32).min(full_w.saturating_sub(rx));
                let rh = ((rect.bottom - rect.top).max(1) as u32).min(full_h.saturating_sub(ry));
                let cropped = image::imageops::crop_imm(&img, rx, ry, rw, rh).to_image();
                (cropped, format!(" (window \"{}\" {rx},{ry} {rw}x{rh})", winfo.title))
            }
            None => return super::tool_error("ocr_screen", format!("no window matches '{window_filter}'")),
        }
    } else {
        let region_x = args.get("x").and_then(parse_int).map(|v| v as u32);
        let region_y = args.get("y").and_then(parse_int).map(|v| v as u32);
        let region_w = args.get("width").and_then(parse_int).map(|v| v as u32);
        let region_h = args.get("height").and_then(parse_int).map(|v| v as u32);

        if let (Some(rx), Some(ry), Some(rw), Some(rh)) = (region_x, region_y, region_w, region_h) {
            if rx + rw > full_w || ry + rh > full_h {
                return super::tool_error("ocr_screen", format!(
                    "region ({rx},{ry} {rw}x{rh}) exceeds screen ({full_w}x{full_h})"
                ));
            }
            let cropped: image::RgbaImage = image::imageops::crop_imm(&img, rx, ry, rw, rh).to_image();
            (cropped, format!(" (region {rx},{ry} {rw}x{rh})"))
        } else {
            (img, format!(" ({full_w}x{full_h})"))
        }
    };

    // Run OCR on a temporary STA thread (WinRT requires STA)
    let result = super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || {
        ocr_image_winrt(&work_img)
    }).and_then(|r| r);

    match result {
        Ok(text) => {
            if text.is_empty() {
                NativeToolResult::text_only(format!("OCR{region_desc}: no text detected"))
            } else {
                let line_count = text.lines().count();
                NativeToolResult::text_only(format!(
                    "OCR{region_desc}: {line_count} lines\n{text}"
                ))
            }
        }
        Err(e) => super::tool_error("ocr_screen", format!("OCR: {e}")),
    }
}

/// Internal: run OCR via Windows.Media.Ocr WinRT API. Must be called from STA thread.
#[cfg(windows)]
pub(super) fn ocr_image_winrt(img: &image::RgbaImage) -> Result<String, String> {
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

/// Pump STA message loop for a duration (ms). Required for WinRT async ops to complete on STA.
#[cfg(windows)]
pub(super) fn pump_sta_messages(duration_ms: u64) {
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
        if super::desktop_call_cancelled() {
            break;
        }
        unsafe {
            while PeekMessageW(&mut msg, 0, 0, 0, 1) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if super::interruptible_sleep(std::time::Duration::from_millis(2)).is_err() {
            break;
        }
    }
}

#[cfg(windows)]
fn wait_for_winrt_async(
    op_name: &str,
    duration_ms: u64,
    mut status: impl FnMut() -> Result<i32, String>,
    mut cancel: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
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

    loop {
        if super::desktop_call_cancelled() {
            let _ = cancel();
            return Err(super::desktop_cancel_error());
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

        super::interruptible_sleep(std::time::Duration::from_millis(2))?;
    }
}

/// OCR via tesseract CLI (macOS/Linux).
#[cfg(not(windows))]
pub(super) fn ocr_image_tesseract(img: &image::RgbaImage, language: Option<&str>) -> Result<String, String> {
    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join("llama_chat_ocr_tmp.png");
    let dyn_img = image::DynamicImage::ImageRgba8(img.clone());
    dyn_img.save(&tmp_path).map_err(|e| format!("Failed to save temp image: {e}"))?;
    let mut cmd = std::process::Command::new("tesseract");
    cmd.arg(tmp_path.to_str().unwrap_or(""))
        .arg("stdout");
    if let Some(lang) = language {
        cmd.arg("-l").arg(lang);
    }
    let output = cmd.output()
        .map_err(|e| format!("tesseract not found or failed: {e}. Install: brew install tesseract (macOS) or sudo apt install tesseract-ocr (Linux)"))?;
    let _ = std::fs::remove_file(&tmp_path);
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!("tesseract error: {}", String::from_utf8_lossy(&output.stderr).trim()))
    }
}

/// OCR with bounding boxes via tesseract TSV output (Linux, or macOS/Linux fallback).
#[cfg(not(windows))]
pub(super) fn ocr_find_text_tesseract(img: &image::RgbaImage, search: &str, offset_x: f64, offset_y: f64) -> Result<Vec<OcrMatch>, String> {
    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join("llama_chat_ocr_find_tmp.png");
    let dyn_img = image::DynamicImage::ImageRgba8(img.clone());
    dyn_img.save(&tmp_path).map_err(|e| format!("Failed to save temp image: {e}"))?;
    let mut cmd = std::process::Command::new("tesseract");
    cmd.arg(tmp_path.to_str().unwrap_or(""))
        .arg("stdout")
        .arg("--psm").arg("3")
        .arg("tsv");
    let output = cmd.output()
        .map_err(|e| format!("tesseract failed: {e}"))?;
    let _ = std::fs::remove_file(&tmp_path);
    if !output.status.success() {
        return Err(format!("tesseract error: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let tsv = String::from_utf8_lossy(&output.stdout);
    let search_lower = search.to_lowercase();
    let mut matches = Vec::new();
    for line in tsv.lines().skip(1) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() >= 12 {
            let word = cols[11].trim();
            if word.to_lowercase().contains(&search_lower) {
                let x: f64 = cols[6].parse().unwrap_or(0.0);
                let y: f64 = cols[7].parse().unwrap_or(0.0);
                let w: f64 = cols[8].parse().unwrap_or(0.0);
                let h: f64 = cols[9].parse().unwrap_or(0.0);
                let confidence: f64 = cols[10].parse::<f64>().unwrap_or(0.0) / 100.0;
                matches.push(OcrMatch {
                    text: word.to_string(),
                    x: x + offset_x,
                    y: y + offset_y,
                    width: w,
                    height: h,
                    center_x: x + w / 2.0 + offset_x,
                    center_y: y + h / 2.0 + offset_y,
                    confidence,
                });
            }
        }
    }
    Ok(matches)
}

// ─── macOS Vision framework OCR ──────────────────────────────────────────────

/// The embedded Swift script for plain text OCR via the Vision framework.
/// Expects the image file path as the first command-line argument.
/// Optional language hint as argument 2.
/// Outputs each recognized text observation on stdout, one per line.
#[cfg(target_os = "macos")]
const SWIFT_OCR_TEXT_SCRIPT: &str = r#"
import Foundation
import Vision

let args = CommandLine.arguments
guard args.count > 1 else {
    fputs("Usage: swift - <image_path> [language]\n", stderr)
    exit(1)
}
let imagePath = args[1]
guard let image = NSImage(contentsOfFile: imagePath),
      let tiffData = image.tiffRepresentation,
      let bitmap = NSBitmapImageRep(data: tiffData),
      let cgImage = bitmap.cgImage else {
    fputs("Error: could not load image at \(imagePath)\n", stderr)
    exit(1)
}

let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
if args.count > 2 {
    request.recognitionLanguages = [args[2]]
}

let handler = VNImageRequestHandler(cgImage: cgImage, options: [:])
do {
    try handler.perform([request])
} catch {
    fputs("Vision error: \(error.localizedDescription)\n", stderr)
    exit(1)
}

guard let observations = request.results else { exit(0) }
for observation in observations {
    if let candidate = observation.topCandidates(1).first {
        print(candidate.string)
    }
}
"#;

/// The embedded Swift script for OCR with bounding boxes via the Vision framework.
/// Expects the image file path as the first command-line argument.
/// Outputs one line per observation: TEXT\tX\tY\tW\tH\tCONFIDENCE (pixel coords, origin top-left).
/// The image width and height are passed as arguments 2 and 3.
/// Optional language hint as argument 4.
#[cfg(target_os = "macos")]
const SWIFT_OCR_FIND_SCRIPT: &str = r#"
import Foundation
import Vision

let args = CommandLine.arguments
guard args.count > 3 else {
    fputs("Usage: swift - <image_path> <width> <height> [language]\n", stderr)
    exit(1)
}
let imagePath = args[1]
let imgWidth = Double(args[2]) ?? 0
let imgHeight = Double(args[3]) ?? 0

guard let image = NSImage(contentsOfFile: imagePath),
      let tiffData = image.tiffRepresentation,
      let bitmap = NSBitmapImageRep(data: tiffData),
      let cgImage = bitmap.cgImage else {
    fputs("Error: could not load image at \(imagePath)\n", stderr)
    exit(1)
}

let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
if args.count > 4 {
    request.recognitionLanguages = [args[4]]
}

let handler = VNImageRequestHandler(cgImage: cgImage, options: [:])
do {
    try handler.perform([request])
} catch {
    fputs("Vision error: \(error.localizedDescription)\n", stderr)
    exit(1)
}

guard let observations = request.results else { exit(0) }
for observation in observations {
    if let candidate = observation.topCandidates(1).first {
        // boundingBox is normalized (0..1), origin bottom-left
        let box = observation.boundingBox
        let x = box.origin.x * imgWidth
        let y = (1.0 - box.origin.y - box.size.height) * imgHeight  // flip Y
        let w = box.size.width * imgWidth
        let h = box.size.height * imgHeight
        let conf = observation.confidence
        print("\(candidate.string)\t\(Int(x))\t\(Int(y))\t\(Int(w))\t\(Int(h))\t\(String(format: "%.4f", conf))")
    }
}
"#;

/// OCR via macOS Vision framework (VNRecognizeTextRequest).
/// Writes the image to a temp PNG, runs an embedded Swift script, parses stdout.
#[cfg(target_os = "macos")]
pub(super) fn ocr_image_vision(img: &image::RgbaImage, language: Option<&str>) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join("llama_chat_ocr_vision_tmp.png");
    let dyn_img = image::DynamicImage::ImageRgba8(img.clone());
    dyn_img.save(&tmp_path).map_err(|e| format!("Failed to save temp image: {e}"))?;

    let mut cmd = Command::new("swift");
    cmd.arg("-")
        .arg(tmp_path.to_str().unwrap_or(""));
    if let Some(lang) = language {
        cmd.arg(lang);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            format!("swift not found: {e}")
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(SWIFT_OCR_TEXT_SCRIPT.as_bytes());
    }

    let output = child.wait_with_output().map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("swift execution failed: {e}")
    })?;
    let _ = std::fs::remove_file(&tmp_path);

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!(
            "Vision OCR failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// OCR with bounding boxes via macOS Vision framework.
/// Returns OcrMatch results with pixel coordinates (origin top-left).
#[cfg(target_os = "macos")]
pub(super) fn ocr_find_text_vision(
    img: &image::RgbaImage,
    search: &str,
    offset_x: f64,
    offset_y: f64,
    language: Option<&str>,
) -> Result<Vec<OcrMatch>, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let (img_w, img_h) = (img.width(), img.height());
    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join("llama_chat_ocr_vision_find_tmp.png");
    let dyn_img = image::DynamicImage::ImageRgba8(img.clone());
    dyn_img.save(&tmp_path).map_err(|e| format!("Failed to save temp image: {e}"))?;

    let mut cmd = Command::new("swift");
    cmd.arg("-")
        .arg(tmp_path.to_str().unwrap_or(""))
        .arg(img_w.to_string())
        .arg(img_h.to_string());
    if let Some(lang) = language {
        cmd.arg(lang);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            format!("swift not found: {e}")
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(SWIFT_OCR_FIND_SCRIPT.as_bytes());
    }

    let output = child.wait_with_output().map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("swift execution failed: {e}")
    })?;
    let _ = std::fs::remove_file(&tmp_path);

    if !output.status.success() {
        return Err(format!(
            "Vision OCR failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let search_lower = search.to_lowercase();
    let mut matches = Vec::new();

    for line in stdout.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 5 {
            continue;
        }
        let text = cols[0];
        if !text.to_lowercase().contains(&search_lower) {
            continue;
        }
        let x: f64 = cols[1].parse().unwrap_or(0.0);
        let y: f64 = cols[2].parse().unwrap_or(0.0);
        let w: f64 = cols[3].parse().unwrap_or(0.0);
        let h: f64 = cols[4].parse().unwrap_or(0.0);
        let confidence: f64 = cols.get(5).and_then(|s| s.parse().ok()).unwrap_or(1.0);
        matches.push(OcrMatch {
            text: text.to_string(),
            x: x + offset_x,
            y: y + offset_y,
            width: w,
            height: h,
            center_x: x + w / 2.0 + offset_x,
            center_y: y + h / 2.0 + offset_y,
            confidence,
        });
    }

    Ok(matches)
}

// ─── macOS tool_ocr_screen (Vision first, tesseract fallback) ────────────────

#[cfg(target_os = "macos")]
pub fn tool_ocr_screen(args: &Value) -> NativeToolResult {
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;
    let language = args.get("language").and_then(|v| v.as_str());
    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return super::tool_error("ocr_screen", format!("listing monitors: {e}")),
    };
    let monitor = match monitors.get(monitor_idx) {
        Some(m) => m,
        None => return super::tool_error("ocr_screen", format!("monitor {monitor_idx} not found")),
    };
    let img = match monitor.capture_image() {
        Ok(i) => i,
        Err(e) => return super::tool_error("ocr_screen", format!("screenshot failed: {e}")),
    };
    // Try Vision framework first, fall back to tesseract
    match ocr_image_vision(&img, language) {
        Ok(text) => NativeToolResult::text_only(format!("OCR text:\n{text}")),
        Err(_vision_err) => {
            match ocr_image_tesseract(&img, language) {
                Ok(text) => NativeToolResult::text_only(format!("OCR text:\n{text}")),
                Err(e) => super::tool_error("ocr_screen", e),
            }
        }
    }
}

// ─── Linux tool_ocr_screen (tesseract only) ─────────────────────────────────

#[cfg(target_os = "linux")]
pub fn tool_ocr_screen(args: &Value) -> NativeToolResult {
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;
    let language = args.get("language").and_then(|v| v.as_str());
    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return super::tool_error("ocr_screen", format!("listing monitors: {e}")),
    };
    let monitor = match monitors.get(monitor_idx) {
        Some(m) => m,
        None => return super::tool_error("ocr_screen", format!("monitor {monitor_idx} not found")),
    };
    let img = match monitor.capture_image() {
        Ok(i) => i,
        Err(e) => return super::tool_error("ocr_screen", format!("screenshot failed: {e}")),
    };
    match ocr_image_tesseract(&img, language) {
        Ok(text) => NativeToolResult::text_only(format!("OCR text:\n{text}")),
        Err(e) => super::tool_error("ocr_screen", e),
    }
}

/// OCR the screen (or region) and search for specific text, returning its bounding box coordinates.
#[cfg(windows)]
pub fn tool_ocr_find_text(args: &Value) -> NativeToolResult {
    let search_text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return super::tool_error("ocr_find_text", "'text' argument is required"),
    };
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    // Capture screen
    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return super::tool_error("ocr_find_text", format!("listing monitors: {e}")),
    };
    let monitor = match monitors.get(monitor_idx) {
        Some(m) => m,
        None => return super::tool_error("ocr_find_text", format!("monitor index {monitor_idx} out of range (have {})", monitors.len())),
    };
    let capture = match monitor.capture_image() {
        Ok(img) => img,
        Err(e) => return super::tool_error("ocr_find_text", format!("capturing screen: {e}")),
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
    let result = super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || {
        ocr_find_text_winrt(&work_img, &search, offset_x, offset_y)
    }).and_then(|r| r);

    match result {
        Ok(matches) => {
            if matches.is_empty() {
                NativeToolResult::text_only(format!("Text '{}' not found on screen", search_text))
            } else {
                let lines: Vec<String> = matches.iter().map(|m| {
                    format!("\"{}\" at ({:.0},{:.0}) {:.0}x{:.0} conf={:.2}",
                        m.text, m.x, m.y, m.width, m.height, m.confidence)
                }).collect();
                NativeToolResult::text_only(format!("Found {} match(es):\n{}", matches.len(), lines.join("\n")))
            }
        }
        Err(e) => super::tool_error("ocr_find_text", format!("OCR: {e}")),
    }
}

/// OCR find text returning structured matches. Must be called from STA thread.
#[cfg(windows)]
pub(super) fn ocr_find_text_winrt(img: &image::RgbaImage, search: &str, offset_x: f64, offset_y: f64) -> Result<Vec<OcrMatch>, String> {
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

// ─── macOS tool_ocr_find_text (Vision first, tesseract fallback) ─────────────

#[cfg(target_os = "macos")]
pub fn tool_ocr_find_text(args: &Value) -> NativeToolResult {
    let search = match args.get("text").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return super::tool_error("ocr_find_text", "'text' is required"),
    };
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;
    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return super::tool_error("ocr_find_text", format!("listing monitors: {e}")),
    };
    let monitor = match monitors.get(monitor_idx) {
        Some(m) => m,
        None => return super::tool_error("ocr_find_text", format!("monitor {monitor_idx} not found")),
    };
    let img = match monitor.capture_image() {
        Ok(i) => i,
        Err(e) => return super::tool_error("ocr_find_text", format!("screenshot failed: {e}")),
    };
    let offset_x = monitor.x() as f64;
    let offset_y = monitor.y() as f64;
    // Try Vision framework first, fall back to tesseract
    let result = match ocr_find_text_vision(&img, search, offset_x, offset_y, None) {
        Ok(matches) => Ok(matches),
        Err(_) => ocr_find_text_tesseract(&img, search, offset_x, offset_y),
    };
    match result {
        Ok(matches) if matches.is_empty() => {
            NativeToolResult::text_only(format!("Text '{search}' not found on screen"))
        }
        Ok(matches) => {
            let mut lines = vec![format!("Found {} match(es) for '{search}':", matches.len())];
            for m in &matches {
                lines.push(format!(
                    "  \"{}\" at ({:.0},{:.0}) {:.0}x{:.0} conf={:.2}",
                    m.text, m.x, m.y, m.width, m.height, m.confidence
                ));
            }
            NativeToolResult::text_only(lines.join("\n"))
        }
        Err(e) => super::tool_error("ocr_find_text", e),
    }
}

// ─── Linux tool_ocr_find_text (tesseract only) ──────────────────────────────

#[cfg(target_os = "linux")]
pub fn tool_ocr_find_text(args: &Value) -> NativeToolResult {
    let search = match args.get("text").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return super::tool_error("ocr_find_text", "'text' is required"),
    };
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;
    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return super::tool_error("ocr_find_text", format!("listing monitors: {e}")),
    };
    let monitor = match monitors.get(monitor_idx) {
        Some(m) => m,
        None => return super::tool_error("ocr_find_text", format!("monitor {monitor_idx} not found")),
    };
    let img = match monitor.capture_image() {
        Ok(i) => i,
        Err(e) => return super::tool_error("ocr_find_text", format!("screenshot failed: {e}")),
    };
    let offset_x = monitor.x() as f64;
    let offset_y = monitor.y() as f64;
    match ocr_find_text_tesseract(&img, search, offset_x, offset_y) {
        Ok(matches) if matches.is_empty() => {
            NativeToolResult::text_only(format!("Text '{search}' not found on screen"))
        }
        Ok(matches) => {
            let mut lines = vec![format!("Found {} match(es) for '{search}':", matches.len())];
            for m in &matches {
                lines.push(format!(
                    "  \"{}\" at ({:.0},{:.0}) {:.0}x{:.0} conf={:.2}",
                    m.text, m.x, m.y, m.width, m.height, m.confidence
                ));
            }
            NativeToolResult::text_only(lines.join("\n"))
        }
        Err(e) => super::tool_error("ocr_find_text", e),
    }
}
