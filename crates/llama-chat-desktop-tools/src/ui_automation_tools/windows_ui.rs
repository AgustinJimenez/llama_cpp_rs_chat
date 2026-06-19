#[cfg(windows)]
use windows::Win32::UI::Accessibility::{
    IUIAutomation, IUIAutomationElement, IUIAutomationTreeWalker,
};

#[cfg(windows)]
fn get_element_info(elem: &IUIAutomationElement) -> Result<String, String> {
    let name = unsafe { elem.CurrentName() }
        .map(|s| s.to_string())
        .unwrap_or_default();
    let control_type = unsafe { elem.CurrentControlType() }
        .map(|ct| control_type_name(ct.0))
        .unwrap_or_else(|_| "Unknown".to_string());

    if name.is_empty() {
        Ok(format!("[{control_type}]"))
    } else {
        let display_name = if name.len() > 80 {
            format!("{}...", &name[..80])
        } else {
            name
        };
        Ok(format!("[{control_type}] \"{display_name}\""))
    }
}

#[cfg(windows)]
pub(super) fn ui_tree_winrt(
    hwnd: isize,
    max_depth: usize,
    exclude_types: &[String],
) -> Result<String, String> {
    use windows::Win32::Foundation::HWND as WIN32_HWND;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Accessibility::*;
    use windows::core::HRESULT;

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() && hr != HRESULT(1) {
            return Err(format!("CoInitializeEx failed: {hr:?}"));
        }
    }

    let automation = create_uiautomation_client()?;
    let root = unsafe { automation.ElementFromHandle(WIN32_HWND(hwnd as *mut _)) }
        .map_err(|e| format!("ElementFromHandle: {e}"))?;
    let walker = unsafe { automation.ControlViewWalker() }
        .map_err(|e| format!("ControlViewWalker: {e}"))?;

    let mut output = String::new();
    let mut total_chars = 0usize;
    let mut truncated = false;
    const MAX_CHARS: usize = 50_000;

    if let Ok(info) = get_element_info(&root) {
        output.push_str(&info);
        output.push('\n');
        total_chars += info.len() + 1;
    }

    fn traverse(
        walker: &IUIAutomationTreeWalker,
        parent: &IUIAutomationElement,
        depth: usize,
        max_depth: usize,
        output: &mut String,
        total_chars: &mut usize,
        truncated: &mut bool,
        exclude_types: &[String],
    ) {
        if depth >= max_depth || *total_chars >= MAX_CHARS {
            if *total_chars >= MAX_CHARS {
                *truncated = true;
            }
            return;
        }
        let first_child = unsafe { walker.GetFirstChildElement(parent) };
        let mut current = match first_child {
            Ok(c) => c,
            Err(_) => return,
        };
        loop {
            let control_type_str = unsafe { current.CurrentControlType() }
                .map(|ct| control_type_name(ct.0))
                .unwrap_or_default();
            let excluded = !exclude_types.is_empty()
                && exclude_types
                    .iter()
                    .any(|ex| control_type_str.to_lowercase() == *ex);

            if !excluded {
                let indent = "  ".repeat(depth);
                if let Ok(info) = get_element_info(&current) {
                    let line = format!("{indent}{info}\n");
                    *total_chars += line.len();
                    output.push_str(&line);
                    if *total_chars >= MAX_CHARS {
                        *truncated = true;
                        return;
                    }
                }
                traverse(
                    walker,
                    &current,
                    depth + 1,
                    max_depth,
                    output,
                    total_chars,
                    truncated,
                    exclude_types,
                );
            }

            if *total_chars >= MAX_CHARS {
                *truncated = true;
                return;
            }
            match unsafe { walker.GetNextSiblingElement(&current) } {
                Ok(next) => current = next,
                Err(_) => break,
            }
        }
    }

    traverse(
        &walker,
        &root,
        1,
        max_depth,
        &mut output,
        &mut total_chars,
        &mut truncated,
        exclude_types,
    );

    if truncated {
        output.push_str(
            "\n[WARNING: UI tree output truncated at 50KB. Use exclude_types or reduce depth to see more.]",
        );
    }

    if output.is_empty() {
        Ok("(empty UI tree)".to_string())
    } else {
        Ok(output)
    }
}

#[cfg(not(windows))]
pub(super) fn ui_tree_winrt(
    _hwnd: isize,
    _max_depth: usize,
    _exclude_types: &[String],
) -> Result<String, String> {
    Err("UI Automation tree requires Windows COM APIs".to_string())
}

#[cfg(windows)]
pub(crate) fn control_type_name(id: i32) -> String {
    match id {
        50000 => "Button",
        50001 => "Calendar",
        50002 => "CheckBox",
        50003 => "ComboBox",
        50004 => "Edit",
        50005 => "Hyperlink",
        50006 => "Image",
        50007 => "ListItem",
        50008 => "List",
        50009 => "Menu",
        50010 => "MenuBar",
        50011 => "MenuItem",
        50012 => "ProgressBar",
        50013 => "RadioButton",
        50014 => "ScrollBar",
        50015 => "Slider",
        50016 => "Spinner",
        50017 => "StatusBar",
        50018 => "Tab",
        50019 => "TabItem",
        50020 => "Text",
        50021 => "ToolBar",
        50022 => "ToolTip",
        50023 => "Tree",
        50024 => "TreeItem",
        50025 => "Custom",
        50026 => "Group",
        50027 => "Thumb",
        50028 => "DataGrid",
        50029 => "DataItem",
        50030 => "Document",
        50031 => "SplitButton",
        50032 => "Window",
        50033 => "Pane",
        50034 => "Header",
        50035 => "HeaderItem",
        50036 => "Table",
        50037 => "TitleBar",
        50038 => "Separator",
        _ => return format!("UIA_{id}"),
    }
    .to_string()
}

#[cfg(not(windows))]
#[allow(dead_code)]
pub(crate) fn control_type_name(_id: i32) -> String {
    "unknown".to_string()
}

#[cfg(windows)]
pub(crate) fn create_uiautomation_client() -> Result<IUIAutomation, String> {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation2};
    use windows::core::Interface;

    const UIA_CONNECTION_TIMEOUT_MS: u32 = 1_000;
    const UIA_TRANSACTION_TIMEOUT_MS: u32 = 5_000;

    let automation: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
            .map_err(|e| format!("CoCreateInstance UIAutomation: {e}"))?;
    let automation2: IUIAutomation2 = automation
        .cast()
        .map_err(|e| format!("IUIAutomation2 cast: {e}"))?;

    unsafe {
        automation2
            .SetConnectionTimeout(UIA_CONNECTION_TIMEOUT_MS)
            .map_err(|e| format!("SetConnectionTimeout: {e}"))?;
        automation2
            .SetTransactionTimeout(UIA_TRANSACTION_TIMEOUT_MS)
            .map_err(|e| format!("SetTransactionTimeout: {e}"))?;
    }

    Ok(automation)
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
#[derive(Clone)]
pub(crate) struct UiElementInfo {
    pub cx: i32,
    pub cy: i32,
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    pub name: String,
    pub control_type: String,
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
impl UiElementInfo {
    pub fn desc(&self) -> String {
        if self.name.is_empty() {
            format!("[{}]", self.control_type)
        } else {
            format!("[{}] \"{}\"", self.control_type, self.name)
        }
    }
}

#[cfg(windows)]
thread_local! {
    #[allow(clippy::type_complexity)]
    static UI_ELEMENT_CACHE: std::cell::RefCell<std::collections::HashMap<(isize, String, String), (std::time::Instant, Vec<UiElementInfo>)>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

#[cfg(windows)]
pub(crate) fn invalidate_ui_element_cache(hwnd: isize) {
    UI_ELEMENT_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        cache.retain(|&(h, _, _), _| h != hwnd);
    });
}


#[cfg(windows)]
fn cache_lookup(
    hwnd: isize,
    name_filter: Option<&str>,
    type_filter: Option<&str>,
    max_results: usize,
) -> Option<Vec<UiElementInfo>> {
    let key = (
        hwnd,
        name_filter.unwrap_or("").to_string(),
        type_filter.unwrap_or("").to_string(),
    );
    UI_ELEMENT_CACHE.with(|cell| {
        let cache = cell.borrow();
        if let Some((when, results)) = cache.get(&key) {
            if when.elapsed().as_secs() < 2 {
                let subset: Vec<UiElementInfo> = results.iter().take(max_results).cloned().collect();
                if !subset.is_empty() {
                    return Some(subset);
                }
            }
        }
        None
    })
}

#[cfg(windows)]
fn cache_store(
    hwnd: isize,
    name_filter: Option<&str>,
    type_filter: Option<&str>,
    results: &[UiElementInfo],
) {
    let key = (
        hwnd,
        name_filter.unwrap_or("").to_string(),
        type_filter.unwrap_or("").to_string(),
    );
    UI_ELEMENT_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        cache.retain(|_, (when, _)| when.elapsed().as_secs() < 5);
        cache.insert(key, (std::time::Instant::now(), results.to_vec()));
    });
}

#[cfg(windows)]
pub(crate) fn find_ui_element(
    hwnd: isize,
    name_filter: Option<&str>,
    type_filter: Option<&str>,
) -> Result<UiElementInfo, String> {
    let results = find_ui_elements_all(hwnd, name_filter, type_filter, 1)?;
    results.into_iter().next().ok_or_else(|| {
        let filter_desc = match (name_filter, type_filter) {
            (Some(n), Some(t)) => format!("name='{n}', type='{t}'"),
            (Some(n), None) => format!("name='{n}'"),
            (None, Some(t)) => format!("type='{t}'"),
            (None, None) => "no filter".to_string(),
        };
        format!("No UI element found matching {filter_desc}")
    })
}

#[cfg(windows)]
pub(crate) fn find_ui_elements_all(
    hwnd: isize,
    name_filter: Option<&str>,
    type_filter: Option<&str>,
    max_results: usize,
) -> Result<Vec<UiElementInfo>, String> {
    use windows::Win32::Foundation::HWND as WIN32_HWND;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Accessibility::*;
    use windows::core::HRESULT;

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() && hr != HRESULT(1) {
            return Err(format!("CoInitializeEx failed: {hr:?}"));
        }
    }

    if max_results <= 10 {
        if let Some(cached) = cache_lookup(hwnd, name_filter, type_filter, max_results) {
            return Ok(cached);
        }
    }

    let automation = create_uiautomation_client()?;
    let root = unsafe { automation.ElementFromHandle(WIN32_HWND(hwnd as *mut _)) }
        .map_err(|e| format!("ElementFromHandle: {e}"))?;
    let walker = unsafe { automation.ControlViewWalker() }
        .map_err(|e| format!("ControlViewWalker: {e}"))?;

    let mut results = Vec::new();

    fn search(
        walker: &IUIAutomationTreeWalker,
        parent: &IUIAutomationElement,
        name_filter: Option<&str>,
        type_filter: Option<&str>,
        depth: usize,
        results: &mut Vec<UiElementInfo>,
        max_results: usize,
    ) {
        if depth > 8 || results.len() >= max_results {
            return;
        }

        let name = unsafe { parent.CurrentName() }.map(|s| s.to_string()).unwrap_or_default();
        let control_type = unsafe { parent.CurrentControlType() }
            .map(|ct| control_type_name(ct.0))
            .unwrap_or_default();

        let name_match = name_filter.is_none_or(|f| name.to_lowercase().contains(f));
        let type_match = type_filter.is_none_or(|f| control_type.to_lowercase().contains(f));

        if name_match && type_match && (!name.is_empty() || type_filter.is_some()) {
            if let Ok(r) = unsafe { parent.CurrentBoundingRectangle() } {
                if r.right > r.left && r.bottom > r.top {
                    results.push(UiElementInfo {
                        cx: (r.left + r.right) / 2,
                        cy: (r.top + r.bottom) / 2,
                        left: r.left,
                        top: r.top,
                        width: r.right - r.left,
                        height: r.bottom - r.top,
                        name: name.clone(),
                        control_type: control_type.clone(),
                    });
                    if results.len() >= max_results {
                        return;
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
            search(
                walker,
                &current,
                name_filter,
                type_filter,
                depth + 1,
                results,
                max_results,
            );
            if results.len() >= max_results {
                return;
            }
            match unsafe { walker.GetNextSiblingElement(&current) } {
                Ok(next) => current = next,
                Err(_) => break,
            }
        }
    }

    search(
        &walker,
        &root,
        name_filter,
        type_filter,
        0,
        &mut results,
        max_results,
    );
    cache_store(hwnd, name_filter, type_filter, &results);
    Ok(results)
}

#[cfg(not(windows))]
pub(crate) fn find_ui_element(
    hwnd: isize,
    name_filter: Option<&str>,
    type_filter: Option<&str>,
) -> Result<UiElementInfo, String> {
    let _ = (hwnd, name_filter, type_filter);
    Err("UI element search requires UI Automation (Windows) or accessibility APIs".to_string())
}

#[cfg(not(windows))]
pub(crate) fn find_ui_elements_all(
    hwnd: isize,
    name_filter: Option<&str>,
    type_filter: Option<&str>,
    max_results: usize,
) -> Result<Vec<UiElementInfo>, String> {
    let _ = (hwnd, name_filter, type_filter, max_results);
    Err("UI element search requires UI Automation (Windows) or accessibility APIs".to_string())
}

#[cfg(windows)]
pub(crate) fn find_raw_ui_element(
    walker: &IUIAutomationTreeWalker,
    parent: &IUIAutomationElement,
    name_filter: Option<&str>,
    type_filter: Option<&str>,
    depth: usize,
) -> Option<IUIAutomationElement> {
    if depth > 8 {
        return None;
    }

    let name = unsafe { parent.CurrentName() }.map(|s| s.to_string()).unwrap_or_default();
    let control_type = unsafe { parent.CurrentControlType() }
        .map(|ct| control_type_name(ct.0))
        .unwrap_or_default();

    let name_match = name_filter.is_none_or(|f| name.to_lowercase().contains(f));
    let type_match = type_filter.is_none_or(|f| control_type.to_lowercase().contains(f));

    if name_match && type_match && (!name.is_empty() || type_filter.is_some()) {
        if let Ok(r) = unsafe { parent.CurrentBoundingRectangle() } {
            if r.right > r.left && r.bottom > r.top {
                return Some(parent.clone());
            }
        }
    }

    let first_child = unsafe { walker.GetFirstChildElement(parent) };
    let mut current = match first_child {
        Ok(c) => c,
        Err(_) => return None,
    };
    loop {
        if let Some(found) = find_raw_ui_element(walker, &current, name_filter, type_filter, depth + 1) {
            return Some(found);
        }
        match unsafe { walker.GetNextSiblingElement(&current) } {
            Ok(next) => current = next,
            Err(_) => break,
        }
    }
    None
}

#[cfg(not(windows))]
#[allow(dead_code)]
pub(crate) fn find_raw_ui_element(
    _hwnd: isize,
    _name_filter: Option<&str>,
    _type_filter: Option<&str>,
) -> Result<isize, String> {
    Err("UI Automation not available on this platform".to_string())
}
