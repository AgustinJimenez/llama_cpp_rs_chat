//! macOS platform helpers for desktop automation tools.
//! Provides the same public API as win32.rs using osascript/AppleScript, arboard, and sysinfo.
#![allow(dead_code, non_snake_case)]

use enigo::Key;
use std::process::Command;

mod window;
mod process;

// Re-export from submodules
pub use window::{
    enumerate_windows, find_window_by_filter, find_window_by_pid,
    get_active_window_info, get_window_info_for_hwnd,
    focus_window, minimize_window, maximize_window, close_window,
    resize_window, get_window_rect, set_topmost, get_monitor_work_area,
    is_window_blocked, get_cursor_position, get_pixel_color,
    get_window_class_name, get_system_dpi_scale, find_child_window,
    IsZoomed, ShowWindow, SetWindowPos, PostMessageW, SetForegroundWindow,
    GetForegroundWindow,
};

pub use process::{
    enumerate_processes, terminate_process, get_process_resource_info, is_process_alive,
};

// Types — match win32.rs signatures
pub type HWND = isize;
pub type BOOL = i32;
pub type DWORD = u32;
pub type HANDLE = isize;
pub type LPARAM = isize;
pub type HDC = isize;
pub type COLORREF = u32;
pub type HKEY = isize;

// Constants — stubs matching win32.rs (not used on macOS but needed for compilation)
pub const PROCESS_QUERY_LIMITED_INFORMATION: DWORD = 0x1000;
pub const MAX_PATH: usize = 260;
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
pub const USER_DEFAULT_SCREEN_DPI: u32 = 96;
pub const GW_ENABLEDPOPUP: u32 = 6;
pub const CF_HDROP: u32 = 15;
pub const PROCESS_QUERY_INFORMATION: DWORD = 0x0400;
pub const PROCESS_VM_READ: DWORD = 0x0010;
pub const WS_EX_LAYERED: i32 = 0x0008_0000;
pub const LWA_ALPHA: DWORD = 0x2;
pub const GWL_EXSTYLE: i32 = -20;
pub const HKEY_LOCAL_MACHINE: HKEY = -2147483646;
pub const HKEY_CURRENT_USER: HKEY = -2147483647;
pub const KEY_READ: DWORD = 0x20019;
pub const REG_SZ: DWORD = 1;
pub const REG_DWORD: DWORD = 4;
pub const ERROR_SUCCESS: DWORD = 0;
pub const INPUT_KEYBOARD: u32 = 1;
pub const KEYEVENTF_UNICODE: u32 = 0x0004;
pub const KEYEVENTF_KEYUP: u32 = 0x0002;
pub const KEYEVENTF_SCANCODE: u32 = 0x0008;
pub const MAPVK_VK_TO_VSC: u32 = 0;

// Structs — match win32.rs layout
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
    pub _pad: [u8; 8],
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

// --- Win32-compatible stubs (no-ops on macOS) ---

/// Stub: no-op on macOS. Returns 0.
pub unsafe fn SendInput(_count: u32, _inputs: *const INPUT, _size: i32) -> u32 {
    0
}

/// Stub: no-op on macOS. Returns 0.
pub unsafe fn MapVirtualKeyW(_code: u32, _map_type: u32) -> u32 {
    0
}

// --- Helper: run osascript ---

pub(crate) fn run_osascript(script: &str) -> Result<String, String> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| format!("Failed to run osascript: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("osascript error: {stderr}"))
    }
}

// --- Clipboard (via arboard) ---

pub fn read_clipboard() -> Result<String, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("Clipboard error: {e}"))?;
    clipboard.get_text().map_err(|e| format!("Clipboard read error: {e}"))
}

pub fn write_clipboard(text: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("Clipboard error: {e}"))?;
    clipboard.set_text(text.to_string()).map_err(|e| format!("Clipboard write error: {e}"))
}

pub fn read_clipboard_files() -> Result<Vec<String>, String> {
    Err("Clipboard file reading not supported on macOS".to_string())
}

pub fn get_clipboard_formats() -> Vec<&'static str> {
    let mut formats = Vec::new();
    if let Ok(mut cb) = arboard::Clipboard::new() {
        if cb.get_text().is_ok() {
            formats.push("text");
        }
        if cb.get_image().is_ok() {
            formats.push("image");
        }
    }
    formats
}

// --- Shell execute ---

pub fn shell_execute(file: &str, args: Option<&str>) -> Result<(), String> {
    let mut cmd = Command::new("open");
    cmd.arg(file);
    if let Some(a) = args {
        cmd.arg("--args").arg(a);
    }
    cmd.spawn().map_err(|e| format!("open failed: {e}"))?;
    Ok(())
}

// --- Key mapping ---

pub fn key_to_vk(key: &Key) -> Option<u32> {
    match key {
        Key::Return => Some(0x0D),
        Key::Tab => Some(0x09),
        Key::Escape => Some(0x1B),
        Key::Backspace => Some(0x08),
        Key::Delete => Some(0x2E),
        Key::Space => Some(0x20),
        Key::UpArrow => Some(0x26),
        Key::DownArrow => Some(0x28),
        Key::LeftArrow => Some(0x25),
        Key::RightArrow => Some(0x27),
        Key::Home => Some(0x24),
        Key::End => Some(0x23),
        Key::PageUp => Some(0x21),
        Key::PageDown => Some(0x22),
        Key::Control => Some(0x11),
        Key::Alt => Some(0x12),
        Key::Shift => Some(0x10),
        Key::Meta => Some(0x5B),
        Key::CapsLock => Some(0x14),
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
        Key::Unicode(c) => Some(*c as u32),
        _ => None,
    }
}

// --- Misc ---

pub fn set_window_opacity(_hwnd: HWND, alpha: u8) -> Result<(), String> {
    if alpha == 255 {
        return Ok(()); // fully opaque is the default, no-op
    }
    // macOS doesn't expose per-window transparency via public APIs.
    // NSWindow.alphaValue requires Objective-C runtime access from within the target app.
    Err(format!(
        "Window opacity ({:.0}%) is not available on macOS. \
         macOS requires in-process NSWindow.alphaValue access which cannot be done externally.",
        alpha as f64 / 255.0 * 100.0
    ))
}

pub fn read_registry_value(_hkey_root: HKEY, subkey: &str, value_name: &str) -> Result<String, String> {
    // macOS equivalent: `defaults read <domain> <key>`
    let output = Command::new("defaults")
        .arg("read")
        .arg(subkey)
        .arg(value_name)
        .output()
        .map_err(|e| format!("defaults read failed: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!(
            "defaults read failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}
