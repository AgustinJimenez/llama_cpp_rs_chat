//! Template image matching tool — find a small image on the screen.

use serde_json::Value;

use super::NativeToolResult;
use super::parse_int;

/// Find a template image on the screen using pixel comparison (SSD).
/// The template can be loaded from a file path.
pub fn tool_find_image_on_screen(args: &Value) -> NativeToolResult {
    let template_path = match args.get("template").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return super::tool_error("find_image_on_screen", "'template' (image file path) is required")
        }
    };
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;
    let confidence_threshold = args
        .get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.9);
    let step = args.get("step").and_then(parse_int).unwrap_or(2) as u32;

    // Load template image
    let template = match image::open(template_path) {
        Ok(img) => img.to_rgba8(),
        Err(e) => {
            return super::tool_error("find_image_on_screen", format!("loading template '{template_path}': {e}"))
        }
    };

    // Capture screen
    let monitors = match super::validated_monitors("find_image_on_screen", monitor_idx) {
        Ok(m) => m,
        Err(e) => return e,
    };
    let screen = match monitors[monitor_idx].capture_image() {
        Ok(i) => i,
        Err(e) => return super::tool_error("find_image_on_screen", format!("capturing screen: {e}")),
    };

    // Find best match
    match find_best_match(&screen, &template, step, confidence_threshold) {
        Some((x, y, confidence)) => {
            let tw = template.width();
            let th = template.height();
            let cx = x + tw / 2;
            let cy = y + th / 2;
            NativeToolResult::text_only(format!(
                "Found template at ({x}, {y}), size {tw}x{th}, center ({cx}, {cy}), confidence {confidence:.1}%"
            ))
        }
        None => NativeToolResult::text_only(format!(
            "Template not found on screen (threshold: {:.0}%)",
            confidence_threshold * 100.0
        )),
    }
}

/// Find the best matching position for a template within a screen image.
/// Uses Sum of Squared Differences (SSD) with early termination.
/// Returns (x, y, confidence_percent) or None.
fn find_best_match(
    screen: &image::RgbaImage,
    template: &image::RgbaImage,
    step: u32,
    confidence_threshold: f64,
) -> Option<(u32, u32, f64)> {
    let sw = screen.width();
    let sh = screen.height();
    let tw = template.width();
    let th = template.height();

    if tw > sw || th > sh || tw == 0 || th == 0 {
        return None;
    }

    let screen_raw = screen.as_raw();
    let template_raw = template.as_raw();
    let max_diff_per_pixel: f64 = 255.0 * 255.0 * 3.0; // RGB channels
    let total_pixels = (tw * th) as f64;
    let max_total_diff = max_diff_per_pixel * total_pixels;
    // Early termination threshold: if accumulated SSD exceeds this, skip
    let threshold_diff = max_total_diff * (1.0 - confidence_threshold);

    let mut best_x = 0u32;
    let mut best_y = 0u32;
    let mut best_ssd = f64::MAX;

    let step = step.max(1);

    for sy in (0..=(sh - th)).step_by(step as usize) {
        for sx in (0..=(sw - tw)).step_by(step as usize) {
            let mut ssd: f64 = 0.0;
            let mut early_exit = false;

            'pixel: for ty in 0..th {
                let screen_row_start = ((sy + ty) * sw + sx) as usize * 4;
                let template_row_start = (ty * tw) as usize * 4;

                for tx in 0..tw {
                    let si = screen_row_start + tx as usize * 4;
                    let ti = template_row_start + tx as usize * 4;

                    let dr = screen_raw[si] as f64 - template_raw[ti] as f64;
                    let dg = screen_raw[si + 1] as f64 - template_raw[ti + 1] as f64;
                    let db = screen_raw[si + 2] as f64 - template_raw[ti + 2] as f64;
                    ssd += dr * dr + dg * dg + db * db;

                    if ssd > best_ssd || ssd > threshold_diff {
                        early_exit = true;
                        break 'pixel;
                    }
                }
            }

            if !early_exit && ssd < best_ssd {
                best_ssd = ssd;
                best_x = sx;
                best_y = sy;
            }
        }
    }

    let confidence = 1.0 - (best_ssd / max_total_diff);
    if confidence >= confidence_threshold {
        Some((best_x, best_y, confidence * 100.0))
    } else {
        None
    }
}
