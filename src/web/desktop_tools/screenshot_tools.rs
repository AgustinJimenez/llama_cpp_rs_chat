//! Screenshot capture, comparison, and polling tools.

use serde_json::Value;

use super::NativeToolResult;
use super::parse_int;

// ─── Retry and adaptive polling helpers ───────────────────────────────────────

/// Retry a fallible operation up to `max_retries` times with a delay between attempts.
#[allow(dead_code)]
pub(super) fn retry_on_failure<F, T>(max_retries: u32, delay_ms: u64, mut f: F) -> Result<T, String>
where
    F: FnMut() -> Result<T, String>,
{
    let mut last_err = String::new();
    for i in 0..=max_retries {
        match f() {
            Ok(val) => return Ok(val),
            Err(e) => {
                last_err = e;
                if i < max_retries {
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                }
            }
        }
    }
    Err(last_err)
}

/// Compute an adaptive poll interval with exponential backoff, capped at max_ms.
pub(crate) fn adaptive_poll_ms(attempt: u32, initial_ms: u64, max_ms: u64) -> u64 {
    (initial_ms * (1u64 << attempt.min(6))).min(max_ms)
}

// ─── Screenshot tools ─────────────────────────────────────────────────────────

/// Capture a screenshot of a specific screen region.
pub fn tool_screenshot_region(args: &Value) -> NativeToolResult {
    let x = match args.get("x").and_then(parse_int) {
        Some(v) => v as u32,
        None => return super::tool_error("screenshot_region", "'x' is required"),
    };
    let y = match args.get("y").and_then(parse_int) {
        Some(v) => v as u32,
        None => return super::tool_error("screenshot_region", "'y' is required"),
    };
    let w = match args.get("width").and_then(parse_int) {
        Some(v) => v as u32,
        None => return super::tool_error("screenshot_region", "'width' is required"),
    };
    let h = match args.get("height").and_then(parse_int) {
        Some(v) => v as u32,
        None => return super::tool_error("screenshot_region", "'height' is required"),
    };
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    let monitors = match super::validated_monitors("screenshot_region", monitor_idx) {
        Ok(m) => m,
        Err(e) => return e,
    };

    let img = match monitors[monitor_idx].capture_image() {
        Ok(i) => i,
        Err(e) => return super::tool_error("screenshot_region", format!("capturing screen: {e}")),
    };

    // Crop the image manually using the image crate
    let img_w = img.width();
    let img_h = img.height();
    if x + w > img_w || y + h > img_h {
        return super::tool_error("screenshot_region", format!("region ({x},{y} {w}x{h}) exceeds screen size ({img_w}x{img_h})"));
    }
    let cropped: image::RgbaImage = image::imageops::crop_imm(&img, x, y, w, h).to_image();

    // Encode to PNG
    let mut png_buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut png_buf);
    if let Err(e) = cropped.write_to(&mut cursor, image::ImageFormat::Png) {
        return super::tool_error("screenshot_region", format!("encoding PNG: {e}"));
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
    let highlight = args.get("highlight").map(|v| super::parse_bool(v, false)).unwrap_or(false);
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    let monitors = match super::validated_monitors("screenshot_diff", monitor_idx) {
        Ok(m) => m,
        Err(e) => return e,
    };

    let img = match monitors[monitor_idx].capture_image() {
        Ok(i) => i,
        Err(e) => return super::tool_error("screenshot_diff", format!("capturing screen: {e}")),
    };
    let current_bytes = img.as_raw().clone();

    let w = img.width();
    let h = img.height();

    if save_baseline {
        let mut lock = BASELINE.lock().unwrap_or_else(|poisoned| {
            crate::log_warn!("system", "Mutex poisoned in BASELINE, recovering");
            poisoned.into_inner()
        });
        *lock = Some(current_bytes);
        return NativeToolResult::text_only(format!(
            "Baseline saved: {w}x{h} ({} bytes). Call again without save_baseline to compare.",
            lock.as_ref().map(|b| b.len()).unwrap_or(0)
        ));
    }

    // Compare with baseline
    let lock = BASELINE.lock().unwrap_or_else(|poisoned| {
        crate::log_warn!("system", "Mutex poisoned in BASELINE, recovering");
        poisoned.into_inner()
    });
    let baseline = match lock.as_ref() {
        Some(b) => b,
        None => return super::tool_error("screenshot_diff", "no baseline saved. Call with save_baseline=true first."),
    };

    if current_bytes.len() != baseline.len() {
        return super::tool_error("screenshot_diff", format!("screen resolution changed since baseline (baseline {} bytes, current {} bytes)", baseline.len(), current_bytes.len()));
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
    let summary = format!(
        "Screen diff: {:.2}% pixels changed ({changed_pixels}/{total_pixels}). Changed region: ({min_x},{min_y}) to ({max_x},{max_y}) = {}x{}",
        pct, max_x - min_x + 1, max_y - min_y + 1
    );

    // When highlight is requested, draw a red bounding box around the changed region
    if highlight {
        if let Some(mut diff_img) = image::RgbaImage::from_raw(w, h, current_bytes) {
            let red = image::Rgba([255u8, 0, 0, 200]);
            for border in 0..2u32 {
                // Top and bottom edges
                for px in min_x.saturating_sub(border)..=(max_x + border).min(w - 1) {
                    if min_y >= border {
                        diff_img.put_pixel(px, min_y - border, red);
                    }
                    if max_y + border < h {
                        diff_img.put_pixel(px, max_y + border, red);
                    }
                }
                // Left and right edges
                for py in min_y.saturating_sub(border)..=(max_y + border).min(h - 1) {
                    if min_x >= border {
                        diff_img.put_pixel(min_x - border, py, red);
                    }
                    if max_x + border < w {
                        diff_img.put_pixel(max_x + border, py, red);
                    }
                }
            }
            // Encode the annotated image as PNG
            let mut png_buf = Vec::new();
            let mut cursor = std::io::Cursor::new(&mut png_buf);
            if diff_img.write_to(&mut cursor, image::ImageFormat::Png).is_ok() {
                return NativeToolResult::with_image(summary, png_buf);
            }
            // Fall through to text-only if PNG encoding fails
        }
    }

    NativeToolResult::text_only(summary)
}

/// Capture a screenshot of a specific window by title.
pub fn tool_window_screenshot(args: &Value) -> NativeToolResult {
    let title = match args.get("title").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return super::tool_error("window_screenshot", "'title' argument is required"),
    };

    let windows = match xcap::Window::all() {
        Ok(w) => w,
        Err(e) => return super::tool_error("window_screenshot", format!("listing windows: {e}")),
    };

    let title_lower = title.to_lowercase();
    let target = windows.into_iter().find(|w| {
        w.title().unwrap_or_default().to_lowercase().contains(&title_lower)
            || w.app_name().unwrap_or_default().to_lowercase().contains(&title_lower)
    });

    let window = match target {
        Some(w) => w,
        None => return super::tool_error("window_screenshot", format!("no window matches '{title}'")),
    };

    let capture = match window.capture_image() {
        Ok(img) => img,
        Err(e) => return super::tool_error("window_screenshot", format!("capturing window: {e}")),
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
        return super::tool_error("window_screenshot", format!("encoding PNG: {e}"));
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
        Err(e) => return super::tool_error("wait_for_screen_change", format!("capturing baseline: {e}")),
    };

    let start = std::time::Instant::now();
    let mut attempt = 0u32;

    loop {
        if let Err(e) = super::ensure_desktop_not_cancelled() {
            return super::tool_error("wait_for_screen_change", e);
        }
        if start.elapsed().as_millis() >= timeout_ms as u128 {
            return NativeToolResult::text_only(format!(
                "Timeout: no change detected in region ({x},{y} {w}x{h}) after {timeout_ms}ms"
            ));
        }

        let poll_ms = adaptive_poll_ms(attempt, 100, 1000);
        if let Err(e) = super::interruptible_sleep(std::time::Duration::from_millis(poll_ms)) {
            return super::tool_error("wait_for_screen_change", e);
        }
        attempt += 1;

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
pub(super) fn capture_region(monitor_idx: usize, x: u32, y: u32, w: u32, h: u32) -> Result<image::RgbaImage, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adaptive_poll_ms_exponential() {
        assert_eq!(adaptive_poll_ms(0, 100, 1000), 100);
        assert_eq!(adaptive_poll_ms(1, 100, 1000), 200);
        assert_eq!(adaptive_poll_ms(2, 100, 1000), 400);
        assert_eq!(adaptive_poll_ms(3, 100, 1000), 800);
        assert_eq!(adaptive_poll_ms(4, 100, 1000), 1000); // capped
        assert_eq!(adaptive_poll_ms(10, 100, 1000), 1000); // capped
    }

    #[test]
    fn test_retry_on_failure_succeeds_first_try() {
        let mut count = 0;
        let result = retry_on_failure(3, 10, || {
            count += 1;
            Ok::<_, String>(42)
        });
        assert_eq!(result.unwrap(), 42);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_retry_on_failure_succeeds_after_retries() {
        let mut count = 0;
        let result = retry_on_failure(3, 10, || {
            count += 1;
            if count < 3 {
                Err("not yet".to_string())
            } else {
                Ok(99)
            }
        });
        assert_eq!(result.unwrap(), 99);
        assert_eq!(count, 3);
    }

    #[test]
    fn test_retry_on_failure_exhausts_retries() {
        let mut count = 0;
        let result: Result<i32, String> = retry_on_failure(2, 10, || {
            count += 1;
            Err("always fails".to_string())
        });
        assert!(result.is_err());
        assert_eq!(count, 3); // initial + 2 retries
    }
}
