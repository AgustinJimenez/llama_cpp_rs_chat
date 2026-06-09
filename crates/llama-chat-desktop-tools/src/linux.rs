//! Linux platform helpers for desktop automation tools.
//! Provides the same public API as win32.rs using wmctrl, xdotool, arboard, and sysinfo.
#![allow(dead_code)]

use std::process::Command;

#[path = "linux/system.rs"]
mod system;
pub use system::{
    enumerate_processes, get_clipboard_formats, get_cursor_position, get_pixel_color,
    get_process_resource_info, get_system_dpi_scale, get_window_class_name, is_process_alive,
    key_to_vk, read_clipboard, read_clipboard_files, read_registry_value, set_window_opacity,
    shell_execute, terminate_process, write_clipboard,
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

// Constants — stubs matching win32.rs
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

// --- Helper: run command and get stdout ---

fn run_cmd(prog: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(prog)
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run {prog}: {e}. Is it installed?"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("{prog} error: {stderr}"))
    }
}

fn get_process_name_by_pid(pid: u32) -> String {
    // Read /proc/<pid>/comm for the process name
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

// --- Window management (via wmctrl + xdotool) ---

pub fn enumerate_windows() -> Vec<WindowInfo> {
    // wmctrl -lGp format: ID desktop PID x y w h hostname title...
    let output = match run_cmd("wmctrl", &["-lGp"]) {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    let active_id = run_cmd("xdotool", &["getactivewindow"])
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);

    let mut windows = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.splitn(9, char::is_whitespace).collect();
        if parts.len() < 9 {
            continue;
        }
        // Parse hex window ID
        let wid = u64::from_str_radix(parts[0].trim_start_matches("0x"), 16).unwrap_or(0);
        let pid: u32 = parts[2].parse().unwrap_or(0);
        let x: i32 = parts[3].parse().unwrap_or(0);
        let y: i32 = parts[4].parse().unwrap_or(0);
        let w: i32 = parts[5].parse().unwrap_or(0);
        let h: i32 = parts[6].parse().unwrap_or(0);
        // parts[7] is hostname, parts[8] is title
        let title = parts[8].to_string();
        let process_name = get_process_name_by_pid(pid);

        windows.push(WindowInfo {
            title,
            class_name: String::new(),
            pid,
            x,
            y,
            width: w,
            height: h,
            process_name,
            minimized: false, // wmctrl doesn't easily report this
            maximized: false,
            focused: wid == active_id,
        });
    }
    windows
}

pub fn find_window_by_filter(filter: &str) -> Option<(HWND, WindowInfo)> {
    fn normalized_basename(value: &str) -> String {
        value.rsplit(['\\', '/'])
            .next()
            .unwrap_or(value)
            .trim()
            .trim_end_matches(".exe")
            .to_lowercase()
    }

    fn match_score(filter: &str, title: &str, process_name: &str) -> Option<i32> {
        let filter = filter.trim().to_lowercase();
        if filter.is_empty() {
            return None;
        }

        let filter_base = normalized_basename(&filter);
        let title_lower = title.to_lowercase();
        let process_lower = process_name.to_lowercase();
        let process_base = normalized_basename(process_name);

        let score = if process_base == filter_base || process_lower == filter {
            500
        } else if process_base.contains(&filter_base) || process_lower.contains(&filter) {
            400
        } else if title_lower == filter {
            300
        } else if title_lower.starts_with(&filter) {
            250
        } else if title_lower.contains(&filter) {
            200
        } else {
            return None;
        };

        Some(score)
    }

    let output = match run_cmd("wmctrl", &["-lGp"]) {
        Ok(o) => o,
        Err(_) => return None,
    };
    let lower = filter.to_lowercase();
    let active_id = run_cmd("xdotool", &["getactivewindow"])
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let mut best_match: Option<(i32, HWND, WindowInfo)> = None;

    for line in output.lines() {
        let parts: Vec<&str> = line.splitn(9, char::is_whitespace).collect();
        if parts.len() < 9 {
            continue;
        }
        let wid = u64::from_str_radix(parts[0].trim_start_matches("0x"), 16).unwrap_or(0);
        let pid: u32 = parts[2].parse().unwrap_or(0);
        let title = parts[8].to_string();
        let process_name = get_process_name_by_pid(pid);

        if let Some(mut score) = match_score(&lower, &title, &process_name) {
            let info = WindowInfo {
                title,
                class_name: String::new(),
                pid,
                x: parts[3].parse().unwrap_or(0),
                y: parts[4].parse().unwrap_or(0),
                width: parts[5].parse().unwrap_or(0),
                height: parts[6].parse().unwrap_or(0),
                process_name,
                minimized: false,
                maximized: false,
                focused: wid == active_id,
            };

            if info.focused {
                score += 10;
            }

            let should_replace = best_match
                .as_ref()
                .map(|(best_score, _, _)| score > *best_score)
                .unwrap_or(true);
            if should_replace {
                best_match = Some((score, wid as HWND, info));
            }
        }
    }

    best_match.map(|(_, hwnd, info)| (hwnd, info))
}

pub fn find_window_by_pid(pid: u32) -> Option<(HWND, WindowInfo)> {
    let output = run_cmd("wmctrl", &["-lGp"]).ok()?;
    let active_id = run_cmd("xdotool", &["getactivewindow"])
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);

    for line in output.lines() {
        let parts: Vec<&str> = line.splitn(9, char::is_whitespace).collect();
        if parts.len() < 9 {
            continue;
        }
        let wid = u64::from_str_radix(parts[0].trim_start_matches("0x"), 16).unwrap_or(0);
        let candidate_pid: u32 = parts[2].parse().unwrap_or(0);
        if candidate_pid != pid {
            continue;
        }
        let title = parts[8].to_string();
        let process_name = get_process_name_by_pid(pid);
        return Some((
            wid as HWND,
            WindowInfo {
                title,
                class_name: String::new(),
                pid,
                x: parts[3].parse().unwrap_or(0),
                y: parts[4].parse().unwrap_or(0),
                width: parts[5].parse().unwrap_or(0),
                height: parts[6].parse().unwrap_or(0),
                process_name,
                minimized: false,
                maximized: false,
                focused: wid == active_id,
            },
        ));
    }
    None
}

pub fn get_active_window_info() -> Option<(HWND, WindowInfo)> {
    let wid_str = run_cmd("xdotool", &["getactivewindow"]).ok()?;
    let wid: u64 = wid_str.trim().parse().ok()?;
    let hex_id = format!("0x{:08x}", wid);

    // Get window geometry
    let geom = run_cmd("xdotool", &["getwindowgeometry", "--shell", &wid_str.trim()]).ok()?;
    let mut x = 0i32;
    let mut y = 0i32;
    let mut w = 0i32;
    let mut h = 0i32;
    for line in geom.lines() {
        if let Some(val) = line.strip_prefix("X=") { x = val.parse().unwrap_or(0); }
        if let Some(val) = line.strip_prefix("Y=") { y = val.parse().unwrap_or(0); }
        if let Some(val) = line.strip_prefix("WIDTH=") { w = val.parse().unwrap_or(0); }
        if let Some(val) = line.strip_prefix("HEIGHT=") { h = val.parse().unwrap_or(0); }
    }

    // Get PID
    let pid_str = run_cmd("xdotool", &["getwindowpid", &wid_str.trim()]).unwrap_or_default();
    let pid: u32 = pid_str.trim().parse().unwrap_or(0);

    // Get title
    let title = run_cmd("xdotool", &["getwindowname", &wid_str.trim()])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| hex_id);

    Some((wid as HWND, WindowInfo {
        title,
        class_name: String::new(),
        pid,
        x,
        y,
        width: w,
        height: h,
        process_name: get_process_name_by_pid(pid),
        minimized: false,
        maximized: false,
        focused: true,
    }))
}

/// Get WindowInfo for a specific HWND (stub — Linux doesn't use class names for GPU detection).
pub fn get_window_info_for_hwnd(_hwnd: HWND) -> Option<WindowInfo> {
    // Linux uses process_name for GPU app detection; re-use get_active_window_info
    get_active_window_info().map(|(_, info)| info)
}

pub fn focus_window(hwnd: HWND) -> bool {
    let hex = format!("0x{:08x}", hwnd as u64);
    run_cmd("wmctrl", &["-ia", &hex]).is_ok()
}

pub fn minimize_window(hwnd: HWND) -> bool {
    let id = format!("{}", hwnd as u64);
    run_cmd("xdotool", &["windowminimize", &id]).is_ok()
}

pub fn maximize_window(hwnd: HWND) -> bool {
    let hex = format!("0x{:08x}", hwnd as u64);
    run_cmd("wmctrl", &["-ir", &hex, "-b", "add,maximized_vert,maximized_horz"]).is_ok()
}

pub fn close_window(hwnd: HWND) -> bool {
    let hex = format!("0x{:08x}", hwnd as u64);
    run_cmd("wmctrl", &["-ic", &hex]).is_ok()
}

pub fn resize_window(hwnd: HWND, x: Option<i32>, y: Option<i32>, w: Option<i32>, h: Option<i32>) -> bool {
    let hex = format!("0x{:08x}", hwnd as u64);
    // wmctrl -ir <id> -e 0,x,y,w,h  (-1 means "current value")
    let sx = x.map_or("-1".to_string(), |v| v.to_string());
    let sy = y.map_or("-1".to_string(), |v| v.to_string());
    let sw = w.map_or("-1".to_string(), |v| v.to_string());
    let sh = h.map_or("-1".to_string(), |v| v.to_string());
    let spec = format!("0,{sx},{sy},{sw},{sh}");
    run_cmd("wmctrl", &["-ir", &hex, "-e", &spec]).is_ok()
}

pub fn get_window_rect(hwnd: HWND) -> RECT {
    let id = format!("{}", hwnd as u64);
    if let Ok(geom) = run_cmd("xdotool", &["getwindowgeometry", "--shell", &id]) {
        let mut x = 0i32;
        let mut y = 0i32;
        let mut w = 0i32;
        let mut h = 0i32;
        for line in geom.lines() {
            if let Some(val) = line.strip_prefix("X=") { x = val.parse().unwrap_or(0); }
            if let Some(val) = line.strip_prefix("Y=") { y = val.parse().unwrap_or(0); }
            if let Some(val) = line.strip_prefix("WIDTH=") { w = val.parse().unwrap_or(0); }
            if let Some(val) = line.strip_prefix("HEIGHT=") { h = val.parse().unwrap_or(0); }
        }
        RECT { left: x, top: y, right: x + w, bottom: y + h }
    } else {
        RECT { left: 0, top: 0, right: 0, bottom: 0 }
    }
}

pub fn set_topmost(hwnd: HWND, topmost: bool) -> bool {
    let hex = format!("0x{:08x}", hwnd as u64);
    let action = if topmost { "add" } else { "remove" };
    run_cmd("wmctrl", &["-ir", &hex, "-b", &format!("{action},above")]).is_ok()
}

pub fn get_monitor_work_area(_hwnd: HWND) -> Result<RECT, String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("xcap error: {e}"))?;
    if let Some(m) = monitors.first() {
        Ok(RECT {
            left: m.x(),
            top: m.y(),
            right: m.x() + m.width().unwrap_or(0) as i32,
            bottom: m.y() + m.height().unwrap_or(0) as i32,
        })
    } else {
        Err("No monitors found".to_string())
    }
}

pub fn is_window_blocked(_hwnd: HWND) -> Option<HWND> {
    // Get active window ID
    let wid = match std::process::Command::new("xdotool").arg("getactivewindow").output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => return None,
    };
    // Check for modal state
    match std::process::Command::new("xprop").args(&["-id", &wid, "_NET_WM_STATE"]).output() {
        Ok(out) if out.status.success() => {
            let props = String::from_utf8_lossy(&out.stdout);
            if props.contains("_NET_WM_STATE_MODAL") {
                Some(1)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn find_child_window(_parent: HWND, _class_name: &str) -> HWND { 0 }

pub unsafe fn GetForegroundWindow() -> HWND {
    run_cmd("xdotool", &["getactivewindow"])
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0) as HWND
}

pub unsafe fn SetWindowPos(hwnd: HWND, _insert_after: HWND, x: i32, y: i32, cx: i32, cy: i32, _flags: u32) -> BOOL {
    if resize_window(hwnd, Some(x), Some(y), Some(cx), Some(cy)) { 1 } else { 0 }
}
