//! Dialog navigation tools: file_dialog_navigate, get_context_menu.

use serde_json::Value;

use super::super::NativeToolResult;
use super::super::parse_int;

#[cfg(windows)]
use super::super::win32;
#[cfg(target_os = "macos")]
use super::super::macos as win32;
#[cfg(target_os = "linux")]
use super::super::linux as win32;

// ─── file_dialog_navigate ───────────────────────────────────────────────────

/// Navigate a file Open/Save dialog: set filename field and click Open/Save.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_file_dialog_navigate(args: &Value) -> NativeToolResult {
    let filename = match args.get("filename").and_then(|v| v.as_str()) {
        Some(f) => f.to_string(),
        None => return super::super::tool_error("file_dialog_navigate", "'filename' is required"),
    };
    let button_name = args.get("button").and_then(|v| v.as_str()).unwrap_or("Open").to_string();
    let title_filter = args.get("title").and_then(|v| v.as_str());

    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return super::super::tool_error("file_dialog_navigate", format!("no window matches '{filter}'")),
        }
    } else {
        win32::find_window_by_filter("Open")
            .or_else(|| win32::find_window_by_filter("Save"))
            .or_else(|| win32::find_window_by_filter("Browse"))
            .map(|(h, _)| h)
            .unwrap_or_else(|| win32::get_active_window_info().map(|(h, _)| h).unwrap_or(0))
    };

    if hwnd == 0 {
        return super::super::tool_error("file_dialog_navigate", "no file dialog window found");
    }

    let filename_clone = filename.clone();
    let button_clone = button_name.clone();
    let timeout = super::super::parse_timeout(args);

    let result = super::super::spawn_with_timeout(timeout, move || {
        file_dialog_navigate_inner(hwnd, &filename_clone, &button_clone)
    }).and_then(|r| r);

    match result {
        Ok(msg) => NativeToolResult::text_only(msg),
        Err(e) => super::super::tool_error("file_dialog_navigate", e),
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

    let automation = super::super::ui_automation_tools::create_uiautomation_client()?;

    let root = unsafe {
        automation.ElementFromHandle(WIN32_HWND(hwnd as *mut _))
    }.map_err(|e| format!("ElementFromHandle: {e}"))?;

    let walker = unsafe {
        automation.ControlViewWalker()
    }.map_err(|e| format!("ControlViewWalker: {e}"))?;

    let edit_elem = super::super::find_raw_ui_element(&walker, &root, Some("file name"), Some("edit"), 0)
        .or_else(|| super::super::find_raw_ui_element(&walker, &root, None, Some("edit"), 0))
        .ok_or("Could not find filename edit field")?;

    let pattern: IUIAutomationValuePattern = unsafe {
        edit_elem.GetCurrentPatternAs(UIA_ValuePatternId)
    }.map_err(|e| format!("Edit field doesn't support ValuePattern: {e}"))?;
    let bstr = BSTR::from(filename);
    unsafe { pattern.SetValue(&bstr) }.map_err(|e| format!("SetValue failed: {e}"))?;

    std::thread::sleep(std::time::Duration::from_millis(100));

    let button_search = button_name.to_lowercase();
    let button_elem = super::super::find_raw_ui_element(&walker, &root, Some(&button_search), Some("button"), 0)
        .ok_or_else(|| format!("Could not find '{button_name}' button"))?;

    let invoke: IUIAutomationInvokePattern = unsafe {
        button_elem.GetCurrentPatternAs(UIA_InvokePatternId)
    }.map_err(|e| format!("Button doesn't support Invoke: {e}"))?;
    unsafe { invoke.Invoke() }.map_err(|e| format!("Invoke failed: {e}"))?;

    Ok(format!("Set filename to '{filename}' and clicked '{button_name}'"))
}

#[cfg(not(windows))]
fn file_dialog_navigate_inner(_hwnd: isize, filename: &str, button_name: &str) -> Result<String, String> {
    use std::time::Duration;

    let monitors = xcap::Monitor::all().map_err(|e| format!("{e}"))?;
    if monitors.is_empty() {
        return Err("No monitors available".to_string());
    }
    let img = monitors[0].capture_image().map_err(|e| format!("Screen capture: {e}"))?;

    let search_terms = ["file name", "filename", "save as", "location"];
    let mut clicked_field = false;
    for term in &search_terms {
        let s = term.to_string();
        let img_c = img.clone();
        if let Ok(matches) = super::super::ocr_tools::ocr_find_text(&img_c, &s, 0.0, 0.0) {
            if !matches.is_empty() {
                let m = &matches[0];
                let click_x = (m.center_x + m.width) as i64;
                let click_y = m.center_y as i64;
                let click_args = serde_json::json!({ "x": click_x, "y": click_y, "button": "left", "delay_ms": 200 });
                super::super::tool_click_screen(&click_args);
                clicked_field = true;
                break;
            }
        }
    }

    if !clicked_field {
        let key_args = serde_json::json!({ "key": "ctrl+l", "delay_ms": 200 });
        super::super::tool_press_key(&key_args);
    }

    std::thread::sleep(Duration::from_millis(200));

    let select_all = serde_json::json!({ "key": "ctrl+a", "delay_ms": 100 });
    super::super::tool_press_key(&select_all);
    std::thread::sleep(Duration::from_millis(100));

    let type_args = serde_json::json!({ "text": filename, "delay_ms": 200 });
    super::super::tool_type_text(&type_args);
    std::thread::sleep(Duration::from_millis(200));

    let enter_args = serde_json::json!({ "key": "Return", "delay_ms": 300 });
    super::super::tool_press_key(&enter_args);

    Ok(format!("Typed filename '{filename}' and pressed Enter (for '{button_name}')"))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_file_dialog_navigate(_args: &Value) -> NativeToolResult {
    super::super::tool_error("file_dialog_navigate", "not available on this platform")
}

// ─── get_context_menu ───────────────────────────────────────────────────────

/// Right-click to open context menu, read items, optionally click one.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_get_context_menu(args: &Value) -> NativeToolResult {
    let x = match args.get("x").and_then(parse_int) {
        Some(v) => v as i32,
        None => return super::super::tool_error("get_context_menu", "'x' is required"),
    };
    let y = match args.get("y").and_then(parse_int) {
        Some(v) => v as i32,
        None => return super::super::tool_error("get_context_menu", "'y' is required"),
    };
    let click_item = args.get("click_item").and_then(|v| v.as_str()).map(|s| s.to_string());
    let delay_ms = args.get("delay_ms").and_then(parse_int).unwrap_or(500) as u64;

    let click_args = serde_json::json!({ "x": x, "y": y, "button": "right", "delay_ms": delay_ms });
    super::super::tool_click_screen(&click_args);
    std::thread::sleep(std::time::Duration::from_millis(300));

    #[cfg(windows)]
    let menu_result: Result<(Vec<String>, Option<(i32, i32, String)>), String> = {
        let hwnd = match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return NativeToolResult::text_only("Right-clicked but no active window found".to_string()),
        };

        let click_item_clone = click_item.clone();
        let timeout = super::super::parse_timeout(args);
        let result = super::super::spawn_with_timeout(timeout, move || {
            let items = super::super::find_ui_elements_all(hwnd, None, Some("menuitem"), 20)?;
            if items.is_empty() {
                let items2 = super::super::find_ui_elements_all(hwnd, None, Some("menu"), 20)?;
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

    #[cfg(not(windows))]
    #[allow(clippy::type_complexity)]
    let menu_result: Result<(Vec<String>, Option<(i32, i32, String)>), String> = (|| {
        std::thread::sleep(std::time::Duration::from_millis(200));

        let monitors = xcap::Monitor::all().map_err(|e| format!("{e}"))?;
        if monitors.is_empty() {
            return Err("No monitors available".to_string());
        }
        let img = monitors[0].capture_image().map_err(|e| format!("Screen capture: {e}"))?;

        let img_w = img.width() as i32;
        let img_h = img.height() as i32;
        let menu_x = (x - 10).max(0) as u32;
        let menu_y = (y - 10).max(0) as u32;
        let menu_w = (350.min(img_w - menu_x as i32)) as u32;
        let menu_h = (600.min(img_h - menu_y as i32)) as u32;

        let cropped = image::imageops::crop_imm(&img, menu_x, menu_y, menu_w, menu_h).to_image();

        #[cfg(target_os = "macos")]
        let ocr_text = super::super::ocr_tools::ocr_image_vision(&cropped, None)
            .or_else(|_| super::super::ocr_tools::ocr_image_tesseract(&cropped, None));

        #[cfg(target_os = "linux")]
        let ocr_text = super::super::ocr_tools::ocr_image_tesseract(&cropped, None);

        match ocr_text {
            Ok(text) => {
                if text.trim().is_empty() {
                    return Err("No menu items found via OCR".to_string());
                }
                let items: Vec<String> = text.lines()
                    .map(|l| l.trim())
                    .filter(|l| !l.is_empty())
                    .enumerate()
                    .map(|(i, line)| format!("{}. {}", i + 1, line))
                    .collect();

                let to_click = if let Some(ref target) = click_item {
                    let target_lower = target.to_lowercase();
                    let img_c = img.clone();
                    if let Ok(matches) = super::super::ocr_tools::ocr_find_text(&img_c, &target_lower, 0.0, 0.0) {
                        matches.first().map(|m| { let t = &m.text; (m.center_x as i32, m.center_y as i32, format!("\"{t}\"")) })
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
    })();

    match menu_result {
        Ok((items, to_click)) => {
            let item_count = items.len();
            let menu_items = items.join("\n");
            let menu_list = format!("Context menu ({item_count} items):\n{menu_items}");

            if let Some((cx, cy, desc)) = to_click {
                let click_args = serde_json::json!({ "x": cx, "y": cy, "button": "left", "delay_ms": 300 });
                let mut result = super::super::tool_click_screen(&click_args);
                result.text = format!("{menu_list}\nClicked: {desc} at ({cx}, {cy}). {}", result.text);
                result
            } else if let Some(item_name) = click_item {
                NativeToolResult::text_only(format!("{menu_list}\nNote: item '{item_name}' not found in menu"))
            } else {
                NativeToolResult::text_only(menu_list)
            }
        }
        Err(e) => super::super::tool_error("get_context_menu", format!("reading context menu: {e}")),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_get_context_menu(_args: &Value) -> NativeToolResult {
    super::super::tool_error("get_context_menu", "not available on this platform")
}
