//! UI element operation tools: type_into_element, drag_and_drop_element, scroll_element, get_window_text.

use serde_json::Value;

use super::super::NativeToolResult;
use super::super::parse_int;
use super::super::gpu_app_db;

#[cfg(windows)]
use super::super::win32;
#[cfg(target_os = "macos")]
use super::super::macos as win32;
#[cfg(target_os = "linux")]
use super::super::linux as win32;

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub(super) fn gpu_app_for_hwnd(hwnd: win32::HWND) -> Option<&'static gpu_app_db::GpuAppInfo> {
    let info = win32::get_window_info_for_hwnd(hwnd)?;
    gpu_app_db::detect_gpu_app(&info.class_name, &info.process_name)
}

/// Find an element on screen by name using OCR. Returns (cx, cy, description).
/// Used as the non-Windows fallback for UI Automation element search.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub(super) fn ocr_find_element_on_screen(search_name: &str) -> Result<(i32, i32, String), String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("{e}"))?;
    if monitors.is_empty() {
        return Err("No monitors available".to_string());
    }
    let img = monitors[0].capture_image().map_err(|e| format!("Screen capture: {e}"))?;

    let s = search_name.to_string();
    let result = std::thread::spawn(move || {
        super::super::ocr_tools::ocr_find_text(&img, &s, 0.0, 0.0)
    }).join().unwrap_or_else(|_| Err("OCR thread panicked".to_string()));

    match result {
        Ok(matches) if !matches.is_empty() => {
            let m = &matches[0];
            let t = &m.text;
            Ok((m.center_x as i32, m.center_y as i32, format!("\"{t}\"")))
        }
        Ok(_) => Err(format!("Element '{search_name}' not found via OCR")),
        Err(e) => Err(format!("OCR: {e}")),
    }
}

// ─── type_into_element ──────────────────────────────────────────────────────

/// Find a UI element → click to focus → type text into it.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_type_into_element(args: &Value) -> NativeToolResult {
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return super::super::tool_error("type_into_element", "'text' is required"),
    };
    let name_filter = args.get("name").and_then(|v| v.as_str());
    let type_filter = args.get("control_type").and_then(|v| v.as_str());

    if name_filter.is_none() && type_filter.is_none() {
        return super::super::tool_error("type_into_element", "at least 'name' or 'control_type' is required");
    }

    let title_filter = args.get("title").and_then(|v| v.as_str());
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return super::super::tool_error("type_into_element", format!("no window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return super::super::tool_error("type_into_element", "no active window"),
        }
    };

    let prefer_ocr_fallback = gpu_app_for_hwnd(hwnd).is_some() && name_filter.is_some();

    #[cfg(windows)]
    let element_pos: Result<(i32, i32, String), String> = {
        if prefer_ocr_fallback {
            let search_name = name_filter.unwrap().to_lowercase();
            ocr_find_element_on_screen(&search_name)
        } else {
            let name_owned = name_filter.map(|s| s.to_lowercase());
            let type_owned = type_filter.map(|s| s.to_lowercase());
            let timeout = super::super::parse_timeout(args);

            let result = super::super::spawn_with_timeout(timeout, move || {
                super::super::find_ui_element(hwnd, name_owned.as_deref(), type_owned.as_deref())
            }).and_then(|r| r);

            result.map(|info| (info.cx, info.cy, info.desc()))
        }
    };

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
            let click_args = serde_json::json!({ "x": cx, "y": cy, "button": "left", "delay_ms": 200 });
            super::super::tool_click_screen(&click_args);
            let type_args = serde_json::json!({ "text": text, "delay_ms": 300 });
            let mut result = super::super::tool_type_text(&type_args);
            let route = if prefer_ocr_fallback { "ocr_fallback" } else { "uia" };
            let char_count = text.len();
            let prev_text = result.text.clone();
            result.text = format!(
                "Clicked element {desc} at ({cx}, {cy}) via {route}, then typed {char_count} chars. {prev_text}"
            );
            result
        }
        Err(e) => super::super::tool_error("type_into_element", e),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_type_into_element(_args: &Value) -> NativeToolResult {
    super::super::tool_error("type_into_element", "not available on this platform")
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
            None => return super::super::tool_error("get_window_text", format!("no window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return super::super::tool_error("get_window_text", "no active window"),
        }
    };

    let timeout = super::super::parse_timeout(args);
    let result = super::super::spawn_with_timeout(timeout, move || {
        get_window_text_inner(hwnd, max_chars)
    }).and_then(|r| r);

    match result {
        Ok(text) => NativeToolResult::text_only(text),
        Err(e) => super::super::tool_error("get_window_text", e),
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

    let automation = super::super::ui_automation_tools::create_uiautomation_client()?;

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

#[cfg(not(windows))]
fn get_window_text_inner(_hwnd: isize, max_chars: usize) -> Result<String, String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("Monitor list: {e}"))?;
    if monitors.is_empty() {
        return Err("No monitors available".to_string());
    }
    let img = monitors[0].capture_image().map_err(|e| format!("Screen capture: {e}"))?;

    #[cfg(target_os = "macos")]
    let ocr_result = super::super::ocr_tools::ocr_image_vision(&img, None)
        .or_else(|_| super::super::ocr_tools::ocr_image_tesseract(&img, None));

    #[cfg(target_os = "linux")]
    let ocr_result = super::super::ocr_tools::ocr_image_tesseract(&img, None);

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
        .map(|ct| super::super::control_type_name(ct.0))
        .unwrap_or_default();

    if !name.is_empty() {
        match control_type.as_str() {
            "Text" | "Edit" | "Document" | "Hyperlink" => {
                if output.len() + name.len() + 1 <= max_chars {
                    output.push_str(&name);
                    output.push('\n');
                }
            }
            _ => {
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
    super::super::tool_error("get_window_text", "not available on this platform")
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
        return super::super::tool_error("drag_and_drop_element", "at least 'from_name' or 'from_type' is required");
    }
    if to_name.is_none() && to_type.is_none() {
        return super::super::tool_error("drag_and_drop_element", "at least 'to_name' or 'to_type' is required");
    }

    let title_filter = args.get("title").and_then(|v| v.as_str());
    let delay_ms = args.get("delay_ms").and_then(parse_int).unwrap_or(500) as u64;

    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return super::super::tool_error("drag_and_drop_element", format!("no window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return super::super::tool_error("drag_and_drop_element", "no active window"),
        }
    };

    let prefer_ocr_fallback = gpu_app_for_hwnd(hwnd).is_some() && from_name.is_some() && to_name.is_some();

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

            let timeout = super::super::parse_timeout(args);
            let result = super::super::spawn_with_timeout(timeout, move || {
                let from = super::super::find_ui_element(hwnd, fn_owned.as_deref(), ft_owned.as_deref())?;
                let to = super::super::find_ui_element(hwnd, tn_owned.as_deref(), tt_owned.as_deref())?;
                Ok((from, to))
            }).and_then(|r| r);

            result.map(|(from, to)| {
                ((from.cx, from.cy, from.desc()), (to.cx, to.cy, to.desc()))
            })
        }
    };

    #[cfg(not(windows))]
    let positions: Result<((i32, i32, String), (i32, i32, String)), String> = (|| {
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
    })();

    match positions {
        Ok((from, to)) => {
            let drag_args = serde_json::json!({
                "start_x": from.0, "start_y": from.1,
                "end_x": to.0, "end_y": to.1,
                "delay_ms": delay_ms,
            });
            let mut result = super::super::tool_mouse_drag(&drag_args);
            let route = if prefer_ocr_fallback { "ocr_fallback" } else { "uia" };
            let from_desc = &from.2;
            let to_desc = &to.2;
            let prev_text = result.text.clone();
            result.text = format!(
                "Dragged {from_desc} at ({},{}) -> {to_desc} at ({},{}) via {route}. {prev_text}",
                from.0, from.1, to.0, to.1
            );
            result
        }
        Err(e) => super::super::tool_error("drag_and_drop_element", e),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_drag_and_drop_element(_args: &Value) -> NativeToolResult {
    super::super::tool_error("drag_and_drop_element", "not available on this platform")
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
        return super::super::tool_error("scroll_element", "at least 'name' or 'control_type' is required");
    }

    let title_filter = args.get("title").and_then(|v| v.as_str());
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return super::super::tool_error("scroll_element", format!("no window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return super::super::tool_error("scroll_element", "no active window"),
        }
    };

    let prefer_ocr_fallback = gpu_app_for_hwnd(hwnd).is_some() && name_filter.is_some();
    let dir = direction.to_lowercase();

    #[cfg(windows)]
    let element_pos: Result<(i32, i32, String), String> = {
        if prefer_ocr_fallback {
            let search_name = name_filter.unwrap().to_lowercase();
            ocr_find_element_on_screen(&search_name)
        } else {
            let name_owned = name_filter.map(|s| s.to_lowercase());
            let type_owned = type_filter.map(|s| s.to_lowercase());
            let timeout = super::super::parse_timeout(args);
            let result = super::super::spawn_with_timeout(timeout, move || {
                super::super::find_ui_element(hwnd, name_owned.as_deref(), type_owned.as_deref())
            }).and_then(|r| r);
            result.map(|info| (info.cx, info.cy, info.desc()))
        }
    };

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
                "x": cx, "y": cy,
                "direction": dir,
                "clicks": amount,
                "delay_ms": 300,
            });
            let mut result = super::super::tool_scroll_screen(&scroll_args);
            let route = if prefer_ocr_fallback { "ocr_fallback" } else { "uia" };
            let prev_text = result.text.clone();
            result.text = format!(
                "Scrolled {dir} {amount} clicks on element {desc} at ({cx}, {cy}) via {route}. {prev_text}"
            );
            result
        }
        Err(e) => super::super::tool_error("scroll_element", e),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_scroll_element(_args: &Value) -> NativeToolResult {
    super::super::tool_error("scroll_element", "not available on this platform")
}
