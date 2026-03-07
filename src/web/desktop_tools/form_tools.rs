//! Form filling and action sequence execution tools.

use serde_json::Value;

use super::NativeToolResult;
use super::parse_int;

/// Fill multiple form fields by label/name and value pairs.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_fill_form(args: &Value) -> NativeToolResult {
    use super::ui_tools;
    #[cfg(windows)]
    use super::win32;
    #[cfg(target_os = "macos")]
    use super::macos as win32;
    #[cfg(target_os = "linux")]
    use super::linux as win32;

    let fields = match args.get("fields").and_then(|v| v.as_array()) {
        Some(f) => f,
        None => {
            return NativeToolResult::text_only(
                "Error: 'fields' array is required, e.g. [{\"label\":\"Name\",\"value\":\"John\"}]"
                    .to_string(),
            )
        }
    };

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

    let mut filled = Vec::new();
    let mut errors = Vec::new();

    for field in fields {
        let label = field
            .get("label")
            .or_else(|| field.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let value = field.get("value").and_then(|v| v.as_str()).unwrap_or("");

        if label.is_empty() {
            errors.push("Skipped field with empty label".to_string());
            continue;
        }

        // Find the element by name
        let label_lower = label.to_lowercase();
        let element = std::thread::spawn(move || {
            ui_tools::find_ui_element(hwnd, Some(&label_lower), None)
        })
        .join()
        .unwrap_or_else(|_| Err("Thread panicked".to_string()));

        match element {
            Ok(el) => {
                // Click the element
                super::tool_click_screen(&serde_json::json!({
                    "x": el.cx, "y": el.cy, "delay_ms": 100, "screenshot": false
                }));
                // Clear and type
                super::tool_press_key(&serde_json::json!({"key": "ctrl+a", "delay_ms": 50, "screenshot": false}));
                super::tool_type_text(&serde_json::json!({
                    "text": value, "delay_ms": 50, "screenshot": false
                }));
                // Tab to next field
                super::tool_press_key(&serde_json::json!({"key": "tab", "delay_ms": 50, "screenshot": false}));
                filled.push(format!("'{}' = '{}'", label, value));
            }
            Err(e) => errors.push(format!("'{}': {}", label, e)),
        }
    }

    let screenshot = super::capture_post_action_screenshot(200);
    let mut output = format!("Filled {} field(s): {}", filled.len(), filled.join(", "));
    if !errors.is_empty() {
        output.push_str(&format!("\nErrors: {}", errors.join("; ")));
    }

    NativeToolResult {
        text: output,
        images: screenshot.images,
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_fill_form(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: fill_form is not available on this platform".to_string())
}

/// Execute a sequence of desktop actions (click, type, press_key, paste, wait, clear).
pub fn tool_run_action_sequence(args: &Value) -> NativeToolResult {
    let actions = match args.get("actions").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => {
            return NativeToolResult::text_only(
                "Error: 'actions' array is required, e.g. [{\"action\":\"click\",\"x\":100,\"y\":200}]"
                    .to_string(),
            )
        }
    };

    let default_delay = args
        .get("delay_between_ms")
        .and_then(parse_int)
        .unwrap_or(200) as u64;

    let mut results = Vec::new();

    for (i, action) in actions.iter().enumerate() {
        let action_type = match action.get("action").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => {
                results.push(format!("#{}: skipped (no 'action' field)", i + 1));
                continue;
            }
        };

        // Suppress screenshots for intermediate actions
        let mut action_args = action.clone();
        if let Some(obj) = action_args.as_object_mut() {
            if i < actions.len() - 1 {
                obj.insert("screenshot".to_string(), serde_json::json!(false));
            }
        }

        match action_type {
            "click" => {
                let r = super::tool_click_screen(&action_args);
                results.push(format!("#{}: click → {}", i + 1, r.text));
            }
            "type" => {
                let r = super::tool_type_text(&action_args);
                results.push(format!("#{}: type → {}", i + 1, r.text));
            }
            "press_key" | "key" => {
                let r = super::tool_press_key(&action_args);
                results.push(format!("#{}: key → {}", i + 1, r.text));
            }
            "paste" => {
                let r = super::input_tools::tool_paste(&action_args);
                results.push(format!("#{}: paste → {}", i + 1, r.text));
            }
            "clear" => {
                let r = super::input_tools::tool_clear_field(&action_args);
                results.push(format!("#{}: clear → {}", i + 1, r.text));
            }
            "wait" => {
                let ms = action.get("ms").and_then(parse_int).unwrap_or(500) as u64;
                std::thread::sleep(std::time::Duration::from_millis(ms));
                results.push(format!("#{}: waited {}ms", i + 1, ms));
            }
            "scroll" => {
                let r = super::tool_scroll_screen(&action_args);
                results.push(format!("#{}: scroll → {}", i + 1, r.text));
            }
            "move" => {
                let r = super::tool_move_mouse(&action_args);
                results.push(format!("#{}: move → {}", i + 1, r.text));
            }
            other => {
                results.push(format!("#{}: unknown action '{}'", i + 1, other));
            }
        }

        // Delay between actions (except after last)
        if i < actions.len() - 1 {
            let delay = action
                .get("delay_ms")
                .and_then(parse_int)
                .unwrap_or(default_delay as i64) as u64;
            std::thread::sleep(std::time::Duration::from_millis(delay));
        }
    }

    // Final screenshot
    let screenshot = super::capture_post_action_screenshot(0);
    NativeToolResult {
        text: format!(
            "Executed {} action(s):\n{}",
            actions.len(),
            results.join("\n")
        ),
        images: screenshot.images,
    }
}
