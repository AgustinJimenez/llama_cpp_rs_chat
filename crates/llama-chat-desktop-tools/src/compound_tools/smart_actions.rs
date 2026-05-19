//! Smart automation tools: smart_wait, click_and_verify.

use serde_json::Value;

use super::super::NativeToolResult;
use super::super::parse_int;
use super::super::parse_float;
use super::text_search::tool_find_and_click_text;

// ─── smart_wait ─────────────────────────────────────────────────────────────

/// Wait until screen changes, specific text appears via OCR, or both — depending on mode.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_smart_wait(args: &Value) -> NativeToolResult {
    let text = args.get("text").and_then(|v| v.as_str()).map(|s| s.to_string());
    let timeout_ms = args.get("timeout_ms").and_then(parse_int).unwrap_or(10000).clamp(500, 30000) as u64;
    let poll_ms = args.get("poll_ms").and_then(parse_int).unwrap_or(500).clamp(200, 2000) as u64;
    let mode = args.get("mode").and_then(|v| v.as_str()).unwrap_or("any");
    let threshold_pct = args.get("threshold").and_then(parse_float).unwrap_or(1.0).clamp(0.1, 50.0);
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    if text.is_none() && mode == "all" {
        return super::super::tool_error("smart_wait", "'text' is required when mode is 'all'");
    }

    // Capture baseline
    let monitors = match super::super::validated_monitors("smart_wait", monitor_idx) {
        Ok(m) => m,
        Err(e) => return e,
    };
    let baseline_img = match monitors[monitor_idx].capture_image() {
        Ok(i) => i,
        Err(e) => return super::super::tool_error("smart_wait", format!("capture: {e}")),
    };
    let baseline_raw = baseline_img.as_raw().to_vec();

    let start = std::time::Instant::now();
    let mut screen_changed = false;
    let mut text_found = false;
    let mut attempt = 0u32;

    if text.is_none() {
        text_found = true;
    }

    loop {
        if let Err(e) = super::super::ensure_desktop_not_cancelled() {
            return super::super::tool_error("smart_wait", e);
        }
        let elapsed = start.elapsed().as_millis() as u64;
        if elapsed >= timeout_ms {
            break;
        }

        let monitors = match xcap::Monitor::all() {
            Ok(m) => m,
            Err(_) => break,
        };
        if monitor_idx >= monitors.len() {
            break;
        }

        if !screen_changed {
            if let Ok(img) = monitors[monitor_idx].capture_image() {
                let diff = super::super::pixel_diff_pct(&baseline_raw, img.as_raw());
                if diff >= threshold_pct {
                    screen_changed = true;
                }
            }
        }

        if !text_found {
            if let Some(ref search_text) = text {
                if let Ok(img) = monitors[monitor_idx].capture_image() {
                    let search = search_text.to_lowercase();
                    let ocr_result = super::super::spawn_with_timeout(
                        std::time::Duration::from_secs(10),
                        move || {
                            super::super::ocr_tools::ocr_find_text(&img, &search, 0.0, 0.0)
                        },
                    ).and_then(|r| r);
                    if let Ok(matches) = ocr_result {
                        if !matches.is_empty() {
                            text_found = true;
                        }
                    }
                }
            }
        }

        let done = match mode {
            "all" => screen_changed && text_found,
            _ => screen_changed || text_found,
        };
        if done {
            break;
        }

        let sleep_ms = super::super::adaptive_poll_ms(attempt, poll_ms, poll_ms * 4);
        if let Err(e) = super::super::interruptible_sleep(std::time::Duration::from_millis(sleep_ms)) {
            return super::super::tool_error("smart_wait", e);
        }
        attempt += 1;
    }

    let elapsed = start.elapsed().as_millis();
    let mut result_parts = Vec::new();
    if screen_changed {
        result_parts.push("screen changed".to_string());
    }
    if text_found && text.is_some() {
        result_parts.push(format!("text '{}' found", text.as_deref().unwrap()));
    }

    let screenshot = super::super::capture_post_action_screenshot(0);
    if result_parts.is_empty() {
        NativeToolResult {
            text: format!("Timeout after {}ms: no conditions met (mode={})", elapsed, mode),
            images: screenshot.images,
        }
    } else {
        NativeToolResult {
            text: format!("Wait complete after {}ms: {} (mode={})", elapsed, result_parts.join(" + "), mode),
            images: screenshot.images,
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_smart_wait(_args: &Value) -> NativeToolResult {
    super::super::tool_error("smart_wait", "not available on this platform")
}

// ─── click_and_verify ───────────────────────────────────────────────────────

/// Find text on screen via OCR, click it, then verify different text appeared.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_click_and_verify(args: &Value) -> NativeToolResult {
    let click_text = match args.get("click_text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return super::super::tool_error("click_and_verify", "'click_text' is required"),
    };
    let expect_text = match args.get("expect_text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return super::super::tool_error("click_and_verify", "'expect_text' is required"),
    };
    let timeout_ms = args.get("timeout_ms").and_then(parse_int).unwrap_or(5000).clamp(500, 30000) as u64;
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    let click_result = tool_find_and_click_text(&serde_json::json!({
        "text": click_text,
        "monitor": monitor_idx,
        "delay_ms": 500,
        "screenshot": false
    }));
    if click_result.text.starts_with("Error") {
        return click_result;
    }

    let wait_result = tool_smart_wait(&serde_json::json!({
        "text": expect_text,
        "timeout_ms": timeout_ms,
        "monitor": monitor_idx,
        "mode": "any"
    }));

    let verified = wait_result.text.contains("text '") && wait_result.text.contains("found");
    let screenshot = super::super::capture_post_action_screenshot(0);
    NativeToolResult {
        text: format!(
            "Clicked '{}'. Verification: {} — {}",
            click_text,
            if verified { "PASSED" } else { "FAILED" },
            wait_result.text
        ),
        images: screenshot.images,
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_click_and_verify(_args: &Value) -> NativeToolResult {
    super::super::tool_error("click_and_verify", "not available on this platform")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smart_wait_mode_all_requires_text() {
        let args = serde_json::json!({ "mode": "all", "timeout_ms": 500 });
        let result = tool_smart_wait(&args);
        assert!(result.text.contains("Error [smart_wait]"));
        assert!(result.text.contains("'text' is required when mode is 'all'"));
    }

    #[test]
    fn test_smart_wait_mode_any_without_text_starts() {
        let args = serde_json::json!({ "mode": "any", "timeout_ms": 500 });
        let result = tool_smart_wait(&args);
        assert!(!result.text.contains("'text' is required"));
    }

    #[test]
    fn test_smart_wait_timeout_clamped() {
        let args = serde_json::json!({ "timeout_ms": 100, "text": "x" });
        let result = tool_smart_wait(&args);
        assert!(!result.text.contains("Error [smart_wait]: 'text'"));
    }

    #[test]
    fn test_click_and_verify_missing_click_text() {
        let args = serde_json::json!({ "expect_text": "OK" });
        let result = tool_click_and_verify(&args);
        assert!(result.text.contains("Error [click_and_verify]"));
        assert!(result.text.contains("'click_text' is required"));
    }

    #[test]
    fn test_click_and_verify_missing_expect_text() {
        let args = serde_json::json!({ "click_text": "Save" });
        let result = tool_click_and_verify(&args);
        assert!(result.text.contains("Error [click_and_verify]"));
        assert!(result.text.contains("'expect_text' is required"));
    }

    #[test]
    fn test_click_and_verify_missing_both() {
        let args = serde_json::json!({});
        let result = tool_click_and_verify(&args);
        assert!(result.text.contains("Error [click_and_verify]"));
    }
}
