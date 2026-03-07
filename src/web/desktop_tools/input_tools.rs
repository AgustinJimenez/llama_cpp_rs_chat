//! High-level input tools: paste, clear field, hover element.

use serde_json::Value;

use super::NativeToolResult;
use super::parse_int;

/// Paste clipboard contents at the current cursor position (Ctrl+V).
pub fn tool_paste(args: &Value) -> NativeToolResult {
    let delay_ms = args
        .get("delay_ms")
        .and_then(parse_int)
        .unwrap_or(300) as u64;

    // Press Ctrl+V
    let result = super::tool_press_key(&serde_json::json!({
        "key": "ctrl+v",
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

    // Select all
    super::tool_press_key(&serde_json::json!({"key": "ctrl+a", "delay_ms": 50, "screenshot": false}));
    // Delete selection
    super::tool_press_key(&serde_json::json!({"key": "delete", "delay_ms": 100, "screenshot": false}));

    if let Some(text) = then_type {
        let result = super::tool_type_text(&serde_json::json!({
            "text": text,
            "delay_ms": delay_ms,
            "screenshot": true
        }));
        NativeToolResult {
            text: format!("Cleared field and typed '{}'. {}", text, result.text),
            images: result.images,
        }
    } else {
        // Take final screenshot
        let result = super::tool_press_key(&serde_json::json!({"key": "delete", "delay_ms": delay_ms, "screenshot": true}));
        NativeToolResult {
            text: format!("Cleared field. {}", result.text),
            images: result.images,
        }
    }
}

/// Hover over a UI element by name/type, wait, and capture tooltip if present.
#[cfg(windows)]
pub fn tool_hover_element(args: &Value) -> NativeToolResult {
    use super::ui_tools;
    use super::win32;

    let name_filter = args.get("name").and_then(|v| v.as_str());
    let type_filter = args.get("control_type").and_then(|v| v.as_str());
    let hover_ms = args.get("hover_ms").and_then(parse_int).unwrap_or(800) as u64;

    if name_filter.is_none() && type_filter.is_none() {
        return NativeToolResult::text_only("Error: 'name' or 'control_type' is required".to_string());
    }

    // Find window
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

    let element = match std::thread::spawn(move || {
        ui_tools::find_ui_element(hwnd, name_owned.as_deref(), type_owned.as_deref())
    })
    .join()
    .unwrap_or_else(|_| Err("UI Automation thread panicked".to_string()))
    {
        Ok(el) => el,
        Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
    };

    // Move mouse to element center
    super::tool_move_mouse(&serde_json::json!({
        "x": element.cx,
        "y": element.cy,
        "screenshot": false
    }));

    // Wait for hover effects (tooltip)
    std::thread::sleep(std::time::Duration::from_millis(hover_ms));

    // Try to find tooltip in UI tree
    let tooltip_text = std::thread::spawn(move || -> String {
        match ui_tools::find_ui_elements_all(hwnd, None, Some("tooltip"), 1) {
            Ok(tips) if !tips.is_empty() => format!(" Tooltip: '{}'", tips[0].name),
            _ => String::new(),
        }
    })
    .join()
    .unwrap_or_default();

    // Take screenshot showing hover state
    let screenshot = super::capture_post_action_screenshot(0);

    NativeToolResult {
        text: format!(
            "Hovering over '{}' [{}] at ({}, {}).{}",
            element.name, element.control_type, element.cx, element.cy, tooltip_text
        ),
        images: screenshot.images,
    }
}

#[cfg(not(windows))]
pub fn tool_hover_element(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: hover_element is only available on Windows".to_string())
}
