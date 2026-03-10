//! Compound desktop tools that combine multiple primitives into single actions.

use serde_json::Value;

use super::NativeToolResult;
use super::parse_int;
use super::gpu_app_db;

#[cfg(windows)]
use super::win32;
#[cfg(target_os = "macos")]
use super::macos as win32;
#[cfg(target_os = "linux")]
use super::linux as win32;

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn gpu_app_for_hwnd(hwnd: win32::HWND) -> Option<&'static gpu_app_db::GpuAppInfo> {
    let info = win32::get_window_info_for_hwnd(hwnd)?;
    gpu_app_db::detect_gpu_app(&info.class_name, &info.process_name)
}

// ─── find_and_click_text ────────────────────────────────────────────────────

/// OCR the screen → find text → click its center → return screenshot.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_find_and_click_text(args: &Value) -> NativeToolResult {
    let search_text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return super::tool_error("find_and_click_text", "'text' is required"),
    };
    let delay_ms = args.get("delay_ms").and_then(parse_int).unwrap_or(500) as u64;
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;
    let index = args.get("index").and_then(parse_int).unwrap_or(0) as usize;

    // Capture screen
    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return super::tool_error("find_and_click_text", e),
    };
    if monitor_idx >= monitors.len() {
        return super::tool_error("find_and_click_text", format!("monitor {monitor_idx} out of range"));
    }
    let img = match monitors[monitor_idx].capture_image() {
        Ok(i) => i,
        Err(e) => return super::tool_error("find_and_click_text", format!("capturing: {e}")),
    };

    // OCR find text on STA thread (with retry on transient failures)
    let search = search_text.to_lowercase();
    let mut result = Err("OCR not attempted".to_string());
    for attempt in 0..3u32 {
        if let Err(e) = super::ensure_desktop_not_cancelled() {
            return super::tool_error("find_and_click_text", e);
        }

        let img_c = img.clone();
        let s = search.clone();
        result = super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || {
            super::ocr_tools::ocr_find_text(&img_c, &s, 0.0, 0.0)
        }).and_then(|r| r);
        if result.is_ok() { break; }
        if attempt < 2 {
            if let Err(e) = super::interruptible_sleep(std::time::Duration::from_millis(200)) {
                return super::tool_error("find_and_click_text", e);
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
            let mut result = super::tool_click_screen(&click_args);
            let idx_info = if index > 0 { format!(" (index {index})") } else { String::new() };
            result.text = format!(
                "Found \"{}\" and clicked at ({:.0}, {:.0}){idx_info}. {}",
                m.text, m.center_x, m.center_y, result.text
            );
            result
        }
        Err(e) => super::tool_error("find_and_click_text", format!("OCR: {e}")),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_find_and_click_text(_args: &Value) -> NativeToolResult {
    super::tool_error("find_and_click_text", "not available on this platform")
}

// ─── type_into_element ──────────────────────────────────────────────────────

/// Find a UI element → click to focus → type text into it.
/// Windows: UI Automation element search. macOS/Linux: OCR fallback.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_type_into_element(args: &Value) -> NativeToolResult {
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return super::tool_error("type_into_element", "'text' is required"),
    };
    let name_filter = args.get("name").and_then(|v| v.as_str());
    let type_filter = args.get("control_type").and_then(|v| v.as_str());

    if name_filter.is_none() && type_filter.is_none() {
        return super::tool_error("type_into_element", "at least 'name' or 'control_type' is required");
    }

    let title_filter = args.get("title").and_then(|v| v.as_str());
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return super::tool_error("type_into_element", format!("no window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return super::tool_error("type_into_element", "no active window"),
        }
    };

    let prefer_ocr_fallback = gpu_app_for_hwnd(hwnd).is_some() && name_filter.is_some();

    // --- Windows: UI Automation path ---
    #[cfg(windows)]
    let element_pos: Result<(i32, i32, String), String> = {
        if prefer_ocr_fallback {
            let search_name = name_filter.unwrap().to_lowercase();
            ocr_find_element_on_screen(&search_name)
        } else {
            let name_owned = name_filter.map(|s| s.to_lowercase());
            let type_owned = type_filter.map(|s| s.to_lowercase());

            let result = super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || {
                super::find_ui_element(hwnd, name_owned.as_deref(), type_owned.as_deref())
            }).and_then(|r| r);

            result.map(|info| (info.cx, info.cy, info.desc()))
        }
    };

    // --- macOS/Linux: OCR fallback path ---
    #[cfg(not(windows))]
    let element_pos: Result<(i32, i32, String), String> = {
        let search_name = match name_filter {
            Some(n) => n.to_lowercase(),
            None => type_filter.unwrap_or("").to_lowercase(),
        };
        ocr_find_element_on_screen(&search_name)
    };

    match element_pos {
        Ok((cx, cy, desc)) => {
            // Click to focus
            let click_args = serde_json::json!({ "x": cx, "y": cy, "button": "left", "delay_ms": 200 });
            super::tool_click_screen(&click_args);
            // Type text
            let type_args = serde_json::json!({ "text": text, "delay_ms": 300 });
            let mut result = super::tool_type_text(&type_args);
            let route = if prefer_ocr_fallback { "ocr_fallback" } else { "uia" };
            result.text = format!(
                "Clicked element {desc} at ({}, {}) via {route}, then typed {} chars. {}",
                cx, cy, text.len(), result.text
            );
            result
        }
        Err(e) => super::tool_error("type_into_element", e),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_type_into_element(_args: &Value) -> NativeToolResult {
    super::tool_error("type_into_element", "not available on this platform")
}

// ─── get_window_text ────────────────────────────────────────────────────────

/// Extract all text from a window via UI Automation tree walk (Windows) or OCR (macOS/Linux).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_get_window_text(args: &Value) -> NativeToolResult {
    let title_filter = args.get("title").and_then(|v| v.as_str());
    let max_chars = args.get("max_chars").and_then(parse_int).unwrap_or(50000) as usize;

    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return super::tool_error("get_window_text", format!("no window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return super::tool_error("get_window_text", "no active window"),
        }
    };

    let result = super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || {
        get_window_text_inner(hwnd, max_chars)
    }).and_then(|r| r);

    match result {
        Ok(text) => NativeToolResult::text_only(text),
        Err(e) => super::tool_error("get_window_text", e),
    }
}

#[cfg(windows)]
#[allow(unused_imports)]
fn get_window_text_inner(hwnd: isize, max_chars: usize) -> Result<String, String> {
    use windows::Win32::UI::Accessibility::*;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::Foundation::HWND as WIN32_HWND;
    use windows::core::HRESULT;

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() && hr != HRESULT(1) {
            return Err(format!("CoInitializeEx failed: {hr:?}"));
        }
    }

    let automation = super::ui_automation_tools::create_uiautomation_client()?;

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

/// OCR-based fallback for get_window_text on macOS/Linux.
/// Captures the window by title match via xcap, then runs OCR on the image.
#[cfg(not(windows))]
fn get_window_text_inner(_hwnd: isize, max_chars: usize) -> Result<String, String> {
    // Capture the primary monitor as a fallback (window-level capture may not
    // match the hwnd abstraction used on non-Windows platforms)
    let monitors = xcap::Monitor::all().map_err(|e| format!("Monitor list: {e}"))?;
    if monitors.is_empty() {
        return Err("No monitors available".to_string());
    }
    let img = monitors[0].capture_image().map_err(|e| format!("Screen capture: {e}"))?;

    // Use the platform OCR to extract all text from the image
    #[cfg(target_os = "macos")]
    let ocr_result = super::ocr_tools::ocr_image_vision(&img)
        .or_else(|_| super::ocr_tools::ocr_image_tesseract(&img));

    #[cfg(target_os = "linux")]
    let ocr_result = super::ocr_tools::ocr_image_tesseract(&img);

    match ocr_result {
        Ok(text) => {
            if text.is_empty() {
                Ok("No text found in window (via OCR)".to_string())
            } else if text.len() > max_chars {
                Ok(text[..max_chars].to_string())
            } else {
                Ok(text)
            }
        }
        Err(e) => Err(format!("OCR failed: {e}")),
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
    super::tool_error("get_window_text", "not available on this platform")
}

// ─── file_dialog_navigate ───────────────────────────────────────────────────

/// Navigate a file Open/Save dialog: set filename field and click Open/Save.
/// Windows: COM IUIAutomation to set value and invoke button.
/// macOS/Linux: type path into filename field via keyboard, then press Enter.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_file_dialog_navigate(args: &Value) -> NativeToolResult {
    let filename = match args.get("filename").and_then(|v| v.as_str()) {
        Some(f) => f.to_string(),
        None => return super::tool_error("file_dialog_navigate", "'filename' is required"),
    };
    let button_name = args.get("button").and_then(|v| v.as_str()).unwrap_or("Open").to_string();
    let title_filter = args.get("title").and_then(|v| v.as_str());

    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return super::tool_error("file_dialog_navigate", format!("no window matches '{filter}'")),
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
        return super::tool_error("file_dialog_navigate", "no file dialog window found");
    }

    let filename_clone = filename.clone();
    let button_clone = button_name.clone();

    let result = super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || {
        file_dialog_navigate_inner(hwnd, &filename_clone, &button_clone)
    }).and_then(|r| r);

    match result {
        Ok(msg) => NativeToolResult::text_only(msg),
        Err(e) => super::tool_error("file_dialog_navigate", e),
    }
}

#[cfg(windows)]
fn file_dialog_navigate_inner(hwnd: isize, filename: &str, button_name: &str) -> Result<String, String> {
    use windows::Win32::UI::Accessibility::*;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::Foundation::HWND as WIN32_HWND;
    use windows::core::{HRESULT, BSTR};

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() && hr != HRESULT(1) {
            return Err(format!("CoInitializeEx failed: {hr:?}"));
        }
    }

    let automation = super::ui_automation_tools::create_uiautomation_client()?;

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

/// OCR-based fallback for file dialog navigation on macOS/Linux.
/// Strategy: focus the filename field with Ctrl+L (or click via OCR), select all,
/// type the path, then press Enter to confirm.
#[cfg(not(windows))]
fn file_dialog_navigate_inner(_hwnd: isize, filename: &str, button_name: &str) -> Result<String, String> {
    use std::time::Duration;

    // Try to click on the filename field via OCR
    let monitors = xcap::Monitor::all().map_err(|e| format!("{e}"))?;
    if monitors.is_empty() {
        return Err("No monitors available".to_string());
    }
    let img = monitors[0].capture_image().map_err(|e| format!("Screen capture: {e}"))?;

    // Look for "File name" or "file name" label to click near the edit field
    let search_terms = ["file name", "filename", "save as", "location"];
    let mut clicked_field = false;
    for term in &search_terms {
        let s = term.to_string();
        let img_c = img.clone();
        if let Ok(matches) = super::ocr_tools::ocr_find_text(&img_c, &s, 0.0, 0.0) {
            if !matches.is_empty() {
                let m = &matches[0];
                // Click slightly to the right of the label (where the edit field usually is)
                let click_x = (m.center_x + m.width) as i64;
                let click_y = m.center_y as i64;
                let click_args = serde_json::json!({ "x": click_x, "y": click_y, "button": "left", "delay_ms": 200 });
                super::tool_click_screen(&click_args);
                clicked_field = true;
                break;
            }
        }
    }

    if !clicked_field {
        // Fallback: use Ctrl+L which focuses the path bar in many file dialogs (GTK, Qt)
        let key_args = serde_json::json!({ "key": "ctrl+l", "delay_ms": 200 });
        super::tool_press_key(&key_args);
    }

    std::thread::sleep(Duration::from_millis(200));

    // Select all existing text and replace with our filename
    let select_all = serde_json::json!({ "key": "ctrl+a", "delay_ms": 100 });
    super::tool_press_key(&select_all);
    std::thread::sleep(Duration::from_millis(100));

    // Type the filename
    let type_args = serde_json::json!({ "text": filename, "delay_ms": 200 });
    super::tool_type_text(&type_args);
    std::thread::sleep(Duration::from_millis(200));

    // Press Enter to confirm (equivalent to clicking Open/Save)
    let enter_args = serde_json::json!({ "key": "Return", "delay_ms": 300 });
    super::tool_press_key(&enter_args);

    Ok(format!("Typed filename '{}' and pressed Enter (for '{}')", filename, button_name))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_file_dialog_navigate(_args: &Value) -> NativeToolResult {
    super::tool_error("file_dialog_navigate", "not available on this platform")
}

// ─── drag_and_drop_element ──────────────────────────────────────────────────

/// Find two UI elements and drag from one to the other.
/// Windows: UI Automation element search. macOS/Linux: OCR fallback.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_drag_and_drop_element(args: &Value) -> NativeToolResult {
    let from_name = args.get("from_name").and_then(|v| v.as_str());
    let to_name = args.get("to_name").and_then(|v| v.as_str());
    let from_type = args.get("from_type").and_then(|v| v.as_str());
    let to_type = args.get("to_type").and_then(|v| v.as_str());

    if from_name.is_none() && from_type.is_none() {
        return super::tool_error("drag_and_drop_element", "at least 'from_name' or 'from_type' is required");
    }
    if to_name.is_none() && to_type.is_none() {
        return super::tool_error("drag_and_drop_element", "at least 'to_name' or 'to_type' is required");
    }

    let title_filter = args.get("title").and_then(|v| v.as_str());
    let delay_ms = args.get("delay_ms").and_then(parse_int).unwrap_or(500) as u64;

    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return super::tool_error("drag_and_drop_element", format!("no window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return super::tool_error("drag_and_drop_element", "no active window"),
        }
    };

    let prefer_ocr_fallback = gpu_app_for_hwnd(hwnd).is_some() && from_name.is_some() && to_name.is_some();

    // --- Windows: UI Automation path ---
    #[cfg(windows)]
    let positions: Result<((i32, i32, String), (i32, i32, String)), String> = {
        if prefer_ocr_fallback {
            let from_search = from_name.unwrap().to_lowercase();
            let to_search = to_name.unwrap().to_lowercase();
            match (
                ocr_find_element_on_screen(&from_search),
                ocr_find_element_on_screen(&to_search),
            ) {
                (Ok(from_pos), Ok(to_pos)) => Ok((from_pos, to_pos)),
                (Err(e), _) => Err(e),
                (_, Err(e)) => Err(e),
            }
        } else {
            let fn_owned = from_name.map(|s| s.to_lowercase());
            let ft_owned = from_type.map(|s| s.to_lowercase());
            let tn_owned = to_name.map(|s| s.to_lowercase());
            let tt_owned = to_type.map(|s| s.to_lowercase());

            let result = super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || {
                let from = super::find_ui_element(hwnd, fn_owned.as_deref(), ft_owned.as_deref())?;
                let to = super::find_ui_element(hwnd, tn_owned.as_deref(), tt_owned.as_deref())?;
                Ok((from, to))
            }).and_then(|r| r);

            result.map(|(from, to)| {
                ((from.cx, from.cy, from.desc()), (to.cx, to.cy, to.desc()))
            })
        }
    };

    // --- macOS/Linux: OCR fallback path ---
    #[cfg(not(windows))]
    let positions: Result<((i32, i32, String), (i32, i32, String)), String> = {
        let from_search = match from_name {
            Some(n) => n.to_lowercase(),
            None => from_type.unwrap_or("").to_lowercase(),
        };
        let to_search = match to_name {
            Some(n) => n.to_lowercase(),
            None => to_type.unwrap_or("").to_lowercase(),
        };

        let from_pos = ocr_find_element_on_screen(&from_search)?;
        let to_pos = ocr_find_element_on_screen(&to_search)?;
        Ok((from_pos, to_pos))
    };

    match positions {
        Ok((from, to)) => {
            let drag_args = serde_json::json!({
                "start_x": from.0,
                "start_y": from.1,
                "end_x": to.0,
                "end_y": to.1,
                "delay_ms": delay_ms,
            });
            let mut result = super::tool_mouse_drag(&drag_args);
            let route = if prefer_ocr_fallback { "ocr_fallback" } else { "uia" };
            result.text = format!(
                "Dragged {} at ({},{}) -> {} at ({},{}) via {}. {}",
                from.2, from.0, from.1,
                to.2, to.0, to.1,
                route,
                result.text
            );
            result
        }
        Err(e) => super::tool_error("drag_and_drop_element", e),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_drag_and_drop_element(_args: &Value) -> NativeToolResult {
    super::tool_error("drag_and_drop_element", "not available on this platform")
}

// ─── wait_for_text_on_screen ────────────────────────────────────────────────

/// Poll OCR until specified text appears on screen.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_wait_for_text_on_screen(args: &Value) -> NativeToolResult {
    let search_text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return super::tool_error("wait_for_text_on_screen", "'text' is required"),
    };
    let timeout_ms = args.get("timeout_ms").and_then(parse_int).unwrap_or(10000).min(30000) as u64;
    let poll_ms = args.get("poll_ms").and_then(parse_int).unwrap_or(1000).max(500) as u64;
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    let start = std::time::Instant::now();
    let search = search_text.to_lowercase();
    let base_poll = poll_ms;
    let mut attempt = 0u32;

    loop {
        if let Err(e) = super::ensure_desktop_not_cancelled() {
            return super::tool_error("wait_for_text_on_screen", e);
        }

        let monitors = match xcap::Monitor::all() {
            Ok(m) => m,
            Err(e) => return super::tool_error("wait_for_text_on_screen", e),
        };
        if monitor_idx >= monitors.len() {
            return super::tool_error("wait_for_text_on_screen", format!("monitor {monitor_idx} out of range"));
        }
        let img = match monitors[monitor_idx].capture_image() {
            Ok(i) => i,
            Err(e) => return super::tool_error("wait_for_text_on_screen", format!("capturing: {e}")),
        };

        let s = search.clone();
        let result = super::spawn_with_timeout(std::time::Duration::from_secs(10), move || {
            super::ocr_tools::ocr_find_text(&img, &s, 0.0, 0.0)
        }).and_then(|r| r);

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
        if let Err(e) = super::interruptible_sleep(std::time::Duration::from_millis(adaptive_delay)) {
            return super::tool_error("wait_for_text_on_screen", e);
        }
        attempt += 1;
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_wait_for_text_on_screen(_args: &Value) -> NativeToolResult {
    super::tool_error("wait_for_text_on_screen", "not available on this platform")
}

// ─── get_context_menu ───────────────────────────────────────────────────────

/// Right-click to open context menu, read items, optionally click one.
/// Windows: UI Automation to enumerate menu items. macOS/Linux: OCR fallback.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_get_context_menu(args: &Value) -> NativeToolResult {
    let x = match args.get("x").and_then(parse_int) {
        Some(v) => v as i32,
        None => return super::tool_error("get_context_menu", "'x' is required"),
    };
    let y = match args.get("y").and_then(parse_int) {
        Some(v) => v as i32,
        None => return super::tool_error("get_context_menu", "'y' is required"),
    };
    let click_item = args.get("click_item").and_then(|v| v.as_str()).map(|s| s.to_string());
    let delay_ms = args.get("delay_ms").and_then(parse_int).unwrap_or(500) as u64;

    // Right-click to open the menu
    let click_args = serde_json::json!({ "x": x, "y": y, "button": "right", "delay_ms": delay_ms });
    super::tool_click_screen(&click_args);

    // Wait a bit for menu to appear
    std::thread::sleep(std::time::Duration::from_millis(300));

    // --- Windows: UI Automation path ---
    #[cfg(windows)]
    let menu_result: Result<(Vec<String>, Option<(i32, i32, String)>), String> = {
        let hwnd = match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return NativeToolResult::text_only("Right-clicked but no active window found".to_string()),
        };

        let click_item_clone = click_item.clone();
        let result = super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || {
            let items = super::find_ui_elements_all(hwnd, None, Some("menuitem"), 20)?;
            if items.is_empty() {
                let items2 = super::find_ui_elements_all(hwnd, None, Some("menu"), 20)?;
                if items2.is_empty() {
                    return Err("No menu items found".to_string());
                }
                return Ok((items2, None));
            }

            let to_click = if let Some(ref target) = click_item_clone {
                let target_lower = target.to_lowercase();
                items.iter().find(|i| i.name.to_lowercase().contains(&target_lower))
                    .map(|i| (i.cx, i.cy, i.desc()))
            } else {
                None
            };

            Ok((items, to_click))
        }).and_then(|r| r);

        result.map(|(items, to_click)| {
            let descs: Vec<String> = items.iter().enumerate()
                .map(|(i, e)| format!("{}. {}", i + 1, e.desc()))
                .collect();
            (descs, to_click)
        })
    };

    // --- macOS/Linux: OCR fallback path ---
    #[cfg(not(windows))]
    let menu_result: Result<(Vec<String>, Option<(i32, i32, String)>), String> = {
        // Wait a little more for the context menu to fully render
        std::thread::sleep(std::time::Duration::from_millis(200));

        // Capture screen and OCR the region near the right-click point
        let monitors = xcap::Monitor::all().map_err(|e| format!("{e}"))?;
        if monitors.is_empty() {
            return NativeToolResult::text_only("No monitors available".to_string());
        }
        let img = monitors[0].capture_image().map_err(|e| format!("Screen capture: {e}"))?;

        // Crop a generous region around the click point where context menus typically appear
        let img_w = img.width() as i32;
        let img_h = img.height() as i32;
        let menu_x = (x - 10).max(0) as u32;
        let menu_y = (y - 10).max(0) as u32;
        let menu_w = (350.min(img_w - menu_x as i32)) as u32;
        let menu_h = (600.min(img_h - menu_y as i32)) as u32;

        let cropped = image::imageops::crop_imm(&img, menu_x, menu_y, menu_w, menu_h).to_image();

        // Run OCR on the cropped region
        #[cfg(target_os = "macos")]
        let ocr_text = super::ocr_tools::ocr_image_vision(&cropped)
            .or_else(|_| super::ocr_tools::ocr_image_tesseract(&cropped));

        #[cfg(target_os = "linux")]
        let ocr_text = super::ocr_tools::ocr_image_tesseract(&cropped);

        match ocr_text {
            Ok(text) => {
                if text.trim().is_empty() {
                    return Err("No menu items found via OCR".to_string());
                }
                // Parse lines as menu items
                let items: Vec<String> = text.lines()
                    .map(|l| l.trim())
                    .filter(|l| !l.is_empty())
                    .enumerate()
                    .map(|(i, line)| format!("{}. {}", i + 1, line))
                    .collect();

                // If click_item is specified, find it via OCR and click
                let to_click = if let Some(ref target) = click_item {
                    let target_lower = target.to_lowercase();
                    // Search for the target text in the full screen image to get accurate coords
                    let img_c = img.clone();
                    if let Ok(matches) = super::ocr_tools::ocr_find_text(&img_c, &target_lower, 0.0, 0.0) {
                        matches.first().map(|m| (m.center_x as i32, m.center_y as i32, format!("\"{}\"", m.text)))
                    } else {
                        None
                    }
                } else {
                    None
                };

                Ok((items, to_click))
            }
            Err(e) => Err(format!("OCR failed: {e}")),
        }
    };

    match menu_result {
        Ok((items, to_click)) => {
            let menu_list = format!("Context menu ({} items):\n{}", items.len(), items.join("\n"));

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
        Err(e) => super::tool_error("get_context_menu", format!("reading context menu: {e}")),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_get_context_menu(_args: &Value) -> NativeToolResult {
    super::tool_error("get_context_menu", "not available on this platform")
}

// ─── scroll_element ─────────────────────────────────────────────────────────

/// Find a UI element and scroll it (via ScrollPattern or mouse wheel fallback).
/// Windows: UI Automation element search. macOS/Linux: OCR fallback.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_scroll_element(args: &Value) -> NativeToolResult {
    let name_filter = args.get("name").and_then(|v| v.as_str());
    let type_filter = args.get("control_type").and_then(|v| v.as_str());
    let direction = args.get("direction").and_then(|v| v.as_str()).unwrap_or("down");
    let amount = args.get("amount").and_then(parse_int).unwrap_or(3) as i32;

    if name_filter.is_none() && type_filter.is_none() {
        return super::tool_error("scroll_element", "at least 'name' or 'control_type' is required");
    }

    let title_filter = args.get("title").and_then(|v| v.as_str());
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return super::tool_error("scroll_element", format!("no window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return super::tool_error("scroll_element", "no active window"),
        }
    };

    let prefer_ocr_fallback = gpu_app_for_hwnd(hwnd).is_some() && name_filter.is_some();

    let dir = direction.to_lowercase();

    // --- Windows: UI Automation path ---
    #[cfg(windows)]
    let element_pos: Result<(i32, i32, String), String> = {
        if prefer_ocr_fallback {
            let search_name = name_filter.unwrap().to_lowercase();
            ocr_find_element_on_screen(&search_name)
        } else {
            let name_owned = name_filter.map(|s| s.to_lowercase());
            let type_owned = type_filter.map(|s| s.to_lowercase());

            let result = super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || {
                super::find_ui_element(hwnd, name_owned.as_deref(), type_owned.as_deref())
            }).and_then(|r| r);

            result.map(|info| (info.cx, info.cy, info.desc()))
        }
    };

    // --- macOS/Linux: OCR fallback path ---
    #[cfg(not(windows))]
    let element_pos: Result<(i32, i32, String), String> = {
        let search_name = match name_filter {
            Some(n) => n.to_lowercase(),
            None => type_filter.unwrap_or("").to_lowercase(),
        };
        ocr_find_element_on_screen(&search_name)
    };

    match element_pos {
        Ok((cx, cy, desc)) => {
            let scroll_args = serde_json::json!({
                "x": cx,
                "y": cy,
                "direction": dir,
                "clicks": amount,
                "delay_ms": 300,
            });
            let mut result = super::tool_scroll_screen(&scroll_args);
            let route = if prefer_ocr_fallback { "ocr_fallback" } else { "uia" };
            result.text = format!(
                "Scrolled {dir} {amount} clicks on element {desc} at ({}, {}) via {}. {}",
                cx, cy, route, result.text
            );
            result
        }
        Err(e) => super::tool_error("scroll_element", e),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_scroll_element(_args: &Value) -> NativeToolResult {
    super::tool_error("scroll_element", "not available on this platform")
}

// ─── Shared OCR helper for element location (macOS/Linux) ───────────────────

/// Find an element on screen by name using OCR. Returns (cx, cy, description).
/// Used as the non-Windows fallback for UI Automation element search.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn ocr_find_element_on_screen(search_name: &str) -> Result<(i32, i32, String), String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("{e}"))?;
    if monitors.is_empty() {
        return Err("No monitors available".to_string());
    }
    let img = monitors[0].capture_image().map_err(|e| format!("Screen capture: {e}"))?;

    let s = search_name.to_string();
    let result = std::thread::spawn(move || {
        super::ocr_tools::ocr_find_text(&img, &s, 0.0, 0.0)
    }).join().unwrap_or_else(|_| Err("OCR thread panicked".to_string()));

    match result {
        Ok(matches) if !matches.is_empty() => {
            let m = &matches[0];
            Ok((m.center_x as i32, m.center_y as i32, format!("\"{}\"", m.text)))
        }
        Ok(_) => Err(format!("Element '{}' not found via OCR", search_name)),
        Err(e) => Err(format!("OCR: {e}")),
    }
}
