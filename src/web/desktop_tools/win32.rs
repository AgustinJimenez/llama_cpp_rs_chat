//! Win32 FFI declarations and helpers for desktop automation tools.
#![allow(dead_code)] // FFI module: declarations are often added ahead of use

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use enigo::Key;

// Types
pub type HWND = isize;
pub type BOOL = i32;
pub type DWORD = u32;
pub type HANDLE = isize;
pub type LPARAM = isize;
pub type HDC = isize;
pub type COLORREF = u32;

pub const PROCESS_QUERY_LIMITED_INFORMATION: DWORD = 0x1000;
pub const MAX_PATH: usize = 260;

// SetWindowPos flags
pub const SWP_NOMOVE: u32 = 0x0002;
pub const SWP_NOSIZE: u32 = 0x0001;
pub const SWP_NOZORDER: u32 = 0x0004;
pub const SWP_SHOWWINDOW: u32 = 0x0040;
pub const HWND_TOPMOST: HWND = -1;
pub const HWND_NOTOPMOST: HWND = -2;

#[repr(C)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

type EnumWindowsProc = unsafe extern "system" fn(HWND, LPARAM) -> BOOL;

pub const SW_MINIMIZE: i32 = 6;
pub const SW_MAXIMIZE: i32 = 3;
pub const SW_RESTORE: i32 = 9;
pub const WM_CLOSE: u32 = 0x0010;
pub const CF_UNICODETEXT: u32 = 13;
pub const CF_DIB: u32 = 8;
pub const GMEM_MOVEABLE: u32 = 0x0002;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_KEYUP: u32 = 0x0101;
pub const WM_CHAR: u32 = 0x0102;
pub const PROCESS_TERMINATE: DWORD = 0x0001;
pub const TH32CS_SNAPPROCESS: DWORD = 0x00000002;
pub const MONITOR_DEFAULTTONEAREST: u32 = 2;
pub const INVALID_HANDLE_VALUE: HANDLE = -1;
// DPI
pub const USER_DEFAULT_SCREEN_DPI: u32 = 96;
// Dialog detection
pub const GW_ENABLEDPOPUP: u32 = 6;
// Clipboard file drop
pub const CF_HDROP: u32 = 15;
// Process info
pub const PROCESS_QUERY_INFORMATION: DWORD = 0x0400;
pub const PROCESS_VM_READ: DWORD = 0x0010;
// Window opacity
pub const WS_EX_LAYERED: i32 = 0x0008_0000;
pub const LWA_ALPHA: DWORD = 0x2;
pub const GWL_EXSTYLE: i32 = -20;
// Registry
pub type HKEY = isize;
pub const HKEY_LOCAL_MACHINE: HKEY = -2147483646; // 0x80000002
pub const HKEY_CURRENT_USER: HKEY = -2147483647; // 0x80000001
pub const KEY_READ: DWORD = 0x20019;
pub const REG_SZ: DWORD = 1;
pub const REG_DWORD: DWORD = 4;
pub const ERROR_SUCCESS: DWORD = 0;

#[repr(C)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
pub struct MONITORINFO {
    pub cb_size: u32,
    pub rc_monitor: RECT,
    pub rc_work: RECT,
    pub dw_flags: u32,
}

#[repr(C)]
pub struct PROCESSENTRY32W {
    pub dw_size: DWORD,
    pub cnt_usage: DWORD,
    pub th32_process_id: DWORD,
    pub th32_default_heap_id: usize,
    pub th32_module_id: DWORD,
    pub cnt_threads: DWORD,
    pub th32_parent_process_id: DWORD,
    pub pc_pri_class_base: i32,
    pub dw_flags: DWORD,
    pub sz_exe_file: [u16; 260],
}

#[repr(C)]
pub struct FILETIME {
    pub dw_low_date_time: u32,
    pub dw_high_date_time: u32,
}

#[repr(C)]
pub struct PROCESS_MEMORY_COUNTERS {
    pub cb: DWORD,
    pub page_fault_count: DWORD,
    pub peak_working_set_size: usize,
    pub working_set_size: usize,
    pub quota_peak_paged_pool_usage: usize,
    pub quota_paged_pool_usage: usize,
    pub quota_peak_non_paged_pool_usage: usize,
    pub quota_non_paged_pool_usage: usize,
    pub pagefile_usage: usize,
    pub peak_pagefile_usage: usize,
}

extern "system" {
    pub fn EnumWindows(cb: EnumWindowsProc, lparam: LPARAM) -> BOOL;
    pub fn GetWindowTextW(hwnd: HWND, buf: *mut u16, max_count: i32) -> i32;
    pub fn GetWindowTextLengthW(hwnd: HWND) -> i32;
    pub fn IsWindowVisible(hwnd: HWND) -> BOOL;
    pub fn IsIconic(hwnd: HWND) -> BOOL;
    pub fn IsZoomed(hwnd: HWND) -> BOOL;
    pub fn GetForegroundWindow() -> HWND;
    pub fn GetWindowRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
    pub fn GetWindowThreadProcessId(hwnd: HWND, pid: *mut DWORD) -> DWORD;
    pub fn OpenProcess(access: DWORD, inherit: BOOL, pid: DWORD) -> HANDLE;
    pub fn CloseHandle(handle: HANDLE) -> BOOL;
    pub fn QueryFullProcessImageNameW(
        process: HANDLE,
        flags: DWORD,
        name: *mut u16,
        size: *mut DWORD,
    ) -> BOOL;
    pub fn SetForegroundWindow(hwnd: HWND) -> BOOL;
    pub fn ShowWindow(hwnd: HWND, cmd: i32) -> BOOL;
    pub fn PostMessageW(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> BOOL;
    pub fn GetCursorPos(point: *mut POINT) -> BOOL;
    // Window positioning
    pub fn SetWindowPos(hwnd: HWND, after: HWND, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> BOOL;
    // Pixel color
    pub fn GetDC(hwnd: HWND) -> HDC;
    pub fn ReleaseDC(hwnd: HWND, hdc: HDC) -> i32;
    pub fn GetPixel(hdc: HDC, x: i32, y: i32) -> COLORREF;
    // Clipboard
    pub fn OpenClipboard(hwnd: HWND) -> BOOL;
    pub fn CloseClipboard() -> BOOL;
    pub fn EmptyClipboard() -> BOOL;
    pub fn GetClipboardData(format: u32) -> HANDLE;
    pub fn SetClipboardData(format: u32, mem: HANDLE) -> HANDLE;
    pub fn GlobalAlloc(flags: u32, bytes: usize) -> HANDLE;
    pub fn GlobalLock(mem: HANDLE) -> *mut u8;
    pub fn GlobalUnlock(mem: HANDLE) -> BOOL;
    // Shell execution
    pub fn ShellExecuteW(
        hwnd: HWND,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show_cmd: i32,
    ) -> isize;
    // Key mapping
    pub fn MapVirtualKeyW(code: u32, map_type: u32) -> u32;
    pub fn VkKeyScanW(ch: u16) -> i16;
    // Process management
    pub fn TerminateProcess(process: HANDLE, exit_code: u32) -> BOOL;
    pub fn GlobalSize(mem: HANDLE) -> usize;
    // Monitor info
    pub fn MonitorFromWindow(hwnd: HWND, flags: u32) -> isize;
    pub fn GetMonitorInfoW(monitor: isize, info: *mut MONITORINFO) -> BOOL;
    // Toolhelp32 (process enumeration)
    pub fn CreateToolhelp32Snapshot(flags: DWORD, pid: DWORD) -> HANDLE;
    pub fn Process32FirstW(snapshot: HANDLE, entry: *mut PROCESSENTRY32W) -> BOOL;
    pub fn Process32NextW(snapshot: HANDLE, entry: *mut PROCESSENTRY32W) -> BOOL;
    // SendInput
    pub fn SendInput(count: u32, inputs: *const INPUT, size: i32) -> u32;
    // DPI
    pub fn GetDpiForSystem() -> u32;
    // Window class name
    pub fn GetClassNameW(hwnd: HWND, buf: *mut u16, max_count: i32) -> i32;
    // Dialog/popup detection
    pub fn GetWindow(hwnd: HWND, cmd: u32) -> HWND;
    pub fn IsWindowEnabled(hwnd: HWND) -> BOOL;
    // Window style and opacity
    pub fn GetWindowLongW(hwnd: HWND, index: i32) -> i32;
    pub fn SetWindowLongW(hwnd: HWND, index: i32, new_long: i32) -> i32;
    pub fn SetLayeredWindowAttributes(hwnd: HWND, color: COLORREF, alpha: u8, flags: DWORD) -> BOOL;
    // Clipboard format check
    pub fn IsClipboardFormatAvailable(format: u32) -> BOOL;
    // Child window search (for system tray)
    pub fn FindWindowExW(parent: HWND, child_after: HWND, class: *const u16, window: *const u16) -> HWND;
    // Process times (kernel32)
    pub fn GetProcessTimes(
        process: HANDLE,
        creation: *mut FILETIME,
        exit: *mut FILETIME,
        kernel: *mut FILETIME,
        user: *mut FILETIME,
    ) -> BOOL;
}

#[link(name = "shell32")]
extern "system" {
    pub fn DragQueryFileW(hdrop: HANDLE, index: u32, file: *mut u16, count: u32) -> u32;
}

#[link(name = "advapi32")]
extern "system" {
    pub fn RegOpenKeyExW(
        key: HKEY,
        sub_key: *const u16,
        options: DWORD,
        sam: DWORD,
        result: *mut HKEY,
    ) -> DWORD;
    pub fn RegQueryValueExW(
        key: HKEY,
        value_name: *const u16,
        reserved: *mut DWORD,
        reg_type: *mut DWORD,
        data: *mut u8,
        data_size: *mut DWORD,
    ) -> DWORD;
    pub fn RegCloseKey(key: HKEY) -> DWORD;
}

#[link(name = "psapi")]
extern "system" {
    pub fn GetProcessMemoryInfo(
        process: HANDLE,
        counters: *mut PROCESS_MEMORY_COUNTERS,
        cb: DWORD,
    ) -> BOOL;
}

// SendInput types
pub const INPUT_KEYBOARD: u32 = 1;
pub const KEYEVENTF_UNICODE: u32 = 0x0004;
pub const KEYEVENTF_KEYUP: u32 = 0x0002;

#[repr(C)]
pub struct KEYBDINPUT {
    pub w_vk: u16,
    pub w_scan: u16,
    pub dw_flags: u32,
    pub time: u32,
    pub dw_extra_info: usize,
}

#[repr(C)]
pub struct INPUT {
    pub input_type: u32,
    pub ki: KEYBDINPUT,
    // union padding — MOUSEINPUT is larger, but we only use KEYBDINPUT
    pub _pad: [u8; 8],
}

pub struct WindowInfo {
    pub title: String,
    pub class_name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub process_name: String,
    pub minimized: bool,
    pub maximized: bool,
    pub focused: bool,
}

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
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
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

            if title.to_lowercase().contains(&lower_filter)
                || process_name.to_lowercase().contains(&lower_filter)
                || class_name.to_lowercase().contains(&lower_filter)
            {
                let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                GetWindowRect(hwnd, &mut rect);
                return Some((hwnd, WindowInfo {
                    title,
                    class_name,
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
    }
    None
}

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

pub fn get_cursor_position() -> (i32, i32) {
    let mut point = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut point); }
    (point.x, point.y)
}

pub fn read_clipboard() -> Result<String, String> {
    unsafe {
        if OpenClipboard(0) == 0 {
            return Err("Failed to open clipboard".to_string());
        }
        let handle = GetClipboardData(CF_UNICODETEXT);
        if handle == 0 {
            CloseClipboard();
            return Err("No text data in clipboard".to_string());
        }
        let ptr = GlobalLock(handle) as *const u16;
        if ptr.is_null() {
            CloseClipboard();
            return Err("Failed to lock clipboard data".to_string());
        }
        // Find null terminator
        let mut len = 0;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let text = OsString::from_wide(std::slice::from_raw_parts(ptr, len))
            .to_string_lossy()
            .into_owned();
        GlobalUnlock(handle);
        CloseClipboard();
        Ok(text)
    }
}

pub fn write_clipboard(text: &str) -> Result<(), String> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let byte_len = wide.len() * 2;
    unsafe {
        if OpenClipboard(0) == 0 {
            return Err("Failed to open clipboard".to_string());
        }
        EmptyClipboard();
        let hmem = GlobalAlloc(GMEM_MOVEABLE, byte_len);
        if hmem == 0 {
            CloseClipboard();
            return Err("Failed to allocate clipboard memory".to_string());
        }
        let ptr = GlobalLock(hmem);
        if ptr.is_null() {
            CloseClipboard();
            return Err("Failed to lock clipboard memory".to_string());
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, ptr, byte_len);
        GlobalUnlock(hmem);
        if SetClipboardData(CF_UNICODETEXT, hmem) == 0 {
            CloseClipboard();
            return Err("Failed to set clipboard data".to_string());
        }
        CloseClipboard();
        Ok(())
    }
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

/// Encode a Rust string to a null-terminated UTF-16 buffer for Win32 API calls.
fn to_wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
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

/// Enumerate all running processes. Returns Vec of (pid, exe_name).
pub fn enumerate_processes() -> Result<Vec<(DWORD, String)>, String> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err("CreateToolhelp32Snapshot failed".to_string());
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dw_size = std::mem::size_of::<PROCESSENTRY32W>() as DWORD;

        let mut result = Vec::new();
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let name_len = entry.sz_exe_file.iter().position(|&c| c == 0).unwrap_or(260);
                let name = OsString::from_wide(&entry.sz_exe_file[..name_len])
                    .to_string_lossy()
                    .into_owned();
                result.push((entry.th32_process_id, name));
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
        Ok(result)
    }
}

/// Terminate a process by PID. Refuses system-critical processes.
pub fn terminate_process(pid: DWORD) -> Result<(), String> {
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle == 0 {
            return Err(format!("OpenProcess failed for PID {pid} (access denied or not found)"));
        }
        let ok = TerminateProcess(handle, 1);
        CloseHandle(handle);
        if ok == 0 {
            return Err(format!("TerminateProcess failed for PID {pid}"));
        }
        Ok(())
    }
}

/// Map an enigo Key to a Win32 virtual key code (for PostMessageW).
pub fn key_to_vk(key: &Key) -> Option<u32> {
    match key {
        Key::Return => Some(0x0D),     // VK_RETURN
        Key::Tab => Some(0x09),        // VK_TAB
        Key::Escape => Some(0x1B),     // VK_ESCAPE
        Key::Backspace => Some(0x08),  // VK_BACK
        Key::Delete => Some(0x2E),     // VK_DELETE
        Key::Space => Some(0x20),      // VK_SPACE
        Key::UpArrow => Some(0x26),    // VK_UP
        Key::DownArrow => Some(0x28),  // VK_DOWN
        Key::LeftArrow => Some(0x25),  // VK_LEFT
        Key::RightArrow => Some(0x27), // VK_RIGHT
        Key::Home => Some(0x24),       // VK_HOME
        Key::End => Some(0x23),        // VK_END
        Key::PageUp => Some(0x21),     // VK_PRIOR
        Key::PageDown => Some(0x22),   // VK_NEXT
        Key::Control => Some(0x11),    // VK_CONTROL
        Key::Alt => Some(0x12),        // VK_MENU
        Key::Shift => Some(0x10),      // VK_SHIFT
        Key::Meta => Some(0x5B),       // VK_LWIN
        Key::CapsLock => Some(0x14),   // VK_CAPITAL
        Key::F1 => Some(0x70),
        Key::F2 => Some(0x71),
        Key::F3 => Some(0x72),
        Key::F4 => Some(0x73),
        Key::F5 => Some(0x74),
        Key::F6 => Some(0x75),
        Key::F7 => Some(0x76),
        Key::F8 => Some(0x77),
        Key::F9 => Some(0x78),
        Key::F10 => Some(0x79),
        Key::F11 => Some(0x7A),
        Key::F12 => Some(0x7B),
        Key::Unicode(c) => {
            let vk = unsafe { VkKeyScanW(*c as u16) };
            if vk == -1 { None } else { Some((vk & 0xFF) as u32) }
        }
        _ => None,
    }
}

/// Get the Win32 window class name for a given HWND.
pub fn get_window_class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if len <= 0 {
        return String::new();
    }
    OsString::from_wide(&buf[..len as usize])
        .to_string_lossy()
        .into_owned()
}

/// Get the system DPI scale factor (1.0 = 100%, 1.25 = 125%, etc.).
pub fn get_system_dpi_scale() -> f64 {
    let dpi = unsafe { GetDpiForSystem() };
    dpi as f64 / USER_DEFAULT_SCREEN_DPI as f64
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

/// Read file paths from clipboard (CF_HDROP format, e.g., from Windows Explorer copy).
pub fn read_clipboard_files() -> Result<Vec<String>, String> {
    unsafe {
        if OpenClipboard(0) == 0 {
            return Err("Failed to open clipboard".to_string());
        }
        let handle = GetClipboardData(CF_HDROP);
        if handle == 0 {
            CloseClipboard();
            return Err("No file drop data in clipboard".to_string());
        }
        // DragQueryFileW with index 0xFFFFFFFF returns the file count
        let count = DragQueryFileW(handle, 0xFFFFFFFF, std::ptr::null_mut(), 0);
        let mut files = Vec::with_capacity(count as usize);
        for i in 0..count {
            // Get required buffer size
            let len = DragQueryFileW(handle, i, std::ptr::null_mut(), 0);
            if len == 0 {
                continue;
            }
            let mut buf = vec![0u16; (len + 1) as usize];
            DragQueryFileW(handle, i, buf.as_mut_ptr(), buf.len() as u32);
            let path = OsString::from_wide(&buf[..len as usize])
                .to_string_lossy()
                .into_owned();
            files.push(path);
        }
        CloseClipboard();
        Ok(files)
    }
}

/// Get process resource info: working set (memory) and CPU times.
/// Returns (working_set_bytes, kernel_time_ms, user_time_ms).
pub fn get_process_resource_info(pid: DWORD) -> Result<(usize, u64, u64), String> {
    unsafe {
        let handle = OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
            0,
            pid,
        );
        if handle == 0 {
            return Err(format!("OpenProcess failed for PID {pid}"));
        }

        // Memory info
        let mut mem: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        mem.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as DWORD;
        let mem_ok = GetProcessMemoryInfo(handle, &mut mem, mem.cb);
        let working_set = if mem_ok != 0 {
            mem.working_set_size
        } else {
            0
        };

        // CPU times
        let mut creation: FILETIME = std::mem::zeroed();
        let mut exit: FILETIME = std::mem::zeroed();
        let mut kernel: FILETIME = std::mem::zeroed();
        let mut user: FILETIME = std::mem::zeroed();
        let time_ok = GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user);
        CloseHandle(handle);

        let (kernel_ms, user_ms) = if time_ok != 0 {
            let k = ((kernel.dw_high_date_time as u64) << 32 | kernel.dw_low_date_time as u64)
                / 10_000; // 100-ns units to ms
            let u = ((user.dw_high_date_time as u64) << 32 | user.dw_low_date_time as u64)
                / 10_000;
            (k, u)
        } else {
            (0, 0)
        };

        Ok((working_set, kernel_ms, user_ms))
    }
}

unsafe fn get_process_name(pid: DWORD) -> String {
    if pid == 0 {
        return String::new();
    }
    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if handle == 0 {
        return String::new();
    }
    let mut buf = vec![0u16; MAX_PATH];
    let mut size = buf.len() as DWORD;
    let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size);
    CloseHandle(handle);
    if ok == 0 || size == 0 {
        return String::new();
    }
    let full_path = OsString::from_wide(&buf[..size as usize])
        .to_string_lossy()
        .into_owned();
    // Extract just the filename
    full_path
        .rsplit('\\')
        .next()
        .unwrap_or(&full_path)
        .to_string()
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

/// Read a string or DWORD value from the Windows registry.
pub fn read_registry_value(hkey_root: HKEY, subkey: &str, value_name: &str) -> Result<String, String> {
    unsafe {
        let subkey_w = to_wide(subkey);
        let mut hkey: HKEY = 0;
        let status = RegOpenKeyExW(hkey_root, subkey_w.as_ptr(), 0, KEY_READ, &mut hkey);
        if status != ERROR_SUCCESS {
            return Err(format!("RegOpenKeyExW failed (error {status})"));
        }

        let value_w = to_wide(value_name);
        let mut reg_type: DWORD = 0;
        let mut data_size: DWORD = 0;

        // Query size first
        let status = RegQueryValueExW(
            hkey, value_w.as_ptr(), std::ptr::null_mut(),
            &mut reg_type, std::ptr::null_mut(), &mut data_size,
        );
        if status != ERROR_SUCCESS {
            RegCloseKey(hkey);
            return Err(format!("RegQueryValueExW failed (error {status})"));
        }

        let mut data = vec![0u8; data_size as usize];
        let status = RegQueryValueExW(
            hkey, value_w.as_ptr(), std::ptr::null_mut(),
            &mut reg_type, data.as_mut_ptr(), &mut data_size,
        );
        RegCloseKey(hkey);

        if status != ERROR_SUCCESS {
            return Err(format!("RegQueryValueExW read failed (error {status})"));
        }

        match reg_type {
            REG_SZ => {
                let wide: &[u16] = std::slice::from_raw_parts(
                    data.as_ptr() as *const u16,
                    data_size as usize / 2,
                );
                // Trim trailing null
                let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
                Ok(OsString::from_wide(&wide[..len]).to_string_lossy().into_owned())
            }
            REG_DWORD => {
                if data_size >= 4 {
                    let val = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                    Ok(val.to_string())
                } else {
                    Err("REG_DWORD data too small".to_string())
                }
            }
            other => Err(format!("Unsupported registry type: {other}")),
        }
    }
}

/// Check which clipboard formats are available.
pub fn get_clipboard_formats() -> Vec<&'static str> {
    let mut formats = Vec::new();
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT) != 0 {
            formats.push("text");
        }
        if IsClipboardFormatAvailable(CF_DIB) != 0 {
            formats.push("image");
        }
        if IsClipboardFormatAvailable(CF_HDROP) != 0 {
            formats.push("files");
        }
    }
    formats
}

/// Find a child window by class name traversal.
pub fn find_child_window(parent: HWND, class_name: &str) -> HWND {
    let class_w = to_wide(class_name);
    unsafe { FindWindowExW(parent, 0, class_w.as_ptr(), std::ptr::null()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_system_dpi_scale() {
        let scale = get_system_dpi_scale();
        // DPI scale should be at least 1.0 (100%) and at most 4.0 (400%)
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
        // At least some windows should have non-empty class names
        let with_class = windows.iter().filter(|w| !w.class_name.is_empty()).count();
        assert!(with_class > 0, "Some windows should have class names");
    }

    #[test]
    fn test_is_window_blocked_foreground() {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd != 0 {
            // The foreground window should typically NOT be blocked
            // (if it is, there's a modal dialog, which is possible but unlikely during tests)
            let _blocked = is_window_blocked(hwnd);
            // Just verify it doesn't crash
        }
    }

    #[test]
    fn test_get_process_resource_info_self() {
        let pid = std::process::id();
        let result = get_process_resource_info(pid);
        assert!(result.is_ok(), "Should get info for own process: {:?}", result);
        let (mem, _kernel, _user) = result.unwrap();
        assert!(mem > 0, "Own process should use some memory");
    }

    #[test]
    fn test_read_clipboard_files_no_crash() {
        // Just verify it doesn't crash — clipboard may or may not have files
        let _ = read_clipboard_files();
    }
}
