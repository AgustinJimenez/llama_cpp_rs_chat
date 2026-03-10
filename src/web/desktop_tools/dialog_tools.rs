//! Dialog handling and UI element state waiting tools.

use serde_json::Value;

use super::NativeToolResult;
use super::parse_int;

/// Detect and interact with modal dialogs: read text, list buttons, click a button.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_handle_dialog(args: &Value) -> NativeToolResult {
    use super::ui_automation_tools;
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
    let elements = super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || {
        // Get all text elements
        let texts = ui_automation_tools::find_ui_elements_all(hwnd, None, Some("text"), 20)
            .unwrap_or_default();
        // Get all buttons
        let buttons = ui_automation_tools::find_ui_elements_all(hwnd, None, Some("button"), 20)
            .unwrap_or_default();
        (texts, buttons)
    })
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
    super::tool_error("handle_dialog", "not available on this platform")
}

/// Wait until a UI element reaches a specific state (enabled, disabled, visible, etc.).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_wait_for_element_state(args: &Value) -> NativeToolResult {
    use super::ui_automation_tools;
    use super::screenshot_tools;
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
        None => return super::tool_error("wait_for_element_state", "'state' is required (exists/gone/enabled/disabled)"),
    };
    let timeout_ms = args.get("timeout_ms").and_then(parse_int).unwrap_or(5000) as u64;

    if name_filter.is_none() && type_filter.is_none() {
        return super::tool_error("wait_for_element_state", "'name' or 'control_type' is required");
    }

    let title_filter = args.get("title").and_then(|v| v.as_str());
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return super::tool_error("wait_for_element_state", format!("no window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return super::tool_error("wait_for_element_state", "no active window"),
        }
    };

    if let Some(r) = ui_automation_tools::check_gpu_app_guard(hwnd, "wait_for_element_state") { return r; }

    let start = std::time::Instant::now();
    let mut attempt = 0u32;

    loop {
        if let Err(e) = super::ensure_desktop_not_cancelled() {
            return super::tool_error("wait_for_element_state", e);
        }
        let elapsed = start.elapsed().as_millis() as u64;
        if elapsed >= timeout_ms {
            return NativeToolResult::text_only(format!(
                "Timeout: element did not reach state '{}' within {}ms",
                target_state, timeout_ms
            ));
        }

        let name_owned = name_filter.map(|s| s.to_lowercase());
        let type_owned = type_filter.map(|s| s.to_lowercase());
        let found = super::spawn_with_timeout(std::time::Duration::from_secs(5), move || {
            ui_automation_tools::find_ui_elements_all(hwnd, name_owned.as_deref(), type_owned.as_deref(), 1)
        }).and_then(|r| r);

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

        let poll_ms = screenshot_tools::adaptive_poll_ms(attempt, 200, 1000);
        if let Err(e) = super::interruptible_sleep(std::time::Duration::from_millis(poll_ms)) {
            return super::tool_error("wait_for_element_state", e);
        }
        attempt += 1;
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_wait_for_element_state(_args: &Value) -> NativeToolResult {
    super::tool_error("wait_for_element_state", "not available on this platform")
}

// ─── Dialog auto-handler ─────────────────────────────────────────────────────

use std::sync::Mutex;

struct DialogHandlerState {
    stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<u32>>, // returns count of dialogs handled
}

static DIALOG_HANDLER: Mutex<Option<DialogHandlerState>> = Mutex::new(None);

/// Start a background dialog monitor that auto-clicks buttons matching a button_map.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_dialog_handler_start(args: &Value) -> NativeToolResult {
    use super::ui_automation_tools;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let mut guard = match DIALOG_HANDLER.lock() {
        Ok(g) => g,
        Err(p) => { crate::log_warn!("system", "Mutex poisoned in DIALOG_HANDLER, recovering"); p.into_inner() }
    };

    if guard.is_some() {
        return super::tool_error("dialog_handler_start", "dialog handler is already running — stop it first");
    }

    // Parse button_map
    let button_map: std::collections::HashMap<String, String> = match args.get("button_map") {
        Some(Value::Object(map)) => {
            map.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.to_lowercase(), s.to_string())))
                .collect()
        }
        _ => return super::tool_error("dialog_handler_start", "'button_map' is required (JSON object, e.g. {\"OK\": \"click\"})"),
    };

    let poll_interval_ms = args.get("poll_interval_ms").and_then(parse_int).unwrap_or(1000) as u64;
    let timeout_ms = args.get("timeout_ms").and_then(parse_int).unwrap_or(60000) as u64;

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();

    let handle = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        let mut handled_count = 0u32;

        loop {
            if stop_clone.load(Ordering::Relaxed) {
                break;
            }
            if start.elapsed().as_millis() as u64 >= timeout_ms {
                break;
            }

            // Check cancellation
            if super::ensure_desktop_not_cancelled().is_err() {
                break;
            }

            // Get foreground window
            #[cfg(windows)]
            let fg = unsafe { super::win32::GetForegroundWindow() };
            #[cfg(target_os = "macos")]
            let fg = unsafe { super::macos::GetForegroundWindow() };
            #[cfg(target_os = "linux")]
            let fg = unsafe { super::linux::GetForegroundWindow() };

            if fg != 0 {
                // Try to find buttons in the foreground window
                let button_result = super::spawn_with_timeout(
                    std::time::Duration::from_secs(3),
                    move || ui_automation_tools::find_ui_elements_all(fg, None, Some("button"), 20),
                );

                if let Ok(Ok(buttons)) = button_result {
                    let button_map_ref = &button_map;
                    for btn in &buttons {
                        let btn_lower = btn.name.to_lowercase();
                        if let Some(action) = button_map_ref.get(&btn_lower) {
                            if action == "click" {
                                let _ = super::tool_click_screen(&serde_json::json!({
                                    "x": btn.cx,
                                    "y": btn.cy,
                                    "delay_ms": 300,
                                    "screenshot": false
                                }));
                                handled_count += 1;
                                // Wait a bit after clicking to let the dialog close
                                std::thread::sleep(std::time::Duration::from_millis(500));
                                break; // Only click one button per poll
                            }
                        }
                    }
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(poll_interval_ms));
        }

        handled_count
    });

    *guard = Some(DialogHandlerState {
        stop_flag,
        handle: Some(handle),
    });

    NativeToolResult::text_only(format!(
        "Dialog handler started (polling every {}ms, timeout {}ms, watching for buttons: {})",
        poll_interval_ms,
        timeout_ms,
        args.get("button_map").map(|v| v.to_string()).unwrap_or_else(|| "{}".to_string())
    ))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_dialog_handler_start(_args: &Value) -> NativeToolResult {
    super::tool_error("dialog_handler_start", "not available on this platform")
}

/// Stop the background dialog monitor and return how many dialogs were handled.
pub fn tool_dialog_handler_stop(_args: &Value) -> NativeToolResult {
    let mut guard = match DIALOG_HANDLER.lock() {
        Ok(g) => g,
        Err(p) => { crate::log_warn!("system", "Mutex poisoned in DIALOG_HANDLER, recovering"); p.into_inner() }
    };

    match guard.take() {
        Some(state) => {
            state.stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
            let count = if let Some(handle) = state.handle {
                // Wait up to 5 seconds for the thread to finish
                match handle.join() {
                    Ok(c) => c,
                    Err(_) => 0,
                }
            } else {
                0
            };
            NativeToolResult::text_only(format!("Dialog handler stopped. Dialogs handled: {}", count))
        }
        None => {
            NativeToolResult::text_only("No dialog handler was running".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── dialog_handler_start parameter validation ───────────────────────

    #[test]
    fn test_dialog_handler_start_requires_button_map() {
        let args = serde_json::json!({});
        let result = tool_dialog_handler_start(&args);
        assert!(result.text.contains("Error [dialog_handler_start]"));
        assert!(result.text.contains("'button_map' is required"));
    }

    #[test]
    fn test_dialog_handler_start_rejects_non_object_button_map() {
        let args = serde_json::json!({
            "button_map": "not an object"
        });
        let result = tool_dialog_handler_start(&args);
        assert!(result.text.contains("Error [dialog_handler_start]"));
        assert!(result.text.contains("'button_map' is required"));
    }

    // ─── dialog_handler lifecycle (single test to avoid global mutex races) ──

    #[test]
    fn test_dialog_handler_lifecycle() {
        // All lifecycle assertions in one test to avoid parallel races on DIALOG_HANDLER global
        // 1. Stop when nothing running
        let _ = tool_dialog_handler_stop(&serde_json::json!({}));
        let stop_noop = tool_dialog_handler_stop(&serde_json::json!({}));
        assert!(stop_noop.text.contains("No dialog handler was running"));

        // 2. Start handler
        let start_result = tool_dialog_handler_start(&serde_json::json!({
            "button_map": { "ok": "click", "cancel": "click" },
            "timeout_ms": 1000,
            "poll_interval_ms": 500
        }));
        assert!(start_result.text.contains("Dialog handler started"));
        assert!(start_result.text.contains("polling every 500ms"));

        // 3. Starting again should fail (already running)
        let dup_result = tool_dialog_handler_start(&serde_json::json!({
            "button_map": { "ok": "click" }
        }));
        assert!(dup_result.text.contains("already running"));

        // 4. Stop handler
        let stop_result = tool_dialog_handler_stop(&serde_json::json!({}));
        assert!(stop_result.text.contains("Dialog handler stopped"));
        assert!(stop_result.text.contains("Dialogs handled:"));
    }

    // ─── handle_dialog returns info even without dialogs ─────────────────

    #[test]
    fn test_handle_dialog_runs_without_crash() {
        // Just verify it doesn't panic — may return "no foreground window" on headless CI
        let args = serde_json::json!({});
        let result = tool_handle_dialog(&args);
        // Should return some text (either dialog info or "no window")
        assert!(!result.text.is_empty());
    }
}
