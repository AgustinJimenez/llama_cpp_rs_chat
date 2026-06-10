//! Win32 type definitions, constants, and FFI extern declarations.
#![allow(dead_code)]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

pub use enigo::Key;

// ─── Primitive type aliases ─────────────────────────────────────────────────

pub type HWND = isize;
pub type BOOL = i32;
pub type DWORD = u32;
pub type HANDLE = isize;
pub type LPARAM = isize;
pub type HDC = isize;
pub type COLORREF = u32;

// ─── Constants ──────────────────────────────────────────────────────────────

pub const PROCESS_QUERY_LIMITED_INFORMATION: DWORD = 0x1000;
pub const MAX_PATH: usize = 260;

// SetWindowPos flags
pub const SWP_NOMOVE: u32 = 0x0002;
pub const SWP_NOSIZE: u32 = 0x0001;
pub const SWP_NOZORDER: u32 = 0x0004;
pub const SWP_SHOWWINDOW: u32 = 0x0040;
pub const HWND_TOPMOST: HWND = -1;
pub const HWND_NOTOPMOST: HWND = -2;

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
// Window style (for fullscreen detection)
pub const GWL_STYLE: i32 = -16;
pub const WS_POPUP: i32 = -2147483648_i32; // 0x80000000
// Registry
pub type HKEY = isize;
pub const HKEY_LOCAL_MACHINE: HKEY = -2147483646; // 0x80000002
pub const HKEY_CURRENT_USER: HKEY = -2147483647; // 0x80000001
pub const KEY_READ: DWORD = 0x20019;
pub const REG_SZ: DWORD = 1;
pub const REG_DWORD: DWORD = 4;
pub const ERROR_SUCCESS: DWORD = 0;

// SendInput types
pub const INPUT_KEYBOARD: u32 = 1;
pub const KEYEVENTF_UNICODE: u32 = 0x0004;
pub const KEYEVENTF_KEYUP: u32 = 0x0002;
pub const KEYEVENTF_SCANCODE: u32 = 0x0008;
pub const MAPVK_VK_TO_VSC: u32 = 0;

// Stealth mouse input
pub const INPUT_MOUSE: u32 = 0;
pub const MOUSEEVENTF_MOVE: u32 = 0x0001;
pub const MOUSEEVENTF_LEFTDOWN: u32 = 0x0002;
pub const MOUSEEVENTF_LEFTUP: u32 = 0x0004;
pub const MOUSEEVENTF_RIGHTDOWN: u32 = 0x0008;
pub const MOUSEEVENTF_RIGHTUP: u32 = 0x0010;
pub const MOUSEEVENTF_ABSOLUTE: u32 = 0x8000;

// ─── Structs ─────────────────────────────────────────────────────────────────

#[repr(C)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

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

#[repr(C)]
pub struct MOUSEINPUT {
    pub dx: i32,
    pub dy: i32,
    pub mouse_data: u32,
    pub dw_flags: u32,
    pub time: u32,
    pub dw_extra_info: usize,
}

/// Raw INPUT struct sized for MOUSEINPUT (largest union variant = 24 bytes on x64).
#[repr(C)]
pub struct MouseINPUT {
    pub input_type: u32,
    pub _pad_union: [u8; 4], // alignment padding on x64
    pub mi: MOUSEINPUT,
}

pub struct WindowInfo {
    pub title: String,
    pub class_name: String,
    pub pid: u32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub process_name: String,
    pub minimized: bool,
    pub maximized: bool,
    pub focused: bool,
}

// ─── FFI extern declarations ─────────────────────────────────────────────────

type EnumWindowsProc = unsafe extern "system" fn(HWND, LPARAM) -> BOOL;

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
    pub fn SetCursorPos(x: i32, y: i32) -> BOOL;
    pub fn GetAsyncKeyState(vkey: i32) -> i16;
    pub fn GetMessageExtraInfo() -> usize;
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
    // Clipboard format registration (for CF_HTML etc.)
    pub fn RegisterClipboardFormatW(format: *const u16) -> u32;
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

extern "system" {
    pub fn GetSystemMetrics(index: i32) -> i32;
}

// ─── Shared helpers used across win32 submodules ─────────────────────────────

/// Encode a Rust string to a null-terminated UTF-16 buffer for Win32 API calls.
pub fn to_wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
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

/// # Safety
/// `pid` must be a valid process ID or 0.
pub unsafe fn get_process_name(pid: DWORD) -> String {
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
