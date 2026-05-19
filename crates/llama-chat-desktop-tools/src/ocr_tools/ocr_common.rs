//! Shared OCR types, cache, screen capture helpers, and platform dispatchers.

use serde_json::Value;
use std::sync::Mutex;

use crate::NativeToolResult;
use crate::parse_int;

// ─── Shared types ─────────────────────────────────────────────────────────────

/// Structured OCR match result with bounding box info.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
#[derive(Clone)]
pub(crate) struct OcrMatch {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub center_x: f64,
    pub center_y: f64,
    pub confidence: f64,
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub(crate) struct OcrCaptureTarget {
    pub image: image::RgbaImage,
    pub raw: Vec<u8>,
    pub region_desc: String,
    pub offset_x: f64,
    pub offset_y: f64,
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
#[derive(Clone)]
pub(crate) enum OcrCachePayload {
    Text(String),
    Matches(Vec<OcrMatch>),
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
#[derive(Clone)]
struct OcrCacheEntry {
    key: String,
    raw: Vec<u8>,
    created_at: std::time::Instant,
    payload: OcrCachePayload,
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
lazy_static::lazy_static! {
    static ref OCR_CACHE: Mutex<Option<OcrCacheEntry>> = Mutex::new(None);
}

// ─── Cache helpers ────────────────────────────────────────────────────────────

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub(crate) fn ocr_cache_settings(args: &Value) -> (u64, f64) {
    let max_age_ms = args
        .get("cache_max_age_ms")
        .and_then(parse_int)
        .unwrap_or(1500)
        .clamp(0, 30_000) as u64;
    let threshold_pct = args
        .get("cache_threshold_pct")
        .and_then(crate::parse_float)
        .unwrap_or(0.5)
        .clamp(0.0, 100.0);
    (max_age_ms, threshold_pct)
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub(crate) fn get_cached_ocr_payload(
    key: &str,
    raw: &[u8],
    max_age_ms: u64,
    threshold_pct: f64,
) -> Option<OcrCachePayload> {
    if max_age_ms == 0 {
        return None;
    }
    let lock = OCR_CACHE.lock().unwrap_or_else(|poisoned| {
        eprintln!("[ocr_tools] OCR_CACHE mutex poisoned — recovering from panic");
        log_warn!("system", "Mutex poisoned in OCR_CACHE, recovering");
        poisoned.into_inner()
    });
    let entry = lock.as_ref()?;
    if entry.key != key || entry.created_at.elapsed().as_millis() > max_age_ms as u128 {
        return None;
    }
    if crate::pixel_diff_pct(&entry.raw, raw) > threshold_pct {
        return None;
    }
    Some(entry.payload.clone())
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub(crate) fn update_cached_ocr_payload(key: String, raw: Vec<u8>, payload: OcrCachePayload) {
    let mut lock = OCR_CACHE.lock().unwrap_or_else(|poisoned| {
        eprintln!("[ocr_tools] OCR_CACHE mutex poisoned — recovering from panic");
        log_warn!("system", "Mutex poisoned in OCR_CACHE, recovering");
        poisoned.into_inner()
    });
    *lock = Some(OcrCacheEntry {
        key,
        raw,
        created_at: std::time::Instant::now(),
        payload,
    });
}

// ─── Screen capture ───────────────────────────────────────────────────────────

#[cfg(windows)]
use crate::win32;
#[cfg(target_os = "macos")]
use crate::macos as win32;
#[cfg(target_os = "linux")]
use crate::linux as win32;

/// Upscale an image 2x for better OCR accuracy on small text.
pub(crate) fn upscale_for_ocr(img: &image::RgbaImage) -> image::RgbaImage {
    let (w, h) = img.dimensions();
    if w >= 3000 || h >= 2000 {
        return img.clone();
    }
    image::imageops::resize(img, w * 2, h * 2, image::imageops::FilterType::Lanczos3)
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub(crate) fn clamp_region_to_monitor(
    monitor_x: i32,
    monitor_y: i32,
    monitor_width: u32,
    monitor_height: u32,
    abs_x: i32,
    abs_y: i32,
    width: u32,
    height: u32,
) -> Result<(u32, u32, u32, u32, f64, f64), String> {
    let rel_x = (abs_x - monitor_x).max(0) as u32;
    let rel_y = (abs_y - monitor_y).max(0) as u32;
    if rel_x >= monitor_width || rel_y >= monitor_height {
        return Err("target region is outside the selected monitor".to_string());
    }
    let clamped_width = width.min(monitor_width.saturating_sub(rel_x)).max(1);
    let clamped_height = height.min(monitor_height.saturating_sub(rel_y)).max(1);
    Ok((
        rel_x,
        rel_y,
        clamped_width,
        clamped_height,
        monitor_x as f64 + rel_x as f64,
        monitor_y as f64 + rel_y as f64,
    ))
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub(crate) fn capture_ocr_target(args: &Value, tool_name: &str) -> Result<OcrCaptureTarget, NativeToolResult> {
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;
    let monitors = crate::validated_monitors(tool_name, monitor_idx)?;
    let monitor = &monitors[monitor_idx];

    let capture = match monitor.capture_image() {
        Ok(img) => img,
        Err(e) => return Err(crate::tool_error(tool_name, format!("capturing screen: {e}"))),
    };

    let monitor_x = monitor.x().unwrap_or(0);
    let monitor_y = monitor.y().unwrap_or(0);
    let full_w = capture.width();
    let full_h = capture.height();

    let pid_filter = args.get("pid").and_then(parse_int).map(|v| v as u32);
    let window_filter = args.get("window").and_then(|v| v.as_str());
    let title_filter = args.get("title").and_then(|v| v.as_str());

    if let Some(target_pid) = pid_filter {
        let (hwnd, info) = match win32::find_window_by_pid(target_pid) {
            Some(result) => result,
            None => {
                return Err(crate::tool_error(
                    tool_name,
                    format!("no window found for pid {target_pid}"),
                ))
            }
        };
        let rect = win32::get_window_rect(hwnd);
        let width = (rect.right - rect.left).max(1) as u32;
        let height = (rect.bottom - rect.top).max(1) as u32;
        let (rx, ry, rw, rh, offset_x, offset_y) = clamp_region_to_monitor(
            monitor_x,
            monitor_y,
            full_w,
            full_h,
            rect.left,
            rect.top,
            width,
            height,
        )
        .map_err(|e| crate::tool_error(tool_name, format!("pid {target_pid}: {e}")))?;
        let cropped = image::imageops::crop_imm(&capture, rx, ry, rw, rh).to_image();
        return Ok(OcrCaptureTarget {
            raw: cropped.as_raw().to_vec(),
            image: cropped,
            region_desc: format!(" (pid={target_pid} window \"{}\" {offset_x:.0},{offset_y:.0} {rw}x{rh})", info.title),
            offset_x,
            offset_y,
        });
    }

    if let Some(filter) = window_filter.or(title_filter) {
        let (hwnd, info) = match win32::find_window_by_filter(filter) {
            Some(result) => result,
            None => {
                return Err(crate::tool_error(
                    tool_name,
                    format!("no window matches '{filter}'"),
                ))
            }
        };
        let rect = win32::get_window_rect(hwnd);
        let width = (rect.right - rect.left).max(1) as u32;
        let height = (rect.bottom - rect.top).max(1) as u32;
        let (rx, ry, rw, rh, offset_x, offset_y) = clamp_region_to_monitor(
            monitor_x,
            monitor_y,
            full_w,
            full_h,
            rect.left,
            rect.top,
            width,
            height,
        )
        .map_err(|e| crate::tool_error(tool_name, format!("window '{filter}': {e}")))?;
        let cropped = image::imageops::crop_imm(&capture, rx, ry, rw, rh).to_image();
        return Ok(OcrCaptureTarget {
            raw: cropped.as_raw().to_vec(),
            image: cropped,
            region_desc: format!(" (window \"{}\" {offset_x:.0},{offset_y:.0} {rw}x{rh})", info.title),
            offset_x,
            offset_y,
        });
    }

    let region_x = args.get("x").and_then(parse_int).map(|v| v as u32);
    let region_y = args.get("y").and_then(parse_int).map(|v| v as u32);
    let region_w = args.get("width").and_then(parse_int).map(|v| v as u32);
    let region_h = args.get("height").and_then(parse_int).map(|v| v as u32);
    if let (Some(rx), Some(ry), Some(rw), Some(rh)) = (region_x, region_y, region_w, region_h) {
        if rx + rw > full_w || ry + rh > full_h {
            return Err(crate::tool_error(
                tool_name,
                format!("region ({rx},{ry} {rw}x{rh}) exceeds monitor {monitor_idx} ({full_w}x{full_h})"),
            ));
        }
        let cropped = image::imageops::crop_imm(&capture, rx, ry, rw, rh).to_image();
        return Ok(OcrCaptureTarget {
            raw: cropped.as_raw().to_vec(),
            image: cropped,
            region_desc: format!(
                " (region {:.0},{:.0} {rw}x{rh})",
                monitor_x as f64 + rx as f64,
                monitor_y as f64 + ry as f64
            ),
            offset_x: monitor_x as f64 + rx as f64,
            offset_y: monitor_y as f64 + ry as f64,
        });
    }

    Ok(OcrCaptureTarget {
        raw: capture.as_raw().to_vec(),
        image: capture,
        region_desc: format!(
            " (monitor {monitor_idx} {:.0},{:.0} {}x{})",
            monitor_x as f64,
            monitor_y as f64,
            full_w,
            full_h
        ),
        offset_x: monitor_x as f64,
        offset_y: monitor_y as f64,
    })
}

// ─── Unified platform dispatchers ─────────────────────────────────────────────

/// Cross-platform OCR text extraction dispatcher.
/// Windows: WinRT, macOS: Vision (tesseract fallback), Linux: tesseract.
#[cfg(windows)]
pub(crate) fn ocr_image(img: &image::RgbaImage) -> Result<String, String> {
    super::ocr_winrt::ocr_image_winrt(img)
}

#[cfg(target_os = "macos")]
pub(crate) fn ocr_image(img: &image::RgbaImage) -> Result<String, String> {
    match super::ocr_macos::ocr_image_vision(img, None) {
        Ok(text) => Ok(text),
        Err(_) => super::ocr_tesseract::ocr_image_tesseract(img, None),
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn ocr_image(img: &image::RgbaImage) -> Result<String, String> {
    super::ocr_tesseract::ocr_image_tesseract(img, None)
}

/// Cross-platform OCR find text dispatcher.
/// Windows: WinRT, macOS: Vision (tesseract fallback), Linux: tesseract.
#[cfg(windows)]
pub(crate) fn ocr_find_text(img: &image::RgbaImage, search: &str, offset_x: f64, offset_y: f64) -> Result<Vec<OcrMatch>, String> {
    super::ocr_winrt::ocr_find_text_winrt(img, search, offset_x, offset_y)
}

#[cfg(target_os = "macos")]
pub(crate) fn ocr_find_text(img: &image::RgbaImage, search: &str, offset_x: f64, offset_y: f64) -> Result<Vec<OcrMatch>, String> {
    match super::ocr_macos::ocr_find_text_vision(img, search, offset_x, offset_y, None) {
        Ok(matches) => Ok(matches),
        Err(_) => super::ocr_tesseract::ocr_find_text_tesseract(img, search, offset_x, offset_y),
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn ocr_find_text(img: &image::RgbaImage, search: &str, offset_x: f64, offset_y: f64) -> Result<Vec<OcrMatch>, String> {
    super::ocr_tesseract::ocr_find_text_tesseract(img, search, offset_x, offset_y)
}

/// OCR an RgbaImage and check if the recognized text contains `search_lower` (case-insensitive).
/// Used by the screen verification system to confirm expected text after actions.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub(crate) fn ocr_png_and_search(img: &image::RgbaImage, search_lower: &str) -> bool {
    match ocr_image(img) {
        Ok(text) => text.to_lowercase().contains(search_lower),
        Err(_) => false,
    }
}
