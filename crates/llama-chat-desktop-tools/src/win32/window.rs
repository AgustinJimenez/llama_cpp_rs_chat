//! Window enumeration, lookup, focus, resize, and geometry helpers.

pub use super::window_search::{enumerate_windows, find_window_by_filter, find_window_by_pid, find_hwnds_by_pid};

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use super::types::*;

pub fn focus_window(hwnd: HWND) -> bool {
    unsafe {
        // Restore if minimized
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd) != 0
    }
}

pub fn minimize_window(hwnd: HWND) -> bool {
    unsafe { ShowWindow(hwnd, SW_MINIMIZE) != 0 }
}

pub fn maximize_window(hwnd: HWND) -> bool {
    unsafe { ShowWindow(hwnd, SW_MAXIMIZE) != 0 }
}

pub fn close_window(hwnd: HWND) -> bool {
    unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) != 0 }
}

/// Send WM_CLOSE to a window handle for graceful close.
pub fn close_window_graceful(hwnd: HWND) {
    unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0); }
}

pub fn get_cursor_position() -> (i32, i32) {
    let mut point = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut point); }
    (point.x, point.y)
}

pub fn resize_window(hwnd: HWND, x: Option<i32>, y: Option<i32>, w: Option<i32>, h: Option<i32>) -> bool {
    let mut flags = SWP_NOZORDER | SWP_SHOWWINDOW;
    if x.is_none() && y.is_none() {
        flags |= SWP_NOMOVE;
    }
    if w.is_none() && h.is_none() {
        flags |= SWP_NOSIZE;
    }
    // Get current rect as defaults
    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetWindowRect(hwnd, &mut rect); }
    let cx = x.unwrap_or(rect.left);
    let cy = y.unwrap_or(rect.top);
    let cw = w.unwrap_or(rect.right - rect.left);
    let ch = h.unwrap_or(rect.bottom - rect.top);
    unsafe { SetWindowPos(hwnd, 0, cx, cy, cw, ch, flags) != 0 }
}

pub fn get_active_window_info() -> Option<(HWND, WindowInfo)> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd == 0 {
        return None;
    }
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u16; (len + 1) as usize];
        let written = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if written <= 0 {
            return None;
        }
        let title = OsString::from_wide(&buf[..written as usize])
            .to_string_lossy()
            .into_owned();
        let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(hwnd, &mut rect);
        let mut pid: DWORD = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let process_name = get_process_name(pid);
        let class_name = get_window_class_name(hwnd);
        Some((hwnd, WindowInfo {
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
            focused: true,
        }))
    }
}

/// Get WindowInfo for a specific HWND (for GPU app detection guard).
pub fn get_window_info_for_hwnd(hwnd: HWND) -> Option<WindowInfo> {
    if hwnd == 0 {
        return None;
    }
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        let title = if len > 0 {
            let mut buf = vec![0u16; (len + 1) as usize];
            let written = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            if written > 0 {
                OsString::from_wide(&buf[..written as usize])
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        let mut pid_val: DWORD = 0;
        GetWindowThreadProcessId(hwnd, &mut pid_val);
        let process_name = get_process_name(pid_val);
        let class_name = get_window_class_name(hwnd);
        let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(hwnd, &mut rect);
        let foreground = GetForegroundWindow();
        Some(WindowInfo {
            title,
            class_name,
            pid: pid_val,
            x: rect.left,
            y: rect.top,
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
            process_name,
            minimized: IsIconic(hwnd) != 0,
            maximized: IsZoomed(hwnd) != 0,
            focused: hwnd == foreground,
        })
    }
}

/// Get a window's bounding rectangle.
pub fn get_window_rect(hwnd: HWND) -> RECT {
    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetWindowRect(hwnd, &mut rect); }
    rect
}

pub fn get_pixel_color(x: i32, y: i32) -> Result<(u8, u8, u8), String> {
    unsafe {
        let hdc = GetDC(0); // 0 = desktop DC
        if hdc == 0 {
            return Err("Failed to get desktop DC".to_string());
        }
        let color = GetPixel(hdc, x, y);
        ReleaseDC(0, hdc);
        if color == 0xFFFFFFFF {
            return Err(format!("GetPixel failed at ({x}, {y}) — coordinates may be off-screen"));
        }
        // COLORREF is 0x00BBGGRR
        let r = (color & 0xFF) as u8;
        let g = ((color >> 8) & 0xFF) as u8;
        let b = ((color >> 16) & 0xFF) as u8;
        Ok((r, g, b))
    }
}

pub fn set_topmost(hwnd: HWND, topmost: bool) -> bool {
    let insert_after = if topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
    unsafe {
        SetWindowPos(hwnd, insert_after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW) != 0
    }
}

pub fn shell_execute(file: &str, args: Option<&str>) -> Result<(), String> {
    let operation = to_wide("open");
    let file_w = to_wide(file);
    let args_w = args.map(to_wide);
    let args_ptr = args_w.as_ref().map_or(std::ptr::null(), |v| v.as_ptr());
    let result = unsafe {
        ShellExecuteW(0, operation.as_ptr(), file_w.as_ptr(), args_ptr, std::ptr::null(), 1 /* SW_SHOWNORMAL */)
    };
    // ShellExecuteW returns > 32 on success
    if result > 32 {
        Ok(())
    } else {
        Err(format!("ShellExecuteW failed with code {result}"))
    }
}

/// Get the work area (usable screen rect excluding taskbar) for the monitor containing a window.
pub fn get_monitor_work_area(hwnd: HWND) -> Result<RECT, String> {
    unsafe {
        let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if hmon == 0 {
            return Err("MonitorFromWindow returned null".to_string());
        }
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cb_size = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(hmon, &mut info) == 0 {
            return Err("GetMonitorInfoW failed".to_string());
        }
        Ok(info.rc_work)
    }
}

/// Set a window's opacity (0=transparent, 255=opaque).
pub fn set_window_opacity(hwnd: HWND, alpha: u8) -> Result<(), String> {
    unsafe {
        // Add WS_EX_LAYERED style
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        if ex_style & WS_EX_LAYERED == 0 {
            SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_LAYERED);
        }
        if SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA) == 0 {
            Err("SetLayeredWindowAttributes failed".to_string())
        } else {
            Ok(())
        }
    }
}

/// Check if a window is blocked by a modal dialog.
/// Returns Some(popup_hwnd) if a modal popup is blocking, None otherwise.
pub fn is_window_blocked(hwnd: HWND) -> Option<HWND> {
    unsafe {
        if IsWindowEnabled(hwnd) != 0 {
            return None; // Window is enabled, not blocked
        }
        let popup = GetWindow(hwnd, GW_ENABLEDPOPUP);
        if popup != 0 && popup != hwnd {
            Some(popup)
        } else {
            None
        }
    }
}

/// Find a child window by class name traversal.
pub fn find_child_window(parent: HWND, class_name: &str) -> HWND {
    let class_w = to_wide(class_name);
    unsafe { FindWindowExW(parent, 0, class_w.as_ptr(), std::ptr::null()) }
}

/// Check if a window is fullscreen by comparing its rect to the monitor it occupies.
pub fn is_window_fullscreen(hwnd: HWND) -> bool {
    unsafe {
        let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return false;
        }
        let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if hmon == 0 {
            return false;
        }
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cb_size = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(hmon, &mut info) == 0 {
            return false;
        }
        let mon = &info.rc_monitor;
        // Check if window rect matches monitor bounds exactly
        let exact_match = rect.left == mon.left
            && rect.top == mon.top
            && rect.right == mon.right
            && rect.bottom == mon.bottom;
        if exact_match {
            return true;
        }
        // Also check WS_POPUP style covering the monitor (borderless fullscreen)
        let style = GetWindowLongW(hwnd, GWL_STYLE);
        let is_popup = (style & WS_POPUP) != 0;
        let covers_monitor = rect.left <= mon.left
            && rect.top <= mon.top
            && rect.right >= mon.right
            && rect.bottom >= mon.bottom;
        is_popup && covers_monitor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_system_dpi_scale() {
        let scale = get_system_dpi_scale();
        assert!(scale >= 1.0, "DPI scale {scale} too low");
        assert!(scale <= 4.0, "DPI scale {scale} too high");
    }

    #[test]
    fn test_get_window_class_name_foreground() {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd != 0 {
            let class = get_window_class_name(hwnd);
            assert!(!class.is_empty(), "Foreground window should have a class name");
        }
    }

    #[test]
    fn test_enumerate_windows_has_class_name() {
        let windows = enumerate_windows();
        let with_class = windows.iter().filter(|w| !w.class_name.is_empty()).count();
        assert!(with_class > 0, "Some windows should have class names");
    }

    #[test]
    fn test_is_window_blocked_foreground() {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd != 0 {
            let _blocked = is_window_blocked(hwnd);
        }
    }
}
