//! Desktop tool tracing, screen verification, and post-action screenshot capture.
//!
//! Contains all the logic for verifying screen changes after actions,
//! writing trace lines for debugging, and capturing post-action screenshots.

use serde_json::Value;

use super::helpers::{
    encode_image_to_png, get_cached_screenshot, optimize_screenshot_for_vision, parse_bool,
    parse_float, parse_int, pixel_diff_pct, tool_error, update_screenshot_cache,
};
use super::{interruptible_sleep, ocr_tools, spawn_with_timeout, NativeToolResult};

pub fn desktop_trace_path() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("LLAMA_CHAT_DESKTOP_TRACE_PATH") {
        if !path.trim().is_empty() {
            return std::path::PathBuf::from(path);
        }
    }
    std::env::temp_dir().join("llama_cpp_chat_desktop_trace.jsonl")
}

pub fn classify_desktop_result_status(text: &str) -> &'static str {
    let lower = text.to_lowercase();
    if lower.starts_with("error") {
        if lower.contains("timed out") || lower.contains("timeout") {
            "timed_out"
        } else if lower.contains("cancelled") || lower.contains("canceled") {
            "cancelled"
        } else {
            "error"
        }
    } else if lower.starts_with("timeout:") {
        "timed_out"
    } else if lower.contains("verification failed:") {
        "verification_failed"
    } else {
        "completed"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerificationRegion {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) width: u32,
    pub(super) height: u32,
}

pub struct ScreenVerificationContext {
    pub(super) baseline_raw: Vec<u8>,
    pub(super) threshold_pct: f64,
    pub(super) timeout_ms: u64,
    pub(super) poll_ms: u64,
    pub(super) region: Option<VerificationRegion>,
    pub(super) expected_text: Option<String>,
}

pub fn normalize_verification_region(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Option<VerificationRegion> {
    if width == 0 || height == 0 {
        return None;
    }
    Some(VerificationRegion {
        x,
        y,
        width,
        height,
    })
}

pub fn action_verification_region_from_args(args: &Value) -> Option<VerificationRegion> {
    if let (Some(x), Some(y), Some(width), Some(height)) = (
        args.get("verify_x").and_then(parse_int),
        args.get("verify_y").and_then(parse_int),
        args.get("verify_width").and_then(parse_int),
        args.get("verify_height").and_then(parse_int),
    ) {
        return normalize_verification_region(x as i32, y as i32, width as u32, height as u32);
    }
    None
}

pub fn capture_screen_state(
    region: Option<VerificationRegion>,
) -> Result<(Vec<u8>, Vec<u8>, u32, u32, String), String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("Failed to enumerate monitors: {e}"))?;
    let monitor = if let Some(region) = region {
        let center_x = region.x + region.width as i32 / 2;
        let center_y = region.y + region.height as i32 / 2;
        monitors
            .iter()
            .find(|m| {
                let mx = m.x().unwrap_or(0);
                let my = m.y().unwrap_or(0);
                let mw = m.width().unwrap_or(0) as i32;
                let mh = m.height().unwrap_or(0) as i32;
                center_x >= mx && center_x < mx + mw && center_y >= my && center_y < my + mh
            })
            .or_else(|| monitors.first())
    } else {
        monitors
            .iter()
            .find(|m| m.is_primary().unwrap_or(false))
            .or(monitors.first())
    }
    .ok_or_else(|| "No monitors detected".to_string())?;
    let img = monitor
        .capture_image()
        .map_err(|e| format!("Screenshot capture failed: {e}"))?;

    if let Some(region) = region {
        let mx = monitor.x().unwrap_or(0);
        let my = monitor.y().unwrap_or(0);
        let rel_x = (region.x - mx).max(0) as u32;
        let rel_y = (region.y - my).max(0) as u32;
        if rel_x >= img.width() || rel_y >= img.height() {
            return Err("Verification region is outside the selected monitor".to_string());
        }
        let width = region.width.min(img.width().saturating_sub(rel_x)).max(1);
        let height = region.height.min(img.height().saturating_sub(rel_y)).max(1);
        let cropped = image::imageops::crop_imm(&img, rel_x, rel_y, width, height).to_image();
        let raw = cropped.as_raw().to_vec();
        let png = encode_image_to_png(&cropped)?;
        return Ok((
            raw,
            png,
            cropped.width(),
            cropped.height(),
            format!(
                " region ({},{}) {}x{}",
                mx + rel_x as i32,
                my + rel_y as i32,
                cropped.width(),
                cropped.height()
            ),
        ));
    }

    let width = img.width();
    let height = img.height();
    let raw = img.as_raw().to_vec();
    let png = encode_image_to_png(&img)?;
    Ok((raw, png, width, height, String::new()))
}

pub fn prepare_screen_verification(
    args: &Value,
    region: Option<VerificationRegion>,
) -> Result<Option<ScreenVerificationContext>, NativeToolResult> {
    let expected_text = args.get("verify_text").and_then(|v| v.as_str()).map(|s| s.to_string());
    let verify = args
        .get("verify_screen_change")
        .map(|v| parse_bool(v, false))
        .unwrap_or(false)
        || expected_text.is_some(); // verify_text implicitly enables verification
    if !verify {
        return Ok(None);
    }

    let threshold_pct = args
        .get("verify_threshold_pct")
        .and_then(parse_float)
        .unwrap_or(0.5)
        .clamp(0.01, 100.0);
    let timeout_ms = args
        .get("verify_timeout_ms")
        .and_then(parse_int)
        .unwrap_or(1200)
        .clamp(100, 10_000) as u64;
    let poll_ms = args
        .get("verify_poll_ms")
        .and_then(parse_int)
        .unwrap_or(150)
        .clamp(50, 1000) as u64;
    let verification_region = action_verification_region_from_args(args).or(region);

    let (baseline_raw, baseline_png, _, _, _) = match capture_screen_state(verification_region) {
        Ok(state) => state,
        Err(e) => return Err(tool_error("desktop_verification", e)),
    };
    update_screenshot_cache(baseline_raw.clone(), baseline_png);

    Ok(Some(ScreenVerificationContext {
        baseline_raw,
        threshold_pct,
        timeout_ms,
        poll_ms,
        region: verification_region,
        expected_text,
    }))
}

pub fn finalize_action_result(
    summary: String,
    delay_ms: u64,
    do_screenshot: bool,
    verification: Option<ScreenVerificationContext>,
) -> NativeToolResult {
    if let Some(verification) = verification {
        if let Err(e) = interruptible_sleep(std::time::Duration::from_millis(delay_ms)) {
            return tool_error("verification", e);
        }

        let start = std::time::Instant::now();
        let (raw, png, width, height, diff_pct, elapsed_ms, region_desc) = loop {
            match capture_screen_state(verification.region) {
                Ok((raw, png, width, height, region_desc)) => {
                    let diff_pct = pixel_diff_pct(&verification.baseline_raw, &raw);
                    let elapsed_ms = start.elapsed().as_millis();
                    let passed = diff_pct >= verification.threshold_pct;
                    if passed || elapsed_ms as u64 >= verification.timeout_ms {
                        break (raw, png, width, height, diff_pct, elapsed_ms, region_desc);
                    }
                }
                Err(e) => return tool_error("desktop_verification", e),
            }

            if let Err(e) =
                interruptible_sleep(std::time::Duration::from_millis(verification.poll_ms))
            {
                return tool_error("desktop_verification", e);
            }
        };
        // Clone raw pixels before cache takes ownership (needed for OCR verification below)
        let raw_for_ocr = if verification.expected_text.is_some() {
            Some(raw.clone())
        } else {
            None
        };
        update_screenshot_cache(raw, png.clone());

        let verification_text = if diff_pct >= verification.threshold_pct {
            format!(
                "Verified screen change: {:.2}% observed after {}ms (threshold {:.2}%).",
                diff_pct, elapsed_ms, verification.threshold_pct
            )
        } else {
            format!(
                "Verification failed: expected screen change >= {:.2}%, observed {:.2}% after {}ms.",
                verification.threshold_pct, diff_pct, elapsed_ms
            )
        };
        let mut text = format!(
            "{summary}. {verification_text} Screenshot {width}x{height}{region_desc}"
        );

        // OCR verification: if expected_text was set, OCR the final screenshot to confirm
        if let Some(ref expected) = verification.expected_text {
            let img = raw_for_ocr.and_then(|raw| image::RgbaImage::from_raw(width, height, raw));
            if let Some(img) = img {
                let search = expected.to_lowercase();
                let found = spawn_with_timeout(std::time::Duration::from_secs(10), move || {
                    ocr_tools::ocr_png_and_search(&img, &search)
                });
                match found {
                    Ok(true) => {
                        text.push_str(&format!(" Text '{}' confirmed via OCR.", expected));
                    }
                    Ok(false) => {
                        text.push_str(&format!(" WARNING: text '{}' not found via OCR.", expected));
                    }
                    Err(e) => {
                        text.push_str(&format!(" WARNING: OCR verification failed: {e}"));
                    }
                }
            }
        }

        if do_screenshot || verification.threshold_pct > 0.0 {
            NativeToolResult::with_image(text, png)
        } else {
            NativeToolResult::text_only(text)
        }
    } else if do_screenshot {
        let mut result = capture_post_action_screenshot(delay_ms);
        result.text = format!("{summary}. {}", result.text);
        result
    } else {
        NativeToolResult::text_only(summary)
    }
}

pub fn summarize_value_for_trace(value: &Value) -> Value {
    match value {
        Value::String(s) => {
            const MAX_LEN: usize = 200;
            if s.len() > MAX_LEN {
                Value::String(format!("{}...", &s[..MAX_LEN]))
            } else {
                Value::String(s.clone())
            }
        }
        Value::Array(items) => Value::Array(items.iter().take(10).map(summarize_value_for_trace).collect()),
        Value::Object(map) => {
            let summarized = map
                .iter()
                .map(|(k, v)| (k.clone(), summarize_value_for_trace(v)))
                .collect();
            Value::Object(summarized)
        }
        _ => value.clone(),
    }
}

pub fn write_desktop_trace_line(entry: &Value) {
    let path = desktop_trace_path();
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let Ok(line) = serde_json::to_string(entry) else {
        return;
    };
    use std::io::Write;
    let _ = writeln!(file, "{line}");
}

pub fn finalize_desktop_result(name: &str, args: &Value, started_at: std::time::Instant, mut result: NativeToolResult) -> NativeToolResult {
    let status = classify_desktop_result_status(&result.text);
    let duration_ms = started_at.elapsed().as_millis() as u64;
    let summary = serde_json::json!({
        "tool": name,
        "status": status,
        "duration_ms": duration_ms,
        "image_count": result.images.len(),
    });

    if !result.text.is_empty() {
        result.text.push_str("\n\n[desktop_result] ");
        result.text.push_str(&summary.to_string());
    } else {
        result.text = format!("[desktop_result] {}", summary);
    }

    write_desktop_trace_line(&serde_json::json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "tool": name,
        "status": status,
        "duration_ms": duration_ms,
        "image_count": result.images.len(),
        "args": summarize_value_for_trace(args),
        "text_preview": result.text.lines().take(8).collect::<Vec<_>>().join("\n"),
    }));

    result
}

/// Helper: take a screenshot after an action with configurable cache parameters.
/// `cache_max_age_ms` — how old a cached screenshot can be before it's considered stale.
/// `cache_threshold_pct` — pixel-diff percentage below which the screen is "unchanged".
///
/// TODO: Own-window exclusion — for Tauri desktop builds, minimize our window before
/// capturing and restore after, so the model sees the target app instead of our UI.
/// For web mode (browser tab), this isn't feasible without browser extensions.
pub fn capture_post_action_screenshot_ext(
    delay_ms: u64,
    cache_max_age_ms: u64,
    cache_threshold_pct: f64,
) -> NativeToolResult {
    if delay_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
    }

    // Capture via xcap directly so we can access raw pixels
    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => {
            return tool_error("screenshot", format!("Failed to enumerate monitors: {e}"))
        }
    };
    let monitor = monitors
        .iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .or(monitors.first());
    let monitor = match monitor {
        Some(m) => m,
        None => return tool_error("screenshot", "No monitors detected"),
    };
    let img = match monitor.capture_image() {
        Ok(i) => i,
        Err(e) => {
            return tool_error("screenshot", format!("Screenshot capture failed: {e}"))
        }
    };

    let width = img.width();
    let height = img.height();
    let new_raw = img.as_raw().to_vec();

    // Check cache: if recent screenshot exists and screen is basically unchanged, reuse it
    if let Some((cached_raw, cached_png)) = get_cached_screenshot(cache_max_age_ms) {
        let diff = pixel_diff_pct(&cached_raw, &new_raw);
        if diff < cache_threshold_pct {
            let optimized = optimize_screenshot_for_vision(&cached_png);
            return NativeToolResult::with_image(
                format!("Screenshot {}x{} (unchanged)", width, height),
                optimized,
            );
        }
    }

    // Screen changed — encode new PNG and update cache
    let png_bytes = match encode_image_to_png(&img) {
        Ok(b) => b,
        Err(e) => return tool_error("screenshot", e),
    };
    update_screenshot_cache(new_raw, png_bytes.clone());

    // Resize + JPEG-compress for vision models (saves tokens)
    let optimized = optimize_screenshot_for_vision(&png_bytes);

    NativeToolResult::with_image(
        format!("Screenshot {}x{}", width, height),
        optimized,
    )
}

/// Helper: take a screenshot after an action with optional delay.
/// Uses a smart cache: if the screen hasn't changed significantly (<5% pixel diff),
/// returns the cached image with an "(unchanged)" note to save tokens.
pub fn capture_post_action_screenshot(delay_ms: u64) -> NativeToolResult {
    capture_post_action_screenshot_ext(delay_ms, 800, 5.0)
}
