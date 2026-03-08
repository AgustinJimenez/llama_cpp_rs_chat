//! Compound desktop tools that combine multiple primitives into single actions.

use serde_json::Value;

use super::NativeToolResult;
use super::parse_int;

#[cfg(windows)]
use super::win32;
#[cfg(target_os = "macos")]
use super::macos as win32;
#[cfg(target_os = "linux")]
use super::linux as win32;

// ─── find_and_click_text ────────────────────────────────────────────────────

/// OCR the screen → find text → click its center → return screenshot.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_find_and_click_text(args: &Value) -> NativeToolResult {
    let search_text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return NativeToolResult::text_only("Error: 'text' is required".to_string()),
    };
    let delay_ms = args.get("delay_ms").and_then(parse_int).unwrap_or(500) as u64;
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;
    let index = args.get("index").and_then(parse_int).unwrap_or(0) as usize;

    // Capture screen
    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
    };
    if monitor_idx >= monitors.len() {
        return NativeToolResult::text_only(format!("Error: monitor {monitor_idx} out of range"));
    }
    let img = match monitors[monitor_idx].capture_image() {
        Ok(i) => i,
        Err(e) => return NativeToolResult::text_only(format!("Error capturing: {e}")),
    };

    // OCR find text on STA thread (with retry on transient failures)
    let search = search_text.to_lowercase();
    let mut result = Err("OCR not attempted".to_string());
    for attempt in 0..3u32 {
        let img_c = img.clone();
        let s = search.clone();
        result = std::thread::spawn(move || {
            super::ocr_find_text_winrt(&img_c, &s, 0.0, 0.0)
        }).join().unwrap_or_else(|_| Err("OCR thread panicked".to_string()));
        if result.is_ok() { break; }
        if attempt < 2 {
            std::thread::sleep(std::time::Duration::from_millis(200));
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
            let mut result = super::tool_click_screen(&click_args);
            let idx_info = if index > 0 { format!(" (index {index})") } else { String::new() };
            result.text = format!(
                "Found \"{}\" and clicked at ({:.0}, {:.0}){idx_info}. {}",
                m.text, m.center_x, m.center_y, result.text
            );
            result
        }
        Err(e) => NativeToolResult::text_only(format!("OCR error: {e}")),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_find_and_click_text(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: find_and_click_text is not available on this platform".to_string())
}

// ─── type_into_element ──────────────────────────────────────────────────────

/// Find a UI element → click to focus → type text into it.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_type_into_element(args: &Value) -> NativeToolResult {
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return NativeToolResult::text_only("Error: 'text' is required".to_string()),
    };
    let name_filter = args.get("name").and_then(|v| v.as_str());
    let type_filter = args.get("control_type").and_then(|v| v.as_str());

    if name_filter.is_none() && type_filter.is_none() {
        return NativeToolResult::text_only("Error: at least 'name' or 'control_type' is required".to_string());
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

    if let Some(r) = super::ui_tools::check_gpu_app_guard(hwnd, "type_into_element") { return r; }

    let name_owned = name_filter.map(|s| s.to_lowercase());
    let type_owned = type_filter.map(|s| s.to_lowercase());

    // Find the element on STA thread
    let result = std::thread::spawn(move || {
        super::find_ui_element(hwnd, name_owned.as_deref(), type_owned.as_deref())
    }).join().unwrap_or_else(|_| Err("UI thread panicked".to_string()));

    match result {
        Ok(info) => {
            let desc = info.desc();
            // Click to focus
            let click_args = serde_json::json!({ "x": info.cx, "y": info.cy, "button": "left", "delay_ms": 200 });
            super::tool_click_screen(&click_args);
            // Type text
            let type_args = serde_json::json!({ "text": text, "delay_ms": 300 });
            let mut result = super::tool_type_text(&type_args);
            result.text = format!("Clicked element {desc} at ({}, {}), then typed {} chars. {}",
                info.cx, info.cy, text.len(), result.text);
            result
        }
        Err(e) => NativeToolResult::text_only(format!("Error: {e}")),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_type_into_element(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: type_into_element is not available on this platform".to_string())
}

// ─── get_window_text ────────────────────────────────────────────────────────

/// Extract all text from a window via UI Automation tree walk.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_get_window_text(args: &Value) -> NativeToolResult {
    let title_filter = args.get("title").and_then(|v| v.as_str());
    let max_chars = args.get("max_chars").and_then(parse_int).unwrap_or(50000) as usize;

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

    let result = std::thread::spawn(move || {
        get_window_text_inner(hwnd, max_chars)
    }).join().unwrap_or_else(|_| Err("UI thread panicked".to_string()));

    match result {
        Ok(text) => NativeToolResult::text_only(text),
        Err(e) => NativeToolResult::text_only(format!("Error: {e}")),
    }
}

#[cfg(windows)]
fn get_window_text_inner(hwnd: isize, max_chars: usize) -> Result<String, String> {
    use windows::Win32::UI::Accessibility::*;
    use windows::Win32::System::Com::{CoInitializeEx, CoCreateInstance, COINIT_APARTMENTTHREADED, CLSCTX_INPROC_SERVER};
    use windows::Win32::Foundation::HWND as WIN32_HWND;
    use windows::core::HRESULT;

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() && hr != HRESULT(1) {
            return Err(format!("CoInitializeEx failed: {hr:?}"));
        }
    }

    let automation: IUIAutomation = unsafe {
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
    }.map_err(|e| format!("CoCreateInstance: {e}"))?;

    let root = unsafe {
        automation.ElementFromHandle(WIN32_HWND(hwnd as *mut _))
    }.map_err(|e| format!("ElementFromHandle: {e}"))?;

    let walker = unsafe {
        automation.ControlViewWalker()
    }.map_err(|e| format!("ControlViewWalker: {e}"))?;

    let mut output = String::new();
    collect_text(&walker, &root, &mut output, max_chars, 0);
    if output.is_empty() {
        Ok("No text found in window".to_string())
    } else {
        Ok(output)
    }
}

#[cfg(windows)]
fn collect_text(
    walker: &windows::Win32::UI::Accessibility::IUIAutomationTreeWalker,
    parent: &windows::Win32::UI::Accessibility::IUIAutomationElement,
    output: &mut String,
    max_chars: usize,
    depth: usize,
) {
    if depth > 10 || output.len() >= max_chars {
        return;
    }

    let name = unsafe { parent.CurrentName() }.map(|s| s.to_string()).unwrap_or_default();
    let control_type = unsafe { parent.CurrentControlType() }
        .map(|ct| super::control_type_name(ct.0))
        .unwrap_or_default();

    // Collect text from text-like elements
    if !name.is_empty() {
        match control_type.as_str() {
            "Text" | "Edit" | "Document" | "Hyperlink" => {
                if output.len() + name.len() + 1 <= max_chars {
                    output.push_str(&name);
                    output.push('\n');
                }
            }
            _ => {
                // Try ValuePattern for editable fields
                let value_result: Result<windows::Win32::UI::Accessibility::IUIAutomationValuePattern, _> = unsafe {
                    parent.GetCurrentPatternAs(windows::Win32::UI::Accessibility::UIA_ValuePatternId)
                };
                if let Ok(pattern) = value_result {
                    if let Ok(val) = unsafe { pattern.CurrentValue() } {
                        let val_str = val.to_string();
                        if !val_str.is_empty() && output.len() + val_str.len() + 1 <= max_chars {
                            output.push_str(&val_str);
                            output.push('\n');
                        }
                    }
                }
            }
        }
    }

    let first_child = unsafe { walker.GetFirstChildElement(parent) };
    let mut current = match first_child {
        Ok(c) => c,
        Err(_) => return,
    };
    loop {
        if output.len() >= max_chars { return; }
        collect_text(walker, &current, output, max_chars, depth + 1);
        match unsafe { walker.GetNextSiblingElement(&current) } {
            Ok(next) => current = next,
            Err(_) => break,
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_get_window_text(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: get_window_text is not available on this platform".to_string())
}

// ─── file_dialog_navigate ───────────────────────────────────────────────────

/// Navigate a file Open/Save dialog: set filename field and click Open/Save.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_file_dialog_navigate(args: &Value) -> NativeToolResult {
    let filename = match args.get("filename").and_then(|v| v.as_str()) {
        Some(f) => f.to_string(),
        None => return NativeToolResult::text_only("Error: 'filename' is required".to_string()),
    };
    let button_name = args.get("button").and_then(|v| v.as_str()).unwrap_or("Open").to_string();
    let title_filter = args.get("title").and_then(|v| v.as_str());

    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return NativeToolResult::text_only(format!("No window matches '{filter}'")),
        }
    } else {
        // Try to find a common dialog window
        win32::find_window_by_filter("Open")
            .or_else(|| win32::find_window_by_filter("Save"))
            .or_else(|| win32::find_window_by_filter("Browse"))
            .map(|(h, _)| h)
            .unwrap_or_else(|| win32::get_active_window_info().map(|(h, _)| h).unwrap_or(0))
    };

    if hwnd == 0 {
        return NativeToolResult::text_only("Error: no file dialog window found".to_string());
    }

    let filename_clone = filename.clone();
    let button_clone = button_name.clone();

    let result = std::thread::spawn(move || {
        file_dialog_navigate_inner(hwnd, &filename_clone, &button_clone)
    }).join().unwrap_or_else(|_| Err("UI thread panicked".to_string()));

    match result {
        Ok(msg) => NativeToolResult::text_only(msg),
        Err(e) => NativeToolResult::text_only(format!("Error: {e}")),
    }
}

#[cfg(windows)]
fn file_dialog_navigate_inner(hwnd: isize, filename: &str, button_name: &str) -> Result<String, String> {
    use windows::Win32::UI::Accessibility::*;
    use windows::Win32::System::Com::{CoInitializeEx, CoCreateInstance, COINIT_APARTMENTTHREADED, CLSCTX_INPROC_SERVER};
    use windows::Win32::Foundation::HWND as WIN32_HWND;
    use windows::core::{HRESULT, BSTR};

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() && hr != HRESULT(1) {
            return Err(format!("CoInitializeEx failed: {hr:?}"));
        }
    }

    let automation: IUIAutomation = unsafe {
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
    }.map_err(|e| format!("CoCreateInstance: {e}"))?;

    let root = unsafe {
        automation.ElementFromHandle(WIN32_HWND(hwnd as *mut _))
    }.map_err(|e| format!("ElementFromHandle: {e}"))?;

    let walker = unsafe {
        automation.ControlViewWalker()
    }.map_err(|e| format!("ControlViewWalker: {e}"))?;

    // Find the filename edit field (usually named "File name:" or similar Edit control)
    let edit_elem = super::find_raw_ui_element(&walker, &root, Some("file name"), Some("edit"), 0)
        .or_else(|| super::find_raw_ui_element(&walker, &root, None, Some("edit"), 0))
        .ok_or("Could not find filename edit field")?;

    // Set the value
    let pattern: IUIAutomationValuePattern = unsafe {
        edit_elem.GetCurrentPatternAs(UIA_ValuePatternId)
    }.map_err(|e| format!("Edit field doesn't support ValuePattern: {e}"))?;
    let bstr = BSTR::from(filename);
    unsafe { pattern.SetValue(&bstr) }.map_err(|e| format!("SetValue failed: {e}"))?;

    // Small delay for UI to update
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Find and click the Open/Save button
    let button_search = button_name.to_lowercase();
    let button_elem = super::find_raw_ui_element(&walker, &root, Some(&button_search), Some("button"), 0)
        .ok_or_else(|| format!("Could not find '{button_name}' button"))?;

    let invoke: IUIAutomationInvokePattern = unsafe {
        button_elem.GetCurrentPatternAs(UIA_InvokePatternId)
    }.map_err(|e| format!("Button doesn't support Invoke: {e}"))?;
    unsafe { invoke.Invoke() }.map_err(|e| format!("Invoke failed: {e}"))?;

    Ok(format!("Set filename to '{}' and clicked '{}'", filename, button_name))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_file_dialog_navigate(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: file_dialog_navigate is not available on this platform".to_string())
}

// ─── drag_and_drop_element ──────────────────────────────────────────────────

/// Find two UI elements and drag from one to the other.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_drag_and_drop_element(args: &Value) -> NativeToolResult {
    let from_name = args.get("from_name").and_then(|v| v.as_str());
    let to_name = args.get("to_name").and_then(|v| v.as_str());
    let from_type = args.get("from_type").and_then(|v| v.as_str());
    let to_type = args.get("to_type").and_then(|v| v.as_str());

    if from_name.is_none() && from_type.is_none() {
        return NativeToolResult::text_only("Error: at least 'from_name' or 'from_type' is required".to_string());
    }
    if to_name.is_none() && to_type.is_none() {
        return NativeToolResult::text_only("Error: at least 'to_name' or 'to_type' is required".to_string());
    }

    let title_filter = args.get("title").and_then(|v| v.as_str());
    let delay_ms = args.get("delay_ms").and_then(parse_int).unwrap_or(500) as u64;

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

    if let Some(r) = super::ui_tools::check_gpu_app_guard(hwnd, "drag_and_drop_element") { return r; }

    let fn_owned = from_name.map(|s| s.to_lowercase());
    let ft_owned = from_type.map(|s| s.to_lowercase());
    let tn_owned = to_name.map(|s| s.to_lowercase());
    let tt_owned = to_type.map(|s| s.to_lowercase());

    let result = std::thread::spawn(move || {
        let from = super::find_ui_element(hwnd, fn_owned.as_deref(), ft_owned.as_deref())?;
        let to = super::find_ui_element(hwnd, tn_owned.as_deref(), tt_owned.as_deref())?;
        Ok((from, to))
    }).join().unwrap_or_else(|_| Err("UI thread panicked".to_string()));

    match result {
        Ok((from, to)) => {
            let drag_args = serde_json::json!({
                "start_x": from.cx,
                "start_y": from.cy,
                "end_x": to.cx,
                "end_y": to.cy,
                "delay_ms": delay_ms,
            });
            let mut result = super::tool_mouse_drag(&drag_args);
            result.text = format!(
                "Dragged {} at ({},{}) → {} at ({},{}). {}",
                from.desc(), from.cx, from.cy,
                to.desc(), to.cx, to.cy,
                result.text
            );
            result
        }
        Err(e) => NativeToolResult::text_only(format!("Error: {e}")),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_drag_and_drop_element(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: drag_and_drop_element is not available on this platform".to_string())
}

// ─── wait_for_text_on_screen ────────────────────────────────────────────────

/// Poll OCR until specified text appears on screen.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_wait_for_text_on_screen(args: &Value) -> NativeToolResult {
    let search_text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return NativeToolResult::text_only("Error: 'text' is required".to_string()),
    };
    let timeout_ms = args.get("timeout_ms").and_then(parse_int).unwrap_or(10000).min(30000) as u64;
    let poll_ms = args.get("poll_ms").and_then(parse_int).unwrap_or(1000).max(500) as u64;
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    let start = std::time::Instant::now();
    let search = search_text.to_lowercase();
    let base_poll = poll_ms;
    let mut attempt = 0u32;

    loop {
        let monitors = match xcap::Monitor::all() {
            Ok(m) => m,
            Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
        };
        if monitor_idx >= monitors.len() {
            return NativeToolResult::text_only(format!("Error: monitor {monitor_idx} out of range"));
        }
        let img = match monitors[monitor_idx].capture_image() {
            Ok(i) => i,
            Err(e) => return NativeToolResult::text_only(format!("Error capturing: {e}")),
        };

        let s = search.clone();
        let result = std::thread::spawn(move || {
            super::ocr_find_text_winrt(&img, &s, 0.0, 0.0)
        }).join().unwrap_or_else(|_| Err("OCR thread panicked".to_string()));

        if let Ok(matches) = result {
            if !matches.is_empty() {
                let m = &matches[0];
                return NativeToolResult::text_only(format!(
                    "Text '{}' found at ({:.0}, {:.0}) after {}ms",
                    m.text, m.center_x, m.center_y, start.elapsed().as_millis()
                ));
            }
        }

        if start.elapsed().as_millis() >= timeout_ms as u128 {
            return NativeToolResult::text_only(format!(
                "Timeout: text '{}' not found after {timeout_ms}ms", search_text
            ));
        }

        let adaptive_delay = super::adaptive_poll_ms(attempt, base_poll, base_poll * 4);
        std::thread::sleep(std::time::Duration::from_millis(adaptive_delay));
        attempt += 1;
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_wait_for_text_on_screen(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: wait_for_text_on_screen is not available on this platform".to_string())
}

// ─── get_context_menu ───────────────────────────────────────────────────────

/// Right-click to open context menu, read items, optionally click one.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_get_context_menu(args: &Value) -> NativeToolResult {
    let x = match args.get("x").and_then(parse_int) {
        Some(v) => v as i32,
        None => return NativeToolResult::text_only("Error: 'x' is required".to_string()),
    };
    let y = match args.get("y").and_then(parse_int) {
        Some(v) => v as i32,
        None => return NativeToolResult::text_only("Error: 'y' is required".to_string()),
    };
    let click_item = args.get("click_item").and_then(|v| v.as_str()).map(|s| s.to_string());
    let delay_ms = args.get("delay_ms").and_then(parse_int).unwrap_or(500) as u64;

    // Right-click to open the menu
    let click_args = serde_json::json!({ "x": x, "y": y, "button": "right", "delay_ms": delay_ms });
    super::tool_click_screen(&click_args);

    // Wait a bit for menu to appear
    std::thread::sleep(std::time::Duration::from_millis(300));

    // Find menu items in the foreground window
    let hwnd = match win32::get_active_window_info() {
        Some((h, _)) => h,
        None => return NativeToolResult::text_only("Right-clicked but no active window found".to_string()),
    };

    let click_item_clone = click_item.clone();
    let result = std::thread::spawn(move || {
        let items = super::find_ui_elements_all(hwnd, None, Some("menuitem"), 20)?;
        if items.is_empty() {
            // Fallback: try looking for menu items in any menu-type
            let items2 = super::find_ui_elements_all(hwnd, None, Some("menu"), 20)?;
            if items2.is_empty() {
                return Err("No menu items found".to_string());
            }
            return Ok((items2, None));
        }

        // If click_item is specified, find and return its info
        let to_click = if let Some(ref target) = click_item_clone {
            let target_lower = target.to_lowercase();
            items.iter().find(|i| i.name.to_lowercase().contains(&target_lower))
                .map(|i| (i.cx, i.cy, i.desc()))
        } else {
            None
        };

        Ok((items, to_click))
    }).join().unwrap_or_else(|_| Err("UI thread panicked".to_string()));

    match result {
        Ok((items, to_click)) => {
            let menu_text: Vec<String> = items.iter().enumerate().map(|(i, e)| {
                format!("{}. {}", i + 1, e.desc())
            }).collect();
            let menu_list = format!("Context menu ({} items):\n{}", items.len(), menu_text.join("\n"));

            if let Some((cx, cy, desc)) = to_click {
                let click_args = serde_json::json!({ "x": cx, "y": cy, "button": "left", "delay_ms": 300 });
                let mut result = super::tool_click_screen(&click_args);
                result.text = format!("{menu_list}\nClicked: {desc} at ({cx}, {cy}). {}", result.text);
                result
            } else if click_item.is_some() {
                NativeToolResult::text_only(format!("{menu_list}\nNote: item '{}' not found in menu", click_item.unwrap()))
            } else {
                NativeToolResult::text_only(menu_list)
            }
        }
        Err(e) => NativeToolResult::text_only(format!("Error reading context menu: {e}")),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_get_context_menu(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: get_context_menu is not available on this platform".to_string())
}

// ─── scroll_element ─────────────────────────────────────────────────────────

/// Find a UI element and scroll it (via ScrollPattern or mouse wheel fallback).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_scroll_element(args: &Value) -> NativeToolResult {
    let name_filter = args.get("name").and_then(|v| v.as_str());
    let type_filter = args.get("control_type").and_then(|v| v.as_str());
    let direction = args.get("direction").and_then(|v| v.as_str()).unwrap_or("down");
    let amount = args.get("amount").and_then(parse_int).unwrap_or(3) as i32;

    if name_filter.is_none() && type_filter.is_none() {
        return NativeToolResult::text_only("Error: at least 'name' or 'control_type' is required".to_string());
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

    if let Some(r) = super::ui_tools::check_gpu_app_guard(hwnd, "scroll_element") { return r; }

    let name_owned = name_filter.map(|s| s.to_lowercase());
    let type_owned = type_filter.map(|s| s.to_lowercase());
    let dir = direction.to_lowercase();

    let result = std::thread::spawn(move || {
        let info = super::find_ui_element(hwnd, name_owned.as_deref(), type_owned.as_deref())?;
        Ok(info)
    }).join().unwrap_or_else(|_| Err("UI thread panicked".to_string()));

    match result {
        Ok(info) => {
            let desc = info.desc();
            // Use mouse wheel at element center as fallback (more reliable than ScrollPattern)
            let scroll_args = serde_json::json!({
                "x": info.cx,
                "y": info.cy,
                "direction": dir,
                "clicks": amount,
                "delay_ms": 300,
            });
            let mut result = super::tool_scroll_screen(&scroll_args);
            result.text = format!("Scrolled {dir} {amount} clicks on element {desc} at ({}, {}). {}",
                info.cx, info.cy, result.text);
            result
        }
        Err(e) => NativeToolResult::text_only(format!("Error: {e}")),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_scroll_element(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: scroll_element is not available on this platform".to_string())
}
