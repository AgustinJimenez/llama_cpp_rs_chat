//! Dialog handling and UI element state waiting tools.

use serde_json::Value;

use super::NativeToolResult;
use super::parse_int;

/// Detect and interact with modal dialogs: read text, list buttons, click a button.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_handle_dialog(args: &Value) -> NativeToolResult {
    use super::ui_tools;
    #[cfg(windows)]
    use super::win32;
    #[cfg(target_os = "macos")]
    use super::macos as win32;
    #[cfg(target_os = "linux")]
    use super::linux as win32;

    let button_to_click = args.get("button").and_then(|v| v.as_str());

    // Get foreground window
    let fg = unsafe { win32::GetForegroundWindow() };
    if fg == 0 {
        return NativeToolResult::text_only("No foreground window found".to_string());
    }

    // Check if it's a dialog/modal (check for popup or small window)
    let info = match win32::get_active_window_info() {
        Some((_, info)) => info,
        None => return NativeToolResult::text_only("Cannot get active window info".to_string()),
    };

    // Use UI Automation to enumerate elements in the dialog
    let hwnd = fg;
    let elements = std::thread::spawn(move || {
        // Get all text elements
        let texts = ui_tools::find_ui_elements_all(hwnd, None, Some("text"), 20)
            .unwrap_or_default();
        // Get all buttons
        let buttons = ui_tools::find_ui_elements_all(hwnd, None, Some("button"), 20)
            .unwrap_or_default();
        (texts, buttons)
    })
    .join()
    .unwrap_or_else(|_| (Vec::new(), Vec::new()));

    let (texts, buttons) = elements;

    // Build dialog description
    let dialog_text: Vec<String> = texts
        .iter()
        .filter(|t| !t.name.is_empty())
        .map(|t| t.name.clone())
        .collect();

    let button_names: Vec<String> = buttons
        .iter()
        .filter(|b| !b.name.is_empty())
        .map(|b| b.name.clone())
        .collect();

    let mut output = format!(
        "Dialog: '{}' [{}]\n",
        info.title, info.class_name
    );
    if !dialog_text.is_empty() {
        output.push_str(&format!("Text: {}\n", dialog_text.join(" | ")));
    }
    if !button_names.is_empty() {
        output.push_str(&format!("Buttons: [{}]\n", button_names.join(", ")));
    }

    // Click specified button if requested
    if let Some(btn_name) = button_to_click {
        let btn_lower = btn_name.to_lowercase();
        if let Some(btn) = buttons.iter().find(|b| b.name.to_lowercase().contains(&btn_lower)) {
            let result = super::tool_click_screen(&serde_json::json!({
                "x": btn.cx,
                "y": btn.cy,
                "delay_ms": 500,
                "screenshot": true
            }));
            output.push_str(&format!("Clicked button '{}' at ({}, {})", btn.name, btn.cx, btn.cy));
            return NativeToolResult {
                text: output,
                images: result.images,
            };
        } else {
            output.push_str(&format!("Button '{}' not found in dialog", btn_name));
        }
    }

    // Take screenshot of dialog
    let screenshot = super::capture_post_action_screenshot(0);
    NativeToolResult {
        text: output,
        images: screenshot.images,
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_handle_dialog(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: handle_dialog is not available on this platform".to_string())
}

/// Wait until a UI element reaches a specific state (enabled, disabled, visible, etc.).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_wait_for_element_state(args: &Value) -> NativeToolResult {
    use super::ui_tools;
    #[cfg(windows)]
    use super::win32;
    #[cfg(target_os = "macos")]
    use super::macos as win32;
    #[cfg(target_os = "linux")]
    use super::linux as win32;

    let name_filter = args.get("name").and_then(|v| v.as_str());
    let type_filter = args.get("control_type").and_then(|v| v.as_str());
    let target_state = match args.get("state").and_then(|v| v.as_str()) {
        Some(s) => s.to_lowercase(),
        None => return NativeToolResult::text_only("Error: 'state' is required (exists/gone/enabled/disabled)".to_string()),
    };
    let timeout_ms = args.get("timeout_ms").and_then(parse_int).unwrap_or(5000) as u64;

    if name_filter.is_none() && type_filter.is_none() {
        return NativeToolResult::text_only("Error: 'name' or 'control_type' is required".to_string());
    }

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

    let start = std::time::Instant::now();
    let mut attempt = 0u32;

    loop {
        let elapsed = start.elapsed().as_millis() as u64;
        if elapsed >= timeout_ms {
            return NativeToolResult::text_only(format!(
                "Timeout: element did not reach state '{}' within {}ms",
                target_state, timeout_ms
            ));
        }

        let name_owned = name_filter.map(|s| s.to_lowercase());
        let type_owned = type_filter.map(|s| s.to_lowercase());
        let found = std::thread::spawn(move || {
            ui_tools::find_ui_elements_all(hwnd, name_owned.as_deref(), type_owned.as_deref(), 1)
        })
        .join()
        .unwrap_or_else(|_| Err("Thread panicked".to_string()));

        let state_matches = match target_state.as_str() {
            "exists" | "visible" => found.as_ref().map_or(false, |v| !v.is_empty()),
            "gone" | "hidden" => found.as_ref().map_or(true, |v| v.is_empty()),
            _ => {
                return NativeToolResult::text_only(format!(
                    "Unknown state '{}'. Use: exists, gone, visible, hidden",
                    target_state
                ));
            }
        };

        if state_matches {
            let screenshot = super::capture_post_action_screenshot(0);
            let name_desc = name_filter.unwrap_or("(any)");
            return NativeToolResult {
                text: format!(
                    "Element '{}' reached state '{}' after {}ms",
                    name_desc,
                    target_state,
                    start.elapsed().as_millis()
                ),
                images: screenshot.images,
            };
        }

        let poll_ms = ui_tools::adaptive_poll_ms(attempt, 200, 1000);
        std::thread::sleep(std::time::Duration::from_millis(poll_ms));
        attempt += 1;
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_wait_for_element_state(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: wait_for_element_state is not available on this platform".to_string())
}
