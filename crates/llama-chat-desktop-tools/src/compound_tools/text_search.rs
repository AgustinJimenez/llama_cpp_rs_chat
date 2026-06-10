//! Text-based screen search tools: find_and_click_text, wait_for_text_on_screen.

use serde_json::Value;

use super::super::NativeToolResult;
use super::super::parse_int;


/// OCR the screen → find text → click its center → return screenshot.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_find_and_click_text(args: &Value) -> NativeToolResult {
    let search_text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return super::super::tool_error("find_and_click_text", "'text' is required"),
    };
    let delay_ms = args.get("delay_ms").and_then(parse_int).unwrap_or(500) as u64;
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;
    let index = args.get("index").and_then(parse_int).unwrap_or(0) as usize;

    let monitors = match super::super::validated_monitors("find_and_click_text", monitor_idx) {
        Ok(m) => m,
        Err(e) => return e,
    };
    let img = match monitors[monitor_idx].capture_image() {
        Ok(i) => i,
        Err(e) => return super::super::tool_error("find_and_click_text", format!("capturing: {e}")),
    };

    let search = search_text.to_lowercase();
    let mut result = Err("OCR not attempted".to_string());
    for attempt in 0..3u32 {
        if let Err(e) = super::super::ensure_desktop_not_cancelled() {
            return super::super::tool_error("find_and_click_text", e);
        }

        let img_c = img.clone();
        let s = search.clone();
        let timeout = super::super::parse_timeout(args);
        result = super::super::spawn_with_timeout(timeout, move || {
            super::super::ocr_tools::ocr_find_text(&img_c, &s, 0.0, 0.0)
        }).and_then(|r| r);
        if result.is_ok() { break; }
        if attempt < 2 {
            if let Err(e) = super::super::interruptible_sleep(std::time::Duration::from_millis(200)) {
                return super::super::tool_error("find_and_click_text", e);
            }
        }
    }

    match result {
        Ok(matches) => {
            if matches.is_empty() {
                return NativeToolResult::text_only(format!("Text '{search_text}' not found on screen"));
            }
            if index >= matches.len() {
                return NativeToolResult::text_only(format!(
                    "Only {} match(es) found, but index {} requested", matches.len(), index
                ));
            }
            let m = &matches[index];
            let click_args = serde_json::json!({
                "x": m.center_x as i64,
                "y": m.center_y as i64,
                "button": "left",
                "delay_ms": delay_ms,
            });
            let mut result = super::super::tool_click_screen(&click_args);
            let idx_info = if index > 0 { format!(" (index {index})") } else { String::new() };
            let mtext = &m.text;
            let mcx = m.center_x;
            let mcy = m.center_y;
            let rtext = &result.text;
            result.text = format!(
                "Found \"{mtext}\" and clicked at ({mcx:.0}, {mcy:.0}){idx_info}. {rtext}"
            );
            result
        }
        Err(e) => super::super::tool_error("find_and_click_text", format!("OCR: {e}")),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_find_and_click_text(_args: &Value) -> NativeToolResult {
    super::super::tool_error("find_and_click_text", "not available on this platform")
}

/// Poll OCR until specified text appears on screen.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_wait_for_text_on_screen(args: &Value) -> NativeToolResult {
    let search_text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return super::super::tool_error("wait_for_text_on_screen", "'text' is required"),
    };
    let timeout_ms = args.get("timeout_ms").and_then(parse_int).unwrap_or(10000).clamp(500, 30000) as u64;
    let poll_ms = args.get("poll_ms").and_then(parse_int).unwrap_or(1000).clamp(500, 10000) as u64;
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    let start = std::time::Instant::now();
    let search = search_text.to_lowercase();
    let base_poll = poll_ms;
    let mut attempt = 0u32;

    loop {
        if let Err(e) = super::super::ensure_desktop_not_cancelled() {
            return super::super::tool_error("wait_for_text_on_screen", e);
        }

        let monitors = match super::super::validated_monitors("wait_for_text_on_screen", monitor_idx) {
            Ok(m) => m,
            Err(e) => return e,
        };
        let img = match monitors[monitor_idx].capture_image() {
            Ok(i) => i,
            Err(e) => return super::super::tool_error("wait_for_text_on_screen", format!("capturing: {e}")),
        };

        let s = search.clone();
        let result = super::super::spawn_with_timeout(std::time::Duration::from_secs(10), move || {
            super::super::ocr_tools::ocr_find_text(&img, &s, 0.0, 0.0)
        }).and_then(|r| r);

        if let Ok(matches) = result {
            if !matches.is_empty() {
                let m = &matches[0];
                let mtext = &m.text;
                let mcx = m.center_x;
                let mcy = m.center_y;
                let elapsed = start.elapsed().as_millis();
                return NativeToolResult::text_only(format!(
                    "Text '{mtext}' found at ({mcx:.0}, {mcy:.0}) after {elapsed}ms"
                ));
            }
        }

        if start.elapsed().as_millis() >= timeout_ms as u128 {
            return NativeToolResult::text_only(format!(
                "Timeout: text '{search_text}' not found after {timeout_ms}ms"
            ));
        }

        let adaptive_delay = super::super::adaptive_poll_ms(attempt, base_poll, base_poll * 4);
        if let Err(e) = super::super::interruptible_sleep(std::time::Duration::from_millis(adaptive_delay)) {
            return super::super::tool_error("wait_for_text_on_screen", e);
        }
        attempt += 1;
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_wait_for_text_on_screen(_args: &Value) -> NativeToolResult {
    super::super::tool_error("wait_for_text_on_screen", "not available on this platform")
}
