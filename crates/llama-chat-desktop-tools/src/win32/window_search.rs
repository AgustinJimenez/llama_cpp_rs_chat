//! Window search and enumeration helpers.
//!
//! Functions for enumerating all visible windows, finding windows by title/process
//! filter, and finding windows by process ID.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use super::types::*;

pub fn enumerate_windows() -> Vec<WindowInfo> {
    let mut hwnds: Vec<HWND> = Vec::new();

    unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = &mut *(lparam as *mut Vec<HWND>);
        list.push(hwnd);
        1 // TRUE — continue enumeration
    }

    unsafe {
        EnumWindows(enum_callback, &mut hwnds as *mut Vec<HWND> as LPARAM);
    }

    let foreground = unsafe { GetForegroundWindow() };
    let mut results = Vec::new();

    for hwnd in hwnds {
        unsafe {
            // Skip invisible windows
            if IsWindowVisible(hwnd) == 0 {
                continue;
            }

            // Get title
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                continue;
            }
            let mut buf = vec![0u16; (len + 1) as usize];
            let written = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            if written <= 0 {
                continue;
            }
            let title = OsString::from_wide(&buf[..written as usize])
                .to_string_lossy()
                .into_owned();

            // Get rect
            let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            if GetWindowRect(hwnd, &mut rect) == 0 {
                continue;
            }

            // Get process name
            let mut pid: DWORD = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            let process_name = get_process_name(pid);
            let class_name = get_window_class_name(hwnd);

            results.push(WindowInfo {
                title,
                class_name,
                pid,
                x: rect.left,
                y: rect.top,
                width: rect.right - rect.left,
                height: rect.bottom - rect.top,
                process_name,
                minimized: IsIconic(hwnd) != 0,
                maximized: IsZoomed(hwnd) != 0,
                focused: hwnd == foreground,
            });
        }
    }

    results
}

/// Find the first window whose title or process name contains the filter (case-insensitive).
/// Returns the HWND and the matching WindowInfo.
pub fn find_window_by_filter(filter: &str) -> Option<(HWND, WindowInfo)> {
    fn normalized_basename(value: &str) -> String {
        value.rsplit(['\\', '/'])
            .next()
            .unwrap_or(value)
            .trim()
            .trim_end_matches(".exe")
            .to_lowercase()
    }

    fn match_score(filter: &str, title: &str, process_name: &str, class_name: &str) -> Option<i32> {
        let filter = filter.trim().to_lowercase();
        if filter.is_empty() {
            return None;
        }

        let filter_base = normalized_basename(&filter);
        let title_lower = title.to_lowercase();
        let process_lower = process_name.to_lowercase();
        let class_lower = class_name.to_lowercase();
        let process_base = normalized_basename(process_name);
        let filter_looks_like_path = filter.contains('\\') || filter.contains('/');
        let title_looks_like_path =
            title_lower.contains(":\\") || title_lower.contains('\\') || title_lower.contains('/');

        let score = if process_base == filter_base || process_lower == filter {
            500
        } else if process_base.contains(&filter_base) || process_lower.contains(&filter) {
            400
        } else if title_looks_like_path && !filter_looks_like_path {
            return None;
        } else if title_lower == filter {
            300
        } else if title_lower.starts_with(&filter) {
            250
        } else if title_lower.contains(&filter) {
            200
        } else if class_lower == filter {
            150
        } else if class_lower.contains(&filter) {
            100
        } else {
            return None;
        };

        Some(score)
    }

    let mut hwnds: Vec<HWND> = Vec::new();

    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = &mut *(lparam as *mut Vec<HWND>);
        list.push(hwnd);
        1
    }

    unsafe {
        EnumWindows(enum_cb, &mut hwnds as *mut Vec<HWND> as LPARAM);
    }

    let lower_filter = filter.to_lowercase();
    let foreground = unsafe { GetForegroundWindow() };

    let mut best_match: Option<(i32, HWND, WindowInfo)> = None;

    for hwnd in hwnds {
        unsafe {
            if IsWindowVisible(hwnd) == 0 {
                continue;
            }
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                continue;
            }
            let mut buf = vec![0u16; (len + 1) as usize];
            let written = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            if written <= 0 {
                continue;
            }
            let title = OsString::from_wide(&buf[..written as usize])
                .to_string_lossy()
                .into_owned();

            let mut pid: DWORD = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            let process_name = get_process_name(pid);
            let class_name = get_window_class_name(hwnd);

            if let Some(mut score) = match_score(&lower_filter, &title, &process_name, &class_name)
            {
                let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                GetWindowRect(hwnd, &mut rect);

                let info = WindowInfo {
                    title,
                    class_name,
                    pid,
                    x: rect.left,
                    y: rect.top,
                    width: rect.right - rect.left,
                    height: rect.bottom - rect.top,
                    process_name,
                    minimized: IsIconic(hwnd) != 0,
                    maximized: IsZoomed(hwnd) != 0,
                    focused: hwnd == foreground,
                };

                if info.focused {
                    score += 10;
                }
                if !info.minimized {
                    score += 5;
                }

                let should_replace = best_match
                    .as_ref()
                    .map(|(best_score, _, _)| score > *best_score)
                    .unwrap_or(true);
                if should_replace {
                    best_match = Some((score, hwnd, info));
                }
            }
        }
    }

    best_match.map(|(_, hwnd, info)| (hwnd, info))
}

/// Find the first visible window belonging to a given process ID.
/// Returns the HWND and WindowInfo if found.
pub fn find_window_by_pid(target_pid: u32) -> Option<(HWND, WindowInfo)> {
    let mut hwnds: Vec<HWND> = Vec::new();

    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = &mut *(lparam as *mut Vec<HWND>);
        list.push(hwnd);
        1
    }

    unsafe {
        EnumWindows(enum_cb, &mut hwnds as *mut Vec<HWND> as LPARAM);
    }

    let foreground = unsafe { GetForegroundWindow() };

    for hwnd in hwnds {
        unsafe {
            if IsWindowVisible(hwnd) == 0 {
                continue;
            }
            let mut pid: DWORD = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid != target_pid {
                continue;
            }
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                continue;
            }
            let mut buf = vec![0u16; (len + 1) as usize];
            let written = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            if written <= 0 {
                continue;
            }
            let title = OsString::from_wide(&buf[..written as usize])
                .to_string_lossy()
                .into_owned();
            let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            GetWindowRect(hwnd, &mut rect);
            let process_name = get_process_name(pid);
            let class_name = get_window_class_name(hwnd);
            return Some((hwnd, WindowInfo {
                title,
                class_name,
                pid,
                x: rect.left,
                y: rect.top,
                width: rect.right - rect.left,
                height: rect.bottom - rect.top,
                process_name,
                minimized: IsIconic(hwnd) != 0,
                maximized: IsZoomed(hwnd) != 0,
                focused: hwnd == foreground,
            }));
        }
    }
    None
}

/// Find all window handles belonging to a given process ID (including those without titles).
pub fn find_hwnds_by_pid(target_pid: u32) -> Vec<HWND> {
    let mut hwnds: Vec<HWND> = Vec::new();

    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = &mut *(lparam as *mut Vec<HWND>);
        list.push(hwnd);
        1
    }

    unsafe {
        EnumWindows(enum_cb, &mut hwnds as *mut Vec<HWND> as LPARAM);
    }

    let mut result = Vec::new();
    for hwnd in hwnds {
        unsafe {
            let mut pid: DWORD = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == target_pid {
                result.push(hwnd);
            }
        }
    }
    result
}
