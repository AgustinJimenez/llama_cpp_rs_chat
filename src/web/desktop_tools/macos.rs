//! macOS platform helpers for desktop automation tools.
//! Provides the same public API as win32.rs using osascript/AppleScript, arboard, and sysinfo.
#![allow(dead_code)]

use enigo::Key;
use std::process::Command;

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

// --- Helper: run osascript ---

fn run_osascript(script: &str) -> Result<String, String> {
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

// --- Window management ---

pub fn enumerate_windows() -> Vec<WindowInfo> {
    // Use AppleScript to get window info from all visible processes
    let script = r#"
tell application "System Events"
    set output to ""
    set frontApp to name of first application process whose frontmost is true
    repeat with proc in (every application process whose visible is true)
        set procName to name of proc
        try
            repeat with win in (every window of proc)
                set winTitle to name of win
                set {posX, posY} to position of win
                set {sizeW, sizeH} to size of win
                set isMini to false
                try
                    set isMini to value of attribute "AXMinimized" of win
                end try
                set output to output & procName & "|||" & winTitle & "|||" & posX & "|||" & posY & "|||" & sizeW & "|||" & sizeH & "|||" & isMini & "|||" & (procName is equal to frontApp) & linefeed
            end repeat
        end try
    end repeat
    return output
end tell
"#;
    let result = match run_osascript(script) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    let mut windows = Vec::new();
    for line in result.lines() {
        let parts: Vec<&str> = line.split("|||").collect();
        if parts.len() >= 8 {
            windows.push(WindowInfo {
                process_name: parts[0].to_string(),
                title: parts[1].to_string(),
                class_name: String::new(),
                pid: 0, // macOS osascript doesn't easily provide PIDs per window
                x: parts[2].parse().unwrap_or(0),
                y: parts[3].parse().unwrap_or(0),
                width: parts[4].parse().unwrap_or(0),
                height: parts[5].parse().unwrap_or(0),
                minimized: parts[6] == "true",
                maximized: false, // macOS doesn't have a simple maximized concept
                focused: parts[7] == "true",
            });
        }
    }
    windows
}

pub fn find_window_by_filter(filter: &str) -> Option<(HWND, WindowInfo)> {
    let windows = enumerate_windows();
    let lower = filter.to_lowercase();
    for (i, w) in windows.into_iter().enumerate() {
        if w.title.to_lowercase().contains(&lower)
            || w.process_name.to_lowercase().contains(&lower)
        {
            return Some((i as HWND, w));
        }
    }
    None
}

pub fn get_active_window_info() -> Option<(HWND, WindowInfo)> {
    let script = r#"
tell application "System Events"
    set proc to first application process whose frontmost is true
    set procName to name of proc
    try
        set win to front window of proc
        set winTitle to name of win
        set {posX, posY} to position of win
        set {sizeW, sizeH} to size of win
        return procName & "|||" & winTitle & "|||" & posX & "|||" & posY & "|||" & sizeW & "|||" & sizeH
    on error
        return procName & "|||" & procName & "|||0|||0|||0|||0"
    end try
end tell
"#;
    let result = run_osascript(script).ok()?;
    let parts: Vec<&str> = result.split("|||").collect();
    if parts.len() >= 6 {
        Some((0, WindowInfo {
            process_name: parts[0].to_string(),
            title: parts[1].to_string(),
            class_name: String::new(),
            pid: 0,
            x: parts[2].parse().unwrap_or(0),
            y: parts[3].parse().unwrap_or(0),
            width: parts[4].parse().unwrap_or(0),
            height: parts[5].parse().unwrap_or(0),
            minimized: false,
            maximized: false,
            focused: true,
        }))
    } else {
        None
    }
}

/// Get WindowInfo for a specific HWND (stub — macOS doesn't use class names for GPU detection).
pub fn get_window_info_for_hwnd(_hwnd: HWND) -> Option<WindowInfo> {
    // macOS uses process_name for GPU app detection; re-use get_active_window_info
    get_active_window_info().map(|(_, info)| info)
}

pub fn focus_window(hwnd: HWND) -> bool {
    // hwnd is an index into enumerate_windows. Re-enumerate to find the process name.
    let windows = enumerate_windows();
    if let Some(w) = windows.get(hwnd as usize) {
        let script = format!(
            "tell application \"{}\" to activate",
            w.process_name.replace('"', "\\\"")
        );
        run_osascript(&script).is_ok()
    } else {
        false
    }
}

pub fn minimize_window(hwnd: HWND) -> bool {
    let windows = enumerate_windows();
    if let Some(w) = windows.get(hwnd as usize) {
        let script = format!(
            "tell application \"System Events\" to set value of attribute \"AXMinimized\" of front window of process \"{}\" to true",
            w.process_name.replace('"', "\\\"")
        );
        run_osascript(&script).is_ok()
    } else {
        false
    }
}

pub fn maximize_window(hwnd: HWND) -> bool {
    // macOS: set window bounds to screen size (approximation)
    let windows = enumerate_windows();
    if let Some(w) = windows.get(hwnd as usize) {
        let script = format!(
            "tell application \"System Events\" to tell process \"{}\" to set position of front window to {{0, 25}}\ntell application \"System Events\" to tell process \"{}\" to set size of front window to {{1920, 1055}}",
            w.process_name.replace('"', "\\\""),
            w.process_name.replace('"', "\\\"")
        );
        run_osascript(&script).is_ok()
    } else {
        false
    }
}

pub fn close_window(hwnd: HWND) -> bool {
    let windows = enumerate_windows();
    if let Some(w) = windows.get(hwnd as usize) {
        let script = format!(
            "tell application \"System Events\" to tell process \"{}\" to click button 1 of front window",
            w.process_name.replace('"', "\\\"")
        );
        // Fallback: tell app to close
        if run_osascript(&script).is_err() {
            let script2 = format!(
                "tell application \"{}\" to close front window",
                w.process_name.replace('"', "\\\"")
            );
            return run_osascript(&script2).is_ok();
        }
        true
    } else {
        false
    }
}

pub fn resize_window(hwnd: HWND, x: Option<i32>, y: Option<i32>, w: Option<i32>, h: Option<i32>) -> bool {
    let windows = enumerate_windows();
    if let Some(win) = windows.get(hwnd as usize) {
        let proc_name = win.process_name.replace('"', "\\\"");
        let mut parts = Vec::new();
        if x.is_some() || y.is_some() {
            let px = x.unwrap_or(win.x);
            let py = y.unwrap_or(win.y);
            parts.push(format!(
                "set position of front window of process \"{proc_name}\" to {{{px}, {py}}}"
            ));
        }
        if w.is_some() || h.is_some() {
            let sw = w.unwrap_or(win.width);
            let sh = h.unwrap_or(win.height);
            parts.push(format!(
                "set size of front window of process \"{proc_name}\" to {{{sw}, {sh}}}"
            ));
        }
        if parts.is_empty() {
            return true;
        }
        let script = format!(
            "tell application \"System Events\"\n{}\nend tell",
            parts.join("\n")
        );
        run_osascript(&script).is_ok()
    } else {
        false
    }
}

pub fn get_window_rect(hwnd: HWND) -> RECT {
    let windows = enumerate_windows();
    if let Some(w) = windows.get(hwnd as usize) {
        RECT { left: w.x, top: w.y, right: w.x + w.width, bottom: w.y + w.height }
    } else {
        RECT { left: 0, top: 0, right: 0, bottom: 0 }
    }
}

pub fn set_topmost(hwnd: HWND, topmost: bool) -> bool {
    if !topmost {
        return false; // Can't un-pin on macOS
    }
    // Get window info to find process name
    if let Some(info) = get_window_info_for_hwnd(hwnd) {
        let script = format!(
            r#"tell application "System Events" to set frontmost of process "{}" to true"#,
            info.process_name
        );
        match std::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
        {
            Ok(out) => out.status.success(),
            Err(_) => false,
        }
    } else {
        false
    }
}

pub fn get_monitor_work_area(_hwnd: HWND) -> Result<RECT, String> {
    // Use xcap to get monitor dimensions
    let monitors = xcap::Monitor::all().map_err(|e| format!("xcap error: {e}"))?;
    if let Some(m) = monitors.first() {
        Ok(RECT {
            left: m.x(),
            top: m.y(),
            right: m.x() + m.width() as i32,
            bottom: m.y() + m.height() as i32,
        })
    } else {
        Err("No monitors found".to_string())
    }
}

pub fn is_window_blocked(_hwnd: HWND) -> Option<HWND> {
    let script = r#"
tell application "System Events"
    set frontApp to first application process whose frontmost is true
    try
        set frontWin to window 1 of frontApp
        set r to subrole of frontWin
        if r is "AXSystemDialog" or r is "AXDialog" or r is "AXSheet" then
            return "blocked"
        end if
    end try
    return "ok"
end tell
"#;
    match run_osascript(script) {
        Ok(result) if result.trim() == "blocked" => Some(1),
        _ => None,
    }
}

// --- Cursor ---

pub fn get_cursor_position() -> (i32, i32) {
    // AppleScript approach
    let script = r#"
tell application "System Events"
    set mousePos to do shell script "python3 -c \"import Quartz; loc = Quartz.NSEvent.mouseLocation(); h = Quartz.CGDisplayPixelsHigh(Quartz.CGMainDisplayID()); print(f'{int(loc.x)},{int(h - loc.y)}')\""
    return mousePos
end tell
"#;
    if let Ok(result) = run_osascript(script) {
        let parts: Vec<&str> = result.split(',').collect();
        if parts.len() == 2 {
            let x = parts[0].parse().unwrap_or(0);
            let y = parts[1].parse().unwrap_or(0);
            return (x, y);
        }
    }
    (0, 0)
}

// --- Pixel color ---

pub fn get_pixel_color(x: i32, y: i32) -> Result<(u8, u8, u8), String> {
    // Capture a 1x1 region using xcap
    let monitors = xcap::Monitor::all().map_err(|e| format!("xcap error: {e}"))?;
    let monitor = monitors.first().ok_or("No monitors")?;
    let img = monitor.capture_image().map_err(|e| format!("capture error: {e}"))?;
    let mx = (x - monitor.x()) as u32;
    let my = (y - monitor.y()) as u32;
    if mx < img.width() && my < img.height() {
        let pixel = img.get_pixel(mx, my);
        Ok((pixel[0], pixel[1], pixel[2]))
    } else {
        Err(format!("Coordinates ({x}, {y}) out of screen bounds"))
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

// --- Process management (via sysinfo) ---

pub fn enumerate_processes() -> Result<Vec<(DWORD, String)>, String> {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let mut result = Vec::new();
    for (pid, proc_) in sys.processes() {
        result.push((pid.as_u32(), proc_.name().to_string_lossy().to_string()));
    }
    Ok(result)
}

pub fn terminate_process(pid: DWORD) -> Result<(), String> {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let spid = sysinfo::Pid::from_u32(pid);
    if let Some(proc_) = sys.process(spid) {
        if proc_.kill() {
            Ok(())
        } else {
            Err(format!("Failed to kill PID {pid}"))
        }
    } else {
        Err(format!("Process {pid} not found"))
    }
}

pub fn get_process_resource_info(pid: DWORD) -> Result<(usize, u64, u64), String> {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let spid = sysinfo::Pid::from_u32(pid);
    if let Some(proc_) = sys.process(spid) {
        let mem = proc_.memory() as usize;
        let cpu_time = proc_.run_time() * 1000; // seconds to ms
        Ok((mem, 0, cpu_time))
    } else {
        Err(format!("Process {pid} not found"))
    }
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

pub fn get_window_class_name(_hwnd: HWND) -> String {
    String::new() // No class names on macOS
}

pub fn get_system_dpi_scale() -> f64 {
    if let Ok(monitors) = xcap::Monitor::all() {
        if let Some(m) = monitors.first() {
            return m.scale_factor() as f64;
        }
    }
    1.0
}

pub fn set_window_opacity(_hwnd: HWND, _alpha: u8) -> Result<(), String> {
    Err("Window opacity not supported on macOS".to_string())
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

pub fn find_child_window(_parent: HWND, _class_name: &str) -> HWND {
    0 // Not applicable on macOS
}

/// Check if a process with the given PID is still alive.
pub fn is_process_alive(pid: DWORD) -> bool {
    if let Ok(procs) = enumerate_processes() {
        procs.iter().any(|(p, _)| *p == pid)
    } else {
        false
    }
}
