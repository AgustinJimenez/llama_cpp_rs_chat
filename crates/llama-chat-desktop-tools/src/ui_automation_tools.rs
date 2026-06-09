//! UI Automation tools: element tree, click, invoke, find, wait.

use serde_json::Value;

#[path = "ui_automation_tools/windows_ui.rs"]
mod windows_ui;

use super::NativeToolResult;
use super::{parse_int, tool_click_screen};
use super::gpu_app_db;
use windows_ui::ui_tree_winrt;
pub(crate) use windows_ui::{
    control_type_name, find_raw_ui_element, find_ui_element,
    find_ui_elements_all,
};
#[cfg(windows)]
pub(crate) use windows_ui::create_uiautomation_client;

#[cfg(windows)]
use super::win32;
#[cfg(target_os = "macos")]
use super::macos as win32;
#[cfg(target_os = "linux")]
use super::linux as win32;

// ─── GPU App Guard ───────────────────────────────────────────────────────────

/// Check if the target window belongs to a GPU-rendered application.
/// Returns an informative error result if so, None otherwise.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub(super) fn check_gpu_app_guard(hwnd: win32::HWND, tool_name: &str) -> Option<NativeToolResult> {
    let info = win32::get_window_info_for_hwnd(hwnd)?;
    let gpu = gpu_app_db::detect_gpu_app(&info.class_name, &info.process_name)?;
    let guidance = gpu_app_db::build_guidance(gpu);
    Some(NativeToolResult::text_only(format!(
        "{} detected (process: {}). \
         '{}' returned no results because {} renders its UI with GPU, not native widgets.\n\n{}\n\n\
         TIP: This app uses GPU rendering. Try execute_app_script or ocr_screen instead of UI Automation tools.",
        gpu.app_name, info.process_name, tool_name, gpu.app_name, guidance
    )))
}

// ─── UI Automation tools ──────────────────────────────────────────────────────

/// Get the UI element tree of a window using UI Automation.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_get_ui_tree(args: &Value) -> NativeToolResult {
    let title_filter = args.get("title").and_then(|v| v.as_str());
    let max_depth = args.get("depth")
        .or_else(|| args.get("max_depth"))
        .and_then(parse_int)
        .unwrap_or(3)
        .max(1).min(12) as usize;

    // Parse exclude_types array (lowercased)
    let exclude_types: Vec<String> = args.get("exclude_types")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).map(|s| s.to_lowercase()).collect())
        .unwrap_or_default();

    // Get target window HWND
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return super::tool_error("get_ui_tree", format!("no window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return super::tool_error("get_ui_tree", "no active window"),
        }
    };

    if let Some(r) = check_gpu_app_guard(hwnd, "get_ui_tree") { return r; }

    // Run on STA thread (COM UI Automation requires it)
    let result = super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || {
        ui_tree_winrt(hwnd, max_depth, &exclude_types)
    }).and_then(|r| r);

    match result {
        Ok(tree) => NativeToolResult::text_only(tree),
        Err(e) => super::tool_error("get_ui_tree", e),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_get_ui_tree(_args: &Value) -> NativeToolResult {
    super::tool_error("get_ui_tree", "not available on this platform")
}

/// Find a UI Automation element by name or control type and click it.
/// Supports `index` param to click the Nth match (0-based, default 0).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_click_ui_element(args: &Value) -> NativeToolResult {
    let name_filter = args.get("name").and_then(|v| v.as_str());
    let type_filter = args.get("control_type").and_then(|v| v.as_str());

    if name_filter.is_none() && type_filter.is_none() {
        return super::tool_error("click_ui_element", "at least 'name' or 'control_type' is required");
    }

    let title_filter = args.get("title").and_then(|v| v.as_str());
    let delay_ms = args.get("delay_ms").and_then(parse_int).unwrap_or(500) as u64;
    let index = args.get("index").and_then(parse_int).unwrap_or(0) as usize;

    // Get target window HWND
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return super::tool_error("click_ui_element", format!("no window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return super::tool_error("click_ui_element", "no active window"),
        }
    };

    if let Some(r) = check_gpu_app_guard(hwnd, "click_ui_element") { return r; }

    let use_cache = args.get("cache").map(|v| super::parse_bool(v, true)).unwrap_or(true);
    if !use_cache {
        // Invalidate cache for this window
        #[cfg(windows)]
        windows_ui::invalidate_ui_element_cache(hwnd);
    }

    let name_owned = name_filter.map(|s| s.to_lowercase());
    let type_owned = type_filter.map(|s| s.to_lowercase());

    // Find element(s) on STA thread — fetch index+1 results to pick the Nth
    let result = super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || {
        let results = find_ui_elements_all(hwnd, name_owned.as_deref(), type_owned.as_deref(), index + 1)?;
        let result_count = results.len();
        results.into_iter().nth(index).ok_or_else(|| {
            format!("Only {} element(s) found, but index {} was requested", result_count, index)
        })
    }).and_then(|r| r);

    match result {
        Ok(info) => {
            let element_desc = info.desc();
            let x = info.cx;
            let y = info.cy;
            let click_args = serde_json::json!({
                "x": x,
                "y": y,
                "button": "left",
                "delay_ms": delay_ms,
            });
            let mut result = tool_click_screen(&click_args);
            let idx_info = if index > 0 { format!(" (index {index})") } else { String::new() };
            result.text = format!("Clicked UI element {element_desc}{idx_info} at ({x}, {y}). {}", result.text);
            result
        }
        Err(e) => super::tool_error("click_ui_element", e),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_click_ui_element(_args: &Value) -> NativeToolResult {
    super::tool_error("click_ui_element", "not available on this platform")
}

/// Invoke a UI Automation action (invoke/toggle/expand/collapse/select/set_value) on an element.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_invoke_ui_action(args: &Value) -> NativeToolResult {
    let name_filter = args.get("name").and_then(|v| v.as_str());
    let type_filter = args.get("control_type").and_then(|v| v.as_str());
    let action = match args.get("action").and_then(|v| v.as_str()) {
        Some(a) => a.to_lowercase(),
        None => return super::tool_error("invoke_ui_action", "'action' is required (invoke/toggle/expand/collapse/select/set_value)"),
    };
    let value = args.get("value").and_then(|v| v.as_str()).map(|s| s.to_string());

    if name_filter.is_none() && type_filter.is_none() {
        return super::tool_error("invoke_ui_action", "at least 'name' or 'control_type' is required");
    }

    let title_filter = args.get("title").and_then(|v| v.as_str());
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return super::tool_error("invoke_ui_action", format!("no window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return super::tool_error("invoke_ui_action", "no active window"),
        }
    };

    if let Some(r) = check_gpu_app_guard(hwnd, "invoke_ui_action") { return r; }

    let name_owned = name_filter.map(|s| s.to_lowercase());
    let type_owned = type_filter.map(|s| s.to_lowercase());

    let result = super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || {
        invoke_ui_action_inner(hwnd, name_owned.as_deref(), type_owned.as_deref(), &action, value.as_deref())
    }).and_then(|r| r);

    match result {
        Ok(msg) => NativeToolResult::text_only(msg),
        Err(e) => super::tool_error("invoke_ui_action", e),
    }
}

#[cfg(windows)]
fn invoke_ui_action_inner(hwnd: isize, name_filter: Option<&str>, type_filter: Option<&str>, action: &str, value: Option<&str>) -> Result<String, String> {
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

    let automation = create_uiautomation_client()?;

    let root = unsafe {
        automation.ElementFromHandle(WIN32_HWND(hwnd as *mut _))
    }.map_err(|e| format!("ElementFromHandle: {e}"))?;

    let walker = unsafe {
        automation.ControlViewWalker()
    }.map_err(|e| format!("ControlViewWalker: {e}"))?;

    // Find the element first (reuse search logic from find_ui_elements_all but we need the raw element)
    let element = find_raw_ui_element(&walker, &root, name_filter, type_filter, 0)
        .ok_or_else(|| {
            let filter_desc = match (name_filter, type_filter) {
                (Some(n), Some(t)) => format!("name='{n}', type='{t}'"),
                (Some(n), None) => format!("name='{n}'"),
                (None, Some(t)) => format!("type='{t}'"),
                (None, None) => "no filter".to_string(),
            };
            format!("No UI element found matching {filter_desc}")
        })?;

    let elem_name = unsafe { element.CurrentName() }.map(|s| s.to_string()).unwrap_or_default();

    match action {
        "invoke" => {
            let pattern: IUIAutomationInvokePattern = unsafe {
                element.GetCurrentPatternAs(UIA_InvokePatternId)
            }.map_err(|e| format!("Element doesn't support Invoke pattern: {e}"))?;
            unsafe { pattern.Invoke() }.map_err(|e| format!("Invoke failed: {e}"))?;
            Ok(format!("Invoked element \"{elem_name}\""))
        }
        "toggle" => {
            let pattern: IUIAutomationTogglePattern = unsafe {
                element.GetCurrentPatternAs(UIA_TogglePatternId)
            }.map_err(|e| format!("Element doesn't support Toggle pattern: {e}"))?;
            unsafe { pattern.Toggle() }.map_err(|e| format!("Toggle failed: {e}"))?;
            let state = unsafe { pattern.CurrentToggleState() }
                .map(|s| format!("{s:?}"))
                .unwrap_or_else(|_| "unknown".to_string());
            Ok(format!("Toggled element \"{elem_name}\" → state: {state}"))
        }
        "expand" => {
            let pattern: IUIAutomationExpandCollapsePattern = unsafe {
                element.GetCurrentPatternAs(UIA_ExpandCollapsePatternId)
            }.map_err(|e| format!("Element doesn't support ExpandCollapse pattern: {e}"))?;
            unsafe { pattern.Expand() }.map_err(|e| format!("Expand failed: {e}"))?;
            Ok(format!("Expanded element \"{elem_name}\""))
        }
        "collapse" => {
            let pattern: IUIAutomationExpandCollapsePattern = unsafe {
                element.GetCurrentPatternAs(UIA_ExpandCollapsePatternId)
            }.map_err(|e| format!("Element doesn't support ExpandCollapse pattern: {e}"))?;
            unsafe { pattern.Collapse() }.map_err(|e| format!("Collapse failed: {e}"))?;
            Ok(format!("Collapsed element \"{elem_name}\""))
        }
        "select" => {
            let pattern: IUIAutomationSelectionItemPattern = unsafe {
                element.GetCurrentPatternAs(UIA_SelectionItemPatternId)
            }.map_err(|e| format!("Element doesn't support SelectionItem pattern: {e}"))?;
            unsafe { pattern.Select() }.map_err(|e| format!("Select failed: {e}"))?;
            Ok(format!("Selected element \"{elem_name}\""))
        }
        "set_value" => {
            let val = value.ok_or("'value' parameter is required for set_value action")?;
            let pattern: IUIAutomationValuePattern = unsafe {
                element.GetCurrentPatternAs(UIA_ValuePatternId)
            }.map_err(|e| format!("Element doesn't support Value pattern: {e}"))?;
            let bstr = BSTR::from(val);
            unsafe { pattern.SetValue(&bstr) }.map_err(|e| format!("SetValue failed: {e}"))?;
            Ok(format!("Set value of \"{elem_name}\" to \"{val}\""))
        }
        other => Err(format!("Unknown action '{other}'. Use: invoke, toggle, expand, collapse, select, set_value")),
    }
}

#[cfg(not(windows))]
fn invoke_ui_action_inner(_hwnd: isize, _name_filter: Option<&str>, _type_filter: Option<&str>, _action: &str, _value: Option<&str>) -> Result<String, String> {
    Err("UI Automation actions require Windows COM APIs".to_string())
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_invoke_ui_action(_args: &Value) -> NativeToolResult {
    super::tool_error("invoke_ui_action", "not available on this platform")
}

/// Read the current value or text of a UI element.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_read_ui_element_value(args: &Value) -> NativeToolResult {
    let name_filter = args.get("name").and_then(|v| v.as_str());
    let type_filter = args.get("control_type").and_then(|v| v.as_str());

    if name_filter.is_none() && type_filter.is_none() {
        return super::tool_error("read_ui_element_value", "at least 'name' or 'control_type' is required");
    }

    let title_filter = args.get("title").and_then(|v| v.as_str());
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return super::tool_error("read_ui_element_value", format!("no window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return super::tool_error("read_ui_element_value", "no active window"),
        }
    };

    if let Some(r) = check_gpu_app_guard(hwnd, "read_ui_element_value") { return r; }

    let name_owned = name_filter.map(|s| s.to_lowercase());
    let type_owned = type_filter.map(|s| s.to_lowercase());

    let result = super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || {
        read_ui_element_value_inner(hwnd, name_owned.as_deref(), type_owned.as_deref())
    }).and_then(|r| r);

    match result {
        Ok(msg) => NativeToolResult::text_only(msg),
        Err(e) => super::tool_error("read_ui_element_value", e),
    }
}

#[cfg(windows)]
fn read_ui_element_value_inner(hwnd: isize, name_filter: Option<&str>, type_filter: Option<&str>) -> Result<String, String> {
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

    let automation = create_uiautomation_client()?;

    let root = unsafe {
        automation.ElementFromHandle(WIN32_HWND(hwnd as *mut _))
    }.map_err(|e| format!("ElementFromHandle: {e}"))?;

    let walker = unsafe {
        automation.ControlViewWalker()
    }.map_err(|e| format!("ControlViewWalker: {e}"))?;

    let element = find_raw_ui_element(&walker, &root, name_filter, type_filter, 0)
        .ok_or_else(|| "No matching UI element found".to_string())?;

    let elem_name = unsafe { element.CurrentName() }.map(|s| s.to_string()).unwrap_or_default();
    let control_type = unsafe { element.CurrentControlType() }
        .map(|ct| control_type_name(ct.0))
        .unwrap_or_default();
    let rect = unsafe { element.CurrentBoundingRectangle() };
    let rect_str = rect.as_ref().map(|r| format!(" at ({}, {}) {}x{}", r.left, r.top, r.right - r.left, r.bottom - r.top)).unwrap_or_default();

    // Try ValuePattern first
    let value_result: Result<IUIAutomationValuePattern, _> = unsafe {
        element.GetCurrentPatternAs(UIA_ValuePatternId)
    };
    if let Ok(pattern) = value_result {
        if let Ok(val) = unsafe { pattern.CurrentValue() } {
            return Ok(format!("[{control_type}] \"{elem_name}\"{rect_str}\nValue: \"{val}\""));
        }
    }

    // Fallback to element name
    Ok(format!("[{control_type}] \"{elem_name}\"{rect_str}\nValue: (no ValuePattern, name shown above)"))
}

#[cfg(not(windows))]
fn read_ui_element_value_inner(_hwnd: isize, _name_filter: Option<&str>, _type_filter: Option<&str>) -> Result<String, String> {
    Err("UI element value reading requires Windows COM APIs".to_string())
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_read_ui_element_value(_args: &Value) -> NativeToolResult {
    super::tool_error("read_ui_element_value", "not available on this platform")
}

/// Poll until a UI element matching name/type appears.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_wait_for_ui_element(args: &Value) -> NativeToolResult {
    let name_filter = args.get("name").and_then(|v| v.as_str());
    let type_filter = args.get("control_type").and_then(|v| v.as_str());
    let timeout_ms = args.get("timeout_ms").and_then(parse_int).unwrap_or(10000).min(30000) as u64;
    let poll_ms = args.get("poll_ms").and_then(parse_int).unwrap_or(500).max(100) as u64;

    if name_filter.is_none() && type_filter.is_none() {
        return super::tool_error("wait_for_ui_element", "at least 'name' or 'control_type' is required");
    }

    let title_filter = args.get("title").and_then(|v| v.as_str());
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return super::tool_error("wait_for_ui_element", format!("no window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return super::tool_error("wait_for_ui_element", "no active window"),
        }
    };

    if let Some(r) = check_gpu_app_guard(hwnd, "wait_for_ui_element") { return r; }

    let name_owned = name_filter.map(|s| s.to_lowercase());
    let type_owned = type_filter.map(|s| s.to_lowercase());

    let start = std::time::Instant::now();
    let base_poll = poll_ms;
    let mut attempt = 0u32;
    loop {
        if let Err(e) = super::ensure_desktop_not_cancelled() {
            return super::tool_error("wait_for_ui_element", e);
        }

        let n = name_owned.clone();
        let t = type_owned.clone();
        let result = super::spawn_with_timeout(std::time::Duration::from_secs(5), move || {
            find_ui_element(hwnd, n.as_deref(), t.as_deref())
        }).and_then(|r| r);

        if let Ok(info) = result {
            return NativeToolResult::text_only(format!(
                "Element appeared: {} at ({}, {}) after {}ms",
                info.desc(), info.cx, info.cy, start.elapsed().as_millis()
            ));
        }

        if start.elapsed().as_millis() >= timeout_ms as u128 {
            let filter_desc = match (name_owned.as_deref(), type_owned.as_deref()) {
                (Some(n), Some(t)) => format!("name='{n}', type='{t}'"),
                (Some(n), None) => format!("name='{n}'"),
                (None, Some(t)) => format!("type='{t}'"),
                (None, None) => "no filter".to_string(),
            };
            return NativeToolResult::text_only(format!(
                "Timeout: element matching {filter_desc} not found after {timeout_ms}ms"
            ));
        }

        let adaptive_delay = super::screenshot_tools::adaptive_poll_ms(attempt, base_poll, base_poll * 4);
        if let Err(e) = super::interruptible_sleep(std::time::Duration::from_millis(adaptive_delay)) {
            return super::tool_error("wait_for_ui_element", e);
        }
        attempt += 1;
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_wait_for_ui_element(_args: &Value) -> NativeToolResult {
    super::tool_error("wait_for_ui_element", "not available on this platform")
}

// ─── Find UI elements tool ───────────────────────────────────────────────────

/// Find all UI elements matching name/type criteria, returning positions and details.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_find_ui_elements(args: &Value) -> NativeToolResult {
    let name_filter = args.get("name").and_then(|v| v.as_str());
    let type_filter = args.get("control_type").and_then(|v| v.as_str());
    let max_results = args.get("max_results").and_then(parse_int).unwrap_or(10).min(50) as usize;

    if name_filter.is_none() && type_filter.is_none() {
        return super::tool_error("find_ui_elements", "at least 'name' or 'control_type' is required");
    }

    let title_filter = args.get("title").and_then(|v| v.as_str());
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return super::tool_error("find_ui_elements", format!("no window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return super::tool_error("find_ui_elements", "no active window"),
        }
    };

    if let Some(r) = check_gpu_app_guard(hwnd, "find_ui_elements") { return r; }

    let use_cache = args.get("cache").map(|v| super::parse_bool(v, true)).unwrap_or(true);
    if !use_cache {
        // Invalidate cache for this window
        #[cfg(windows)]
        windows_ui::invalidate_ui_element_cache(hwnd);
    }

    let name_owned = name_filter.map(|s| s.to_lowercase());
    let type_owned = type_filter.map(|s| s.to_lowercase());

    let result = super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || {
        find_ui_elements_all(hwnd, name_owned.as_deref(), type_owned.as_deref(), max_results)
    }).and_then(|r| r);

    match result {
        Ok(elements) => {
            if elements.is_empty() {
                return NativeToolResult::text_only("No matching UI elements found".to_string());
            }
            let lines: Vec<String> = elements.iter().enumerate().map(|(i, e)| {
                format!("{}. {} at ({}, {}) size {}x{} center ({}, {})",
                    i + 1, e.desc(), e.left, e.top, e.width, e.height, e.cx, e.cy)
            }).collect();
            NativeToolResult::text_only(format!("Found {} element(s):\n{}", elements.len(), lines.join("\n")))
        }
        Err(e) => super::tool_error("find_ui_elements", e),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_find_ui_elements(_args: &Value) -> NativeToolResult {
    super::tool_error("find_ui_elements", "not available on this platform")
}
