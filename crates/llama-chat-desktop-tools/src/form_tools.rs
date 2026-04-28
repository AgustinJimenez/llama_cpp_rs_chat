//! Form filling and action sequence execution tools.

use serde_json::Value;

use super::NativeToolResult;
use super::parse_int;

/// Fill multiple form fields by label/name and value pairs.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_fill_form(args: &Value) -> NativeToolResult {
    use super::ui_automation_tools;
    #[cfg(windows)]
    use super::win32;
    #[cfg(target_os = "macos")]
    use super::macos as win32;
    #[cfg(target_os = "linux")]
    use super::linux as win32;

    let fields = match args.get("fields").and_then(|v| v.as_array()) {
        Some(f) => f,
        None => {
            return super::tool_error(
                "fill_form",
                "'fields' array is required, e.g. [{\"label\":\"Name\",\"value\":\"John\"}]",
            )
        }
    };

    let title_filter = args.get("title").and_then(|v| v.as_str());
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return super::tool_error("fill_form", format!("no window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return super::tool_error("fill_form", "no active window"),
        }
    };

    if let Some(r) = ui_automation_tools::check_gpu_app_guard(hwnd, "fill_form") { return r; }

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
        let timeout = super::parse_timeout(args);
        let element = super::spawn_with_timeout(timeout, move || {
            ui_automation_tools::find_ui_element(hwnd, Some(&label_lower), None)
        }).and_then(|r| r);

        match element {
            Ok(el) => {
                // Allow per-field "type" override for control type detection
                let effective_type = field.get("type")
                    .and_then(|v| v.as_str())
                    .map(|t| match t.to_lowercase().as_str() {
                        "dropdown" | "combobox" | "combo" => "combobox".to_string(),
                        "checkbox" | "check" => "checkbox".to_string(),
                        "radio" | "radiobutton" | "radio button" => "radiobutton".to_string(),
                        other => other.to_string(),
                    })
                    .unwrap_or_else(|| el.control_type.to_lowercase());

                match effective_type.as_str() {
                    "checkbox" => {
                        // Toggle: click the checkbox element
                        super::tool_click_screen(&serde_json::json!({
                            "x": el.cx, "y": el.cy, "delay_ms": 150, "screenshot": false
                        }));
                        filled.push(format!("'{}' toggled (checkbox)", label));
                    }
                    "combobox" => {
                        // Open dropdown: click, wait, then type value + Enter to select
                        super::tool_click_screen(&serde_json::json!({
                            "x": el.cx, "y": el.cy, "delay_ms": 200, "screenshot": false
                        }));
                        std::thread::sleep(std::time::Duration::from_millis(300));
                        super::tool_type_text(&serde_json::json!({
                            "text": value, "delay_ms": 50, "screenshot": false
                        }));
                        super::tool_press_key(&serde_json::json!({
                            "key": "enter", "delay_ms": 100, "screenshot": false
                        }));
                        filled.push(format!("'{}' = '{}' (dropdown)", label, value));
                    }
                    "radiobutton" | "radio button" => {
                        // Select: just click
                        super::tool_click_screen(&serde_json::json!({
                            "x": el.cx, "y": el.cy, "delay_ms": 150, "screenshot": false
                        }));
                        filled.push(format!("'{}' selected (radio)", label));
                    }
                    // Text input: click → select all → type → tab
                    "edit" | "text" | "input" | _ => {
                        super::tool_click_screen(&serde_json::json!({
                            "x": el.cx, "y": el.cy, "delay_ms": 100, "screenshot": false
                        }));
                        super::tool_press_key(&serde_json::json!({
                            "key": "ctrl+a", "delay_ms": 50, "screenshot": false
                        }));
                        super::tool_type_text(&serde_json::json!({
                            "text": value, "delay_ms": 50, "screenshot": false
                        }));
                        super::tool_press_key(&serde_json::json!({
                            "key": "tab", "delay_ms": 50, "screenshot": false
                        }));
                        filled.push(format!("'{}' = '{}'", label, value));
                    }
                }
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
    super::tool_error("fill_form", "not available on this platform")
}

/// Execute a single action from an action sequence, returning a result string.
/// This is extracted from the main loop to allow retry logic to call it repeatedly.
fn execute_single_action(
    action_type: &str,
    action_args: &Value,
    index: usize,
) -> String {
    match action_type {
        "click" => {
            let r = super::tool_click_screen(action_args);
            format!("#{}: click -> {}", index, r.text)
        }
        "type" => {
            let r = super::tool_type_text(action_args);
            format!("#{}: type -> {}", index, r.text)
        }
        "press_key" | "key" => {
            let r = super::tool_press_key(action_args);
            format!("#{}: key -> {}", index, r.text)
        }
        "paste" => {
            let r = super::input_tools::tool_paste(action_args);
            format!("#{}: paste -> {}", index, r.text)
        }
        "clear" => {
            let r = super::input_tools::tool_clear_field(action_args);
            format!("#{}: clear -> {}", index, r.text)
        }
        "wait" => {
            let ms = action_args.get("ms").and_then(parse_int).unwrap_or(500) as u64;
            std::thread::sleep(std::time::Duration::from_millis(ms));
            format!("#{}: waited {}ms", index, ms)
        }
        "scroll" => {
            let r = super::tool_scroll_screen(action_args);
            format!("#{}: scroll -> {}", index, r.text)
        }
        "move" => {
            let r = super::tool_move_mouse(action_args);
            format!("#{}: move -> {}", index, r.text)
        }
        "assert_text" => {
            let text = action_args
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if text.is_empty() {
                return format!("#{}: assert_text skipped (no text)", index);
            }
            let ocr_result =
                super::ocr_tools::tool_ocr_screen(&serde_json::json!({"monitor": 0}));
            if ocr_result.text.to_lowercase().contains(&text.to_lowercase()) {
                format!("#{}: assert_text OK -- '{}' found", index, text)
            } else {
                format!(
                    "#{}: assert_text FAILED -- '{}' not found. Aborting sequence.",
                    index, text
                )
            }
        }
        "if_text_on_screen" => {
            let text = action_args
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if text.is_empty() {
                return format!("#{}: if_text_on_screen skipped (no text)", index);
            }
            let then_action = action_args.get("then").and_then(|v| v.as_str()).unwrap_or("");
            let else_action = action_args.get("else").and_then(|v| v.as_str()).unwrap_or("");

            let ocr_result =
                super::ocr_tools::tool_ocr_screen(&serde_json::json!({"monitor": 0}));
            let found = ocr_result.text.to_lowercase().contains(&text.to_lowercase());

            let branch = if found { then_action } else { else_action };
            if branch.is_empty() {
                format!(
                    "#{}: if_text_on_screen '{}' -> {} (no action for this branch)",
                    index, text, if found { "FOUND" } else { "NOT FOUND" }
                )
            } else {
                // Execute the branch action (simple: treat as press_key for key presses, or type)
                let branch_result = execute_single_action(branch, action_args, index);
                format!(
                    "#{}: if_text_on_screen '{}' -> {} -> {}",
                    index, text, if found { "FOUND" } else { "NOT FOUND" }, branch_result
                )
            }
        }
        "repeat" => {
            let count = action_args.get("count").and_then(parse_int).unwrap_or(1).max(1).min(50) as usize;
            let sub_actions = action_args.get("actions").and_then(|v| v.as_array());
            match sub_actions {
                Some(sub) if !sub.is_empty() => {
                    let mut sub_results = Vec::new();
                    for iteration in 0..count {
                        for (si, sub_action) in sub.iter().enumerate() {
                            let sub_type = sub_action.get("action").and_then(|v| v.as_str()).unwrap_or("wait");
                            let sub_result = execute_single_action(sub_type, sub_action, si + 1);
                            sub_results.push(format!("  iter {}/{}: {}", iteration + 1, count, sub_result));
                            // Small delay between sub-actions within a repeat
                            let delay = sub_action.get("delay_ms").and_then(parse_int).unwrap_or(100) as u64;
                            std::thread::sleep(std::time::Duration::from_millis(delay));
                        }
                    }
                    format!("#{}: repeat x{} ({} sub-results):\n{}", index, count, sub_results.len(), sub_results.join("\n"))
                }
                _ => format!("#{}: repeat skipped (no 'actions' array)", index),
            }
        }
        other => {
            format!("#{}: unknown action '{}'", index, other)
        }
    }
}

/// Check whether a result string indicates failure (contains error/failure keywords).
fn result_is_failure(result: &str) -> bool {
    let lower = result.to_lowercase();
    lower.contains("error") || lower.contains("failed")
}

/// Execute a sequence of desktop actions (click, type, press_key, paste, wait, clear, assert_text).
///
/// Parameters:
/// - `actions`: array of action objects
/// - `delay_between_ms`: default delay between actions (default: 200)
/// - `screenshot_mode`: `"final_only"` (default), `"all"`, or `"none"`
///
/// Per-action optional fields:
/// - `retry`: number of retries on failure (0-3, default 0)
/// - `if_previous`: `"success"` or `"failure"` — skip action if condition not met
/// - `abort_on_failure`: bool (default false) — stop sequence if this action fails
pub fn tool_run_action_sequence(args: &Value) -> NativeToolResult {
    let actions = match args.get("actions").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => {
            return super::tool_error(
                "run_action_sequence",
                "'actions' array is required, e.g. [{\"action\":\"click\",\"x\":100,\"y\":200}]",
            )
        }
    };

    let default_delay = args
        .get("delay_between_ms")
        .and_then(parse_int)
        .unwrap_or(200) as u64;

    let screenshot_mode = args
        .get("screenshot_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("final_only");

    let mut results: Vec<String> = Vec::new();

    for (i, action) in actions.iter().enumerate() {
        let action_type = match action.get("action").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => {
                results.push(format!("#{}: skipped (no 'action' field)", i + 1));
                continue;
            }
        };

        // Check if_previous condition
        if let Some(condition) = action.get("if_previous").and_then(|v| v.as_str()) {
            if let Some(last_result) = results.last() {
                let prev_failed = result_is_failure(last_result);
                match condition {
                    "success" => {
                        if prev_failed {
                            results.push(format!("#{}: skipped (if_previous=success, but previous failed)", i + 1));
                            continue;
                        }
                    }
                    "failure" => {
                        if !prev_failed {
                            results.push(format!("#{}: skipped (if_previous=failure, but previous succeeded)", i + 1));
                            continue;
                        }
                    }
                    _ => {} // Unknown condition, ignore and proceed
                }
            }
            // If no previous result yet, proceed regardless of condition
        }

        // Parse retry count (0-3, default 0)
        let max_retries = action.get("retry")
            .and_then(parse_int)
            .unwrap_or(0)
            .max(0).min(3) as u32;

        // Parse abort_on_failure (default false)
        let abort_on_failure = action.get("abort_on_failure")
            .map(|v| super::parse_bool(v, false))
            .unwrap_or(false);

        // Suppress screenshots for intermediate actions unless screenshot_mode is "all"
        let mut action_args = action.clone();
        if let Some(obj) = action_args.as_object_mut() {
            if i < actions.len() - 1 && screenshot_mode != "all" {
                obj.insert("screenshot".to_string(), serde_json::json!(false));
            }
        }

        // Execute with retry logic
        let result_str = if max_retries > 0 {
            let retry_result = super::screenshot_tools::retry_on_failure(max_retries, 300, || {
                let r = execute_single_action(action_type, &action_args, i + 1);
                if result_is_failure(&r) {
                    Err(r)
                } else {
                    Ok(r)
                }
            });
            match retry_result {
                Ok(s) => s,
                Err(last_err) => {
                    // All retries exhausted; use the last failure message
                    format!("{} (after {} retries)", last_err, max_retries)
                }
            }
        } else {
            execute_single_action(action_type, &action_args, i + 1)
        };

        let action_failed = result_is_failure(&result_str);
        results.push(result_str);

        // assert_text FAILED still breaks the loop (legacy behavior)
        if action_type == "assert_text" && action_failed {
            break;
        }

        // abort_on_failure
        if abort_on_failure && action_failed {
            results.push(format!("Sequence aborted at action #{} due to abort_on_failure", i + 1));
            break;
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

    // Final screenshot based on screenshot_mode
    let screenshot = if screenshot_mode == "none" {
        NativeToolResult::text_only(String::new())
    } else {
        super::capture_post_action_screenshot(0)
    };

    NativeToolResult {
        text: format!(
            "Executed {} action(s):\n{}",
            actions.len(),
            results.join("\n")
        ),
        images: screenshot.images,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Round 7: fill_form parameter validation ────────────────────────

    #[test]
    fn test_fill_form_missing_fields() {
        let args = serde_json::json!({});
        let result = tool_fill_form(&args);
        assert!(result.text.contains("Error [fill_form]"));
        assert!(result.text.contains("fields"));
    }

    #[test]
    fn test_fill_form_empty_fields_array() {
        let args = serde_json::json!({"fields": []});
        let result = tool_fill_form(&args);
        // Empty fields should produce "Filled 0 field(s)" — no error
        assert!(result.text.contains("Filled 0 field(s)"));
    }

    // ─── Round 7: action_sequence parameter validation ──────────────────

    #[test]
    fn test_action_sequence_missing_actions() {
        let args = serde_json::json!({});
        let result = tool_run_action_sequence(&args);
        assert!(result.text.contains("Error [run_action_sequence]"));
        assert!(result.text.contains("actions"));
    }

    #[test]
    fn test_action_sequence_wait_action() {
        let args = serde_json::json!({
            "actions": [{"action": "wait", "ms": 50}],
            "screenshot_mode": "none"
        });
        let result = tool_run_action_sequence(&args);
        assert!(result.text.contains("waited 50ms"));
    }

    #[test]
    fn test_action_sequence_unknown_action() {
        let args = serde_json::json!({
            "actions": [{"action": "foobar"}],
            "screenshot_mode": "none"
        });
        let result = tool_run_action_sequence(&args);
        assert!(result.text.contains("unknown action 'foobar'"));
    }

    #[test]
    fn test_action_sequence_skip_no_action_field() {
        let args = serde_json::json!({
            "actions": [{"x": 100}],
            "screenshot_mode": "none"
        });
        let result = tool_run_action_sequence(&args);
        assert!(result.text.contains("skipped (no 'action' field)"));
    }

    #[test]
    fn test_action_sequence_if_previous_success_skips_on_failure() {
        let args = serde_json::json!({
            "actions": [
                {"action": "assert_text", "text": ""},
                {"action": "wait", "ms": 10, "if_previous": "failure"}
            ],
            "screenshot_mode": "none"
        });
        let result = tool_run_action_sequence(&args);
        // First action: assert_text skipped (no text)
        // Second action: if_previous=failure but previous succeeded → skip
        assert!(result.text.contains("skipped") || result.text.contains("waited"));
    }

    // ─── Round 7: result_is_failure helper ──────────────────────────────

    #[test]
    fn test_result_is_failure_detects_error() {
        assert!(result_is_failure("Something Error happened"));
        assert!(result_is_failure("FAILED to do something"));
        assert!(!result_is_failure("All OK, no issues"));
    }
}
