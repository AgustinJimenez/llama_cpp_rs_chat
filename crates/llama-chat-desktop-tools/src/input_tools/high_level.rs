//! High-level input tools: paste, clear field, and hover element.

use serde_json::Value;

use crate::helpers::{parse_int, tool_error};
use crate::trace::capture_post_action_screenshot;
use crate::NativeToolResult;

use super::keyboard::tool_press_key;
use super::keyboard::tool_type_text;
use super::mouse::tool_move_mouse;

/// Paste clipboard contents at the current cursor position (Ctrl+V).
pub fn tool_paste(args: &Value) -> NativeToolResult {
    let delay_ms = args
        .get("delay_ms")
        .and_then(parse_int)
        .unwrap_or(300) as u64;

    // Press Ctrl+V (Cmd+V on macOS)
    #[cfg(target_os = "macos")]
    let paste_key = "meta+v";
    #[cfg(not(target_os = "macos"))]
    let paste_key = "ctrl+v";
    let result = tool_press_key(&serde_json::json!({
        "key": paste_key,
        "delay_ms": delay_ms,
        "screenshot": true
    }));
    // The press_key tool already takes a screenshot; augment message
    NativeToolResult {
        text: format!("Pasted from clipboard. {}", result.text),
        images: result.images,
    }
}

/// Clear the currently focused field (Ctrl+A → Delete), optionally type new text.
pub fn tool_clear_field(args: &Value) -> NativeToolResult {
    let then_type = args.get("then_type").and_then(|v| v.as_str());
    let delay_ms = args
        .get("delay_ms")
        .and_then(parse_int)
        .unwrap_or(200) as u64;

    // Select all (Cmd+A on macOS)
    #[cfg(target_os = "macos")]
    let select_all_key = "meta+a";
    #[cfg(not(target_os = "macos"))]
    let select_all_key = "ctrl+a";
    tool_press_key(&serde_json::json!({"key": select_all_key, "delay_ms": 50, "screenshot": false}));
    // Delete selection
    tool_press_key(&serde_json::json!({"key": "delete", "delay_ms": 100, "screenshot": false}));

    if let Some(text) = then_type {
        let result = tool_type_text(&serde_json::json!({
            "text": text,
            "delay_ms": delay_ms,
            "screenshot": true
        }));
        NativeToolResult {
            text: format!("Cleared field and typed '{}'. {}", text, result.text),
            images: result.images,
        }
    } else {
        // Take final screenshot (selection already deleted above, just capture)
        let result = capture_post_action_screenshot(delay_ms);
        NativeToolResult {
            text: format!("Cleared field. {}", result.text),
            images: result.images,
        }
    }
}

/// Hover over a UI element by name/type, wait, and capture tooltip if present.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_hover_element(args: &Value) -> NativeToolResult {
    use crate::ui_automation_tools;
    #[cfg(windows)]
    use crate::win32;
    #[cfg(target_os = "macos")]
    use crate::macos as win32;
    #[cfg(target_os = "linux")]
    use crate::linux as win32;

    let name_filter = args.get("name").and_then(|v| v.as_str());
    let type_filter = args.get("control_type").and_then(|v| v.as_str());
    let hover_ms = args.get("hover_ms").and_then(parse_int).unwrap_or(800) as u64;

    if name_filter.is_none() && type_filter.is_none() {
        return tool_error("hover_element", "'name' or 'control_type' is required");
    }

    // Find window
    let title_filter = args.get("title").and_then(|v| v.as_str());
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return tool_error("hover_element", format!("no window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return tool_error("hover_element", "no active window"),
        }
    };

    if let Some(r) = ui_automation_tools::check_gpu_app_guard(hwnd, "hover_element") { return r; }

    let name_owned = name_filter.map(|s| s.to_lowercase());
    let type_owned = type_filter.map(|s| s.to_lowercase());

    let element = match crate::spawn_with_timeout(crate::DEFAULT_THREAD_TIMEOUT, move || {
        ui_automation_tools::find_ui_element(hwnd, name_owned.as_deref(), type_owned.as_deref())
    }).and_then(|r| r)
    {
        Ok(el) => el,
        Err(e) => return tool_error("hover_element", e),
    };

    // Move mouse to element center
    tool_move_mouse(&serde_json::json!({
        "x": element.cx,
        "y": element.cy,
        "screenshot": false
    }));

    // Wait for hover effects (tooltip) — interruptible so it can be cancelled
    if let Err(e) = crate::interruptible_sleep(std::time::Duration::from_millis(hover_ms)) {
        return tool_error("hover_element", e);
    }

    // Try to find tooltip in UI tree
    let tooltip_text = crate::spawn_with_timeout(crate::DEFAULT_THREAD_TIMEOUT, move || -> String {
        match ui_automation_tools::find_ui_elements_all(hwnd, None, Some("tooltip"), 1) {
            Ok(tips) if !tips.is_empty() => format!(" Tooltip: '{}'", tips[0].name),
            _ => String::new(),
        }
    })
    .unwrap_or_default();

    // Take screenshot showing hover state
    use crate::trace::capture_post_action_screenshot;
    let screenshot = capture_post_action_screenshot(0);

    NativeToolResult {
        text: format!(
            "Hovering over '{}' [{}] at ({}, {}).{}",
            element.name, element.control_type, element.cx, element.cy, tooltip_text
        ),
        images: screenshot.images,
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_hover_element(_args: &Value) -> NativeToolResult {
    tool_error("hover_element", "not available on this platform")
}
