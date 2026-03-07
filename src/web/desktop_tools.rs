//! Desktop automation tools: mouse, keyboard, and scroll input simulation.
//!
//! Uses the `enigo` crate for cross-platform input and `xcap` for post-action screenshots.
//! Each action tool optionally captures a screenshot after the action, returning it through
//! the vision pipeline so the LLM can see what happened.

use enigo::{Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use serde_json::Value;

use super::native_tools::NativeToolResult;

/// Helper: take a screenshot after an action with optional delay.
fn capture_post_action_screenshot(delay_ms: u64) -> NativeToolResult {
    if delay_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
    }
    super::native_tools::tool_take_screenshot_with_image(&serde_json::json!({"monitor": 0}))
}

/// Helper: parse integer from JSON value (handles both number and string).
fn parse_int(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

/// Helper: parse bool from JSON value.
fn parse_bool(v: &Value, default: bool) -> bool {
    v.as_bool()
        .or_else(|| v.as_str().map(|s| s.eq_ignore_ascii_case("true")))
        .unwrap_or(default)
}

/// Parse a key string like "ctrl+shift+s" into (modifiers, main_key).
fn parse_key_combo(key_str: &str) -> Result<(Vec<Key>, Key), String> {
    let lower = key_str.to_lowercase();
    let parts: Vec<&str> = lower.split('+').map(|s| s.trim()).collect();

    if parts.is_empty() {
        return Err("Empty key string".to_string());
    }

    let mut modifiers = Vec::new();

    for part in &parts[..parts.len().saturating_sub(1)] {
        modifiers.push(match *part {
            "ctrl" | "control" => Key::Control,
            "alt" => Key::Alt,
            "shift" => Key::Shift,
            "meta" | "win" | "super" | "cmd" | "command" => Key::Meta,
            other => return Err(format!("Unknown modifier: '{other}'")),
        });
    }

    let main = parts.last().ok_or("Empty key string")?;
    let key = str_to_key(main)?;

    Ok((modifiers, key))
}

/// Convert a string key name to an enigo Key.
fn str_to_key(s: &str) -> Result<Key, String> {
    match s {
        "enter" | "return" => Ok(Key::Return),
        "tab" => Ok(Key::Tab),
        "escape" | "esc" => Ok(Key::Escape),
        "backspace" => Ok(Key::Backspace),
        "delete" | "del" => Ok(Key::Delete),
        "space" => Ok(Key::Space),
        "up" => Ok(Key::UpArrow),
        "down" => Ok(Key::DownArrow),
        "left" => Ok(Key::LeftArrow),
        "right" => Ok(Key::RightArrow),
        "home" => Ok(Key::Home),
        "end" => Ok(Key::End),
        "pageup" => Ok(Key::PageUp),
        "pagedown" => Ok(Key::PageDown),
        "insert" => Ok(Key::Other(0x2D)), // VK_INSERT on Windows
        "capslock" => Ok(Key::CapsLock),
        "ctrl" | "control" => Ok(Key::Control),
        "alt" => Ok(Key::Alt),
        "shift" => Ok(Key::Shift),
        "meta" | "win" | "super" | "cmd" => Ok(Key::Meta),
        // F-keys
        s if s.starts_with('f') && s.len() <= 3 => {
            match s[1..].parse::<u8>() {
                Ok(1) => Ok(Key::F1),
                Ok(2) => Ok(Key::F2),
                Ok(3) => Ok(Key::F3),
                Ok(4) => Ok(Key::F4),
                Ok(5) => Ok(Key::F5),
                Ok(6) => Ok(Key::F6),
                Ok(7) => Ok(Key::F7),
                Ok(8) => Ok(Key::F8),
                Ok(9) => Ok(Key::F9),
                Ok(10) => Ok(Key::F10),
                Ok(11) => Ok(Key::F11),
                Ok(12) => Ok(Key::F12),
                Ok(n) => Err(format!("Unsupported function key: F{n}")),
                Err(_) => Err(format!("Invalid key: '{s}'")),
            }
        }
        // Single character (a-z, 0-9, punctuation)
        s if s.len() == 1 => Ok(Key::Unicode(s.chars().next().unwrap())),
        other => Err(format!("Unknown key: '{other}'")),
    }
}

/// Click the mouse at absolute screen coordinates.
pub fn tool_click_screen(args: &Value) -> NativeToolResult {
    let x = match args.get("x").and_then(parse_int) {
        Some(v) => v as i32,
        None => return NativeToolResult::text_only("Error: 'x' coordinate is required".to_string()),
    };
    let y = match args.get("y").and_then(parse_int) {
        Some(v) => v as i32,
        None => return NativeToolResult::text_only("Error: 'y' coordinate is required".to_string()),
    };
    let button_str = args
        .get("button")
        .and_then(|v| v.as_str())
        .unwrap_or("left");
    let delay_ms = args
        .get("delay_ms")
        .and_then(parse_int)
        .unwrap_or(500) as u64;

    let mut enigo = match Enigo::new(&Settings::default()) {
        Ok(e) => e,
        Err(e) => {
            return NativeToolResult::text_only(format!(
                "Error: Failed to init input simulation: {e}"
            ))
        }
    };

    // Move mouse to position
    if let Err(e) = enigo.move_mouse(x, y, Coordinate::Abs) {
        return NativeToolResult::text_only(format!("Error: move_mouse failed: {e}"));
    }

    // Small delay after move for OS to register position
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Perform click
    let click_result = match button_str {
        "left" => enigo.button(Button::Left, Direction::Click),
        "right" => enigo.button(Button::Right, Direction::Click),
        "middle" => enigo.button(Button::Middle, Direction::Click),
        "double" => enigo
            .button(Button::Left, Direction::Click)
            .and_then(|()| {
                std::thread::sleep(std::time::Duration::from_millis(50));
                enigo.button(Button::Left, Direction::Click)
            }),
        other => {
            return NativeToolResult::text_only(format!(
                "Error: Unknown button '{other}'. Use: left, right, middle, double"
            ))
        }
    };

    if let Err(e) = click_result {
        return NativeToolResult::text_only(format!("Error: click failed: {e}"));
    }

    // Auto-screenshot with delay
    let mut result = capture_post_action_screenshot(delay_ms);
    result.text = format!(
        "Clicked {button_str} at ({x}, {y}). {}",
        result.text
    );
    result
}

/// Type text using keyboard simulation.
pub fn tool_type_text(args: &Value) -> NativeToolResult {
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return NativeToolResult::text_only("Error: 'text' argument is required".to_string()),
    };
    let do_screenshot = args
        .get("screenshot")
        .map(|v| parse_bool(v, true))
        .unwrap_or(true);
    let delay_ms = args
        .get("delay_ms")
        .and_then(parse_int)
        .unwrap_or(300) as u64;

    let mut enigo = match Enigo::new(&Settings::default()) {
        Ok(e) => e,
        Err(e) => {
            return NativeToolResult::text_only(format!(
                "Error: Failed to init input simulation: {e}"
            ))
        }
    };

    if let Err(e) = enigo.text(text) {
        return NativeToolResult::text_only(format!("Error: type_text failed: {e}"));
    }

    let summary = if text.len() > 50 {
        format!("Typed {} characters: \"{}...\"", text.len(), &text[..50])
    } else {
        format!("Typed: \"{text}\"")
    };

    if do_screenshot {
        let mut result = capture_post_action_screenshot(delay_ms);
        result.text = format!("{summary}. {}", result.text);
        result
    } else {
        NativeToolResult::text_only(summary)
    }
}

/// Press a key or key combination.
pub fn tool_press_key(args: &Value) -> NativeToolResult {
    let key_str = match args.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return NativeToolResult::text_only("Error: 'key' argument is required".to_string()),
    };
    let do_screenshot = args
        .get("screenshot")
        .map(|v| parse_bool(v, true))
        .unwrap_or(true);
    let delay_ms = args
        .get("delay_ms")
        .and_then(parse_int)
        .unwrap_or(500) as u64;

    let (modifiers, main_key) = match parse_key_combo(key_str) {
        Ok(combo) => combo,
        Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
    };

    let mut enigo = match Enigo::new(&Settings::default()) {
        Ok(e) => e,
        Err(e) => {
            return NativeToolResult::text_only(format!(
                "Error: Failed to init input simulation: {e}"
            ))
        }
    };

    // Press modifiers down
    for modifier in &modifiers {
        if let Err(e) = enigo.key(*modifier, Direction::Press) {
            return NativeToolResult::text_only(format!("Error: key press failed: {e}"));
        }
    }

    // Press and release main key
    if let Err(e) = enigo.key(main_key, Direction::Click) {
        // Release modifiers before returning error
        for modifier in modifiers.iter().rev() {
            let _ = enigo.key(*modifier, Direction::Release);
        }
        return NativeToolResult::text_only(format!("Error: key press failed: {e}"));
    }

    // Release modifiers in reverse order
    for modifier in modifiers.iter().rev() {
        let _ = enigo.key(*modifier, Direction::Release);
    }

    let summary = format!("Pressed: {key_str}");

    if do_screenshot {
        let mut result = capture_post_action_screenshot(delay_ms);
        result.text = format!("{summary}. {}", result.text);
        result
    } else {
        NativeToolResult::text_only(summary)
    }
}

/// Move the mouse cursor without clicking.
pub fn tool_move_mouse(args: &Value) -> NativeToolResult {
    let x = match args.get("x").and_then(parse_int) {
        Some(v) => v as i32,
        None => return NativeToolResult::text_only("Error: 'x' coordinate is required".to_string()),
    };
    let y = match args.get("y").and_then(parse_int) {
        Some(v) => v as i32,
        None => return NativeToolResult::text_only("Error: 'y' coordinate is required".to_string()),
    };

    let mut enigo = match Enigo::new(&Settings::default()) {
        Ok(e) => e,
        Err(e) => {
            return NativeToolResult::text_only(format!(
                "Error: Failed to init input simulation: {e}"
            ))
        }
    };

    if let Err(e) = enigo.move_mouse(x, y, Coordinate::Abs) {
        return NativeToolResult::text_only(format!("Error: move_mouse failed: {e}"));
    }

    NativeToolResult::text_only(format!("Mouse moved to ({x}, {y})"))
}

/// Scroll the mouse wheel at the current or specified position.
pub fn tool_scroll_screen(args: &Value) -> NativeToolResult {
    let amount = match args.get("amount").and_then(parse_int) {
        Some(v) => v as i32,
        None => {
            return NativeToolResult::text_only(
                "Error: 'amount' argument is required (positive=down, negative=up)".to_string(),
            )
        }
    };
    let horizontal = args
        .get("horizontal")
        .map(|v| parse_bool(v, false))
        .unwrap_or(false);
    let do_screenshot = args
        .get("screenshot")
        .map(|v| parse_bool(v, true))
        .unwrap_or(true);
    let delay_ms = args
        .get("delay_ms")
        .and_then(parse_int)
        .unwrap_or(300) as u64;

    let mut enigo = match Enigo::new(&Settings::default()) {
        Ok(e) => e,
        Err(e) => {
            return NativeToolResult::text_only(format!(
                "Error: Failed to init input simulation: {e}"
            ))
        }
    };

    // Move to position if specified
    if let (Some(x), Some(y)) = (
        args.get("x").and_then(parse_int),
        args.get("y").and_then(parse_int),
    ) {
        if let Err(e) = enigo.move_mouse(x as i32, y as i32, Coordinate::Abs) {
            return NativeToolResult::text_only(format!("Error: move_mouse failed: {e}"));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let axis = if horizontal {
        Axis::Horizontal
    } else {
        Axis::Vertical
    };

    if let Err(e) = enigo.scroll(amount, axis) {
        return NativeToolResult::text_only(format!("Error: scroll failed: {e}"));
    }

    let direction = if horizontal {
        if amount > 0 { "right" } else { "left" }
    } else if amount > 0 {
        "down"
    } else {
        "up"
    };
    let summary = format!("Scrolled {direction} by {} units", amount.abs());

    if do_screenshot {
        let mut result = capture_post_action_screenshot(delay_ms);
        result.text = format!("{summary}. {}", result.text);
        result
    } else {
        NativeToolResult::text_only(summary)
    }
}

// ─── Window listing (Windows-only) ───────────────────────────────────────────

#[cfg(windows)]
mod win32 {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

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
    pub const GMEM_MOVEABLE: u32 = 0x0002;

    #[repr(C)]
    pub struct POINT {
        pub x: i32,
        pub y: i32,
    }

    extern "system" {
        fn EnumWindows(cb: EnumWindowsProc, lparam: LPARAM) -> BOOL;
        fn GetWindowTextW(hwnd: HWND, buf: *mut u16, max_count: i32) -> i32;
        fn GetWindowTextLengthW(hwnd: HWND) -> i32;
        fn IsWindowVisible(hwnd: HWND) -> BOOL;
        fn IsIconic(hwnd: HWND) -> BOOL;
        fn IsZoomed(hwnd: HWND) -> BOOL;
        fn GetForegroundWindow() -> HWND;
        fn GetWindowRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
        fn GetWindowThreadProcessId(hwnd: HWND, pid: *mut DWORD) -> DWORD;
        fn OpenProcess(access: DWORD, inherit: BOOL, pid: DWORD) -> HANDLE;
        fn CloseHandle(handle: HANDLE) -> BOOL;
        fn QueryFullProcessImageNameW(
            process: HANDLE,
            flags: DWORD,
            name: *mut u16,
            size: *mut DWORD,
        ) -> BOOL;
        fn SetForegroundWindow(hwnd: HWND) -> BOOL;
        fn ShowWindow(hwnd: HWND, cmd: i32) -> BOOL;
        fn PostMessageW(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> BOOL;
        fn GetCursorPos(point: *mut POINT) -> BOOL;
        // Window positioning
        fn SetWindowPos(hwnd: HWND, after: HWND, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> BOOL;
        // Pixel color
        fn GetDC(hwnd: HWND) -> HDC;
        fn ReleaseDC(hwnd: HWND, hdc: HDC) -> i32;
        fn GetPixel(hdc: HDC, x: i32, y: i32) -> COLORREF;
        // Clipboard
        fn OpenClipboard(hwnd: HWND) -> BOOL;
        fn CloseClipboard() -> BOOL;
        fn EmptyClipboard() -> BOOL;
        fn GetClipboardData(format: u32) -> HANDLE;
        fn SetClipboardData(format: u32, mem: HANDLE) -> HANDLE;
        fn GlobalAlloc(flags: u32, bytes: usize) -> HANDLE;
        fn GlobalLock(mem: HANDLE) -> *mut u8;
        fn GlobalUnlock(mem: HANDLE) -> BOOL;
    }

    pub struct WindowInfo {
        pub title: String,
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

                results.push(WindowInfo {
                    title,
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

                if title.to_lowercase().contains(&lower_filter)
                    || process_name.to_lowercase().contains(&lower_filter)
                {
                    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                    GetWindowRect(hwnd, &mut rect);
                    return Some((hwnd, WindowInfo {
                        title,
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
            Some((hwnd, WindowInfo {
                title,
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
}

/// List all visible windows on the desktop with titles, positions, sizes, and process names.
#[cfg(windows)]
pub fn tool_list_windows(args: &Value) -> NativeToolResult {
    let filter = args
        .get("filter")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());

    let windows = win32::enumerate_windows();

    let filtered: Vec<&win32::WindowInfo> = windows
        .iter()
        .filter(|w| {
            if let Some(ref f) = filter {
                w.title.to_lowercase().contains(f)
                    || w.process_name.to_lowercase().contains(f)
            } else {
                true
            }
        })
        .collect();

    if filtered.is_empty() {
        let msg = if filter.is_some() {
            format!(
                "No visible windows match filter '{}'. Total visible windows: {}",
                filter.as_deref().unwrap_or(""),
                windows.len()
            )
        } else {
            "No visible windows found.".to_string()
        };
        return NativeToolResult::text_only(msg);
    }

    let mut output = format!("Found {} windows:\n", filtered.len());
    for (i, w) in filtered.iter().enumerate() {
        let mut state_parts = Vec::new();
        if w.minimized {
            state_parts.push("minimized");
        }
        if w.maximized {
            state_parts.push("maximized");
        }
        if w.focused {
            state_parts.push("focused");
        }
        let state = if state_parts.is_empty() {
            String::new()
        } else {
            format!(" [{}]", state_parts.join(", "))
        };
        let proc = if w.process_name.is_empty() {
            String::new()
        } else {
            format!(" ({})", w.process_name)
        };
        output.push_str(&format!(
            "  [{}] \"{}\"{} — {},{} {}x{}{}\n",
            i, w.title, proc, w.x, w.y, w.width, w.height, state
        ));
    }

    NativeToolResult::text_only(output)
}

#[cfg(not(windows))]
pub fn tool_list_windows(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: list_windows is only available on Windows".to_string())
}

/// Drag the mouse from one position to another.
pub fn tool_mouse_drag(args: &Value) -> NativeToolResult {
    let x1 = match args.get("from_x").and_then(parse_int) {
        Some(v) => v as i32,
        None => return NativeToolResult::text_only("Error: 'from_x' is required".to_string()),
    };
    let y1 = match args.get("from_y").and_then(parse_int) {
        Some(v) => v as i32,
        None => return NativeToolResult::text_only("Error: 'from_y' is required".to_string()),
    };
    let x2 = match args.get("to_x").and_then(parse_int) {
        Some(v) => v as i32,
        None => return NativeToolResult::text_only("Error: 'to_x' is required".to_string()),
    };
    let y2 = match args.get("to_y").and_then(parse_int) {
        Some(v) => v as i32,
        None => return NativeToolResult::text_only("Error: 'to_y' is required".to_string()),
    };
    let delay_ms = args
        .get("delay_ms")
        .and_then(parse_int)
        .unwrap_or(500) as u64;

    let mut enigo = match Enigo::new(&Settings::default()) {
        Ok(e) => e,
        Err(e) => return NativeToolResult::text_only(format!("Error: Failed to init input simulation: {e}")),
    };

    // Move to start position
    if let Err(e) = enigo.move_mouse(x1, y1, Coordinate::Abs) {
        return NativeToolResult::text_only(format!("Error: move to start failed: {e}"));
    }
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Press button down
    if let Err(e) = enigo.button(Button::Left, Direction::Press) {
        return NativeToolResult::text_only(format!("Error: mouse down failed: {e}"));
    }
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Move to end position
    if let Err(e) = enigo.move_mouse(x2, y2, Coordinate::Abs) {
        let _ = enigo.button(Button::Left, Direction::Release);
        return NativeToolResult::text_only(format!("Error: move to end failed: {e}"));
    }
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Release button
    if let Err(e) = enigo.button(Button::Left, Direction::Release) {
        return NativeToolResult::text_only(format!("Error: mouse up failed: {e}"));
    }

    let mut result = capture_post_action_screenshot(delay_ms);
    result.text = format!("Dragged from ({x1},{y1}) to ({x2},{y2}). {}", result.text);
    result
}

/// Get the current mouse cursor position.
#[cfg(windows)]
pub fn tool_get_cursor_position(_args: &Value) -> NativeToolResult {
    let (x, y) = win32::get_cursor_position();
    NativeToolResult::text_only(format!("Cursor position: ({x}, {y})"))
}

#[cfg(not(windows))]
pub fn tool_get_cursor_position(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: get_cursor_position is only available on Windows".to_string())
}

/// Focus (bring to front) a window by title or process name.
#[cfg(windows)]
pub fn tool_focus_window(args: &Value) -> NativeToolResult {
    let filter = match args.get("title").and_then(|v| v.as_str()) {
        Some(f) => f,
        None => return NativeToolResult::text_only("Error: 'title' argument is required".to_string()),
    };

    match win32::find_window_by_filter(filter) {
        Some((hwnd, info)) => {
            if win32::focus_window(hwnd) {
                NativeToolResult::text_only(format!(
                    "Focused window: \"{}\" ({})",
                    info.title, info.process_name
                ))
            } else {
                NativeToolResult::text_only(format!(
                    "Found \"{}\" but failed to bring to foreground (OS may block focus stealing)",
                    info.title
                ))
            }
        }
        None => NativeToolResult::text_only(format!("No visible window matches '{filter}'")),
    }
}

#[cfg(not(windows))]
pub fn tool_focus_window(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: focus_window is only available on Windows".to_string())
}

/// Minimize a window by title or process name.
#[cfg(windows)]
pub fn tool_minimize_window(args: &Value) -> NativeToolResult {
    let filter = match args.get("title").and_then(|v| v.as_str()) {
        Some(f) => f,
        None => return NativeToolResult::text_only("Error: 'title' argument is required".to_string()),
    };

    match win32::find_window_by_filter(filter) {
        Some((hwnd, info)) => {
            win32::minimize_window(hwnd);
            NativeToolResult::text_only(format!("Minimized: \"{}\"", info.title))
        }
        None => NativeToolResult::text_only(format!("No visible window matches '{filter}'")),
    }
}

#[cfg(not(windows))]
pub fn tool_minimize_window(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: minimize_window is only available on Windows".to_string())
}

/// Maximize a window by title or process name.
#[cfg(windows)]
pub fn tool_maximize_window(args: &Value) -> NativeToolResult {
    let filter = match args.get("title").and_then(|v| v.as_str()) {
        Some(f) => f,
        None => return NativeToolResult::text_only("Error: 'title' argument is required".to_string()),
    };

    match win32::find_window_by_filter(filter) {
        Some((hwnd, info)) => {
            win32::maximize_window(hwnd);
            NativeToolResult::text_only(format!("Maximized: \"{}\"", info.title))
        }
        None => NativeToolResult::text_only(format!("No visible window matches '{filter}'")),
    }
}

#[cfg(not(windows))]
pub fn tool_maximize_window(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: maximize_window is only available on Windows".to_string())
}

/// Close a window by title or process name (sends WM_CLOSE).
#[cfg(windows)]
pub fn tool_close_window(args: &Value) -> NativeToolResult {
    let filter = match args.get("title").and_then(|v| v.as_str()) {
        Some(f) => f,
        None => return NativeToolResult::text_only("Error: 'title' argument is required".to_string()),
    };

    match win32::find_window_by_filter(filter) {
        Some((hwnd, info)) => {
            if win32::close_window(hwnd) {
                NativeToolResult::text_only(format!("Sent close to: \"{}\"", info.title))
            } else {
                NativeToolResult::text_only(format!("Failed to close: \"{}\"", info.title))
            }
        }
        None => NativeToolResult::text_only(format!("No visible window matches '{filter}'")),
    }
}

#[cfg(not(windows))]
pub fn tool_close_window(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: close_window is only available on Windows".to_string())
}

/// Read text from the system clipboard.
#[cfg(windows)]
pub fn tool_read_clipboard(_args: &Value) -> NativeToolResult {
    match win32::read_clipboard() {
        Ok(text) => {
            let summary = if text.len() > 200 {
                format!("Clipboard ({} chars): \"{}...\"", text.len(), &text[..200])
            } else {
                format!("Clipboard: \"{text}\"")
            };
            NativeToolResult::text_only(summary)
        }
        Err(e) => NativeToolResult::text_only(format!("Error: {e}")),
    }
}

#[cfg(not(windows))]
pub fn tool_read_clipboard(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: read_clipboard is only available on Windows".to_string())
}

/// Write text to the system clipboard.
#[cfg(windows)]
pub fn tool_write_clipboard(args: &Value) -> NativeToolResult {
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return NativeToolResult::text_only("Error: 'text' argument is required".to_string()),
    };

    match win32::write_clipboard(text) {
        Ok(()) => {
            let summary = if text.len() > 50 {
                format!("Wrote {} chars to clipboard", text.len())
            } else {
                format!("Wrote to clipboard: \"{text}\"")
            };
            NativeToolResult::text_only(summary)
        }
        Err(e) => NativeToolResult::text_only(format!("Error: {e}")),
    }
}

#[cfg(not(windows))]
pub fn tool_write_clipboard(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: write_clipboard is only available on Windows".to_string())
}

// ─── New tools: Group A (pure FFI) ───────────────────────────────────────────

/// Resize and/or move a window by title or process name.
#[cfg(windows)]
pub fn tool_resize_window(args: &Value) -> NativeToolResult {
    let filter = match args.get("title").and_then(|v| v.as_str()) {
        Some(f) => f,
        None => return NativeToolResult::text_only("Error: 'title' argument is required".to_string()),
    };
    let x = args.get("x").and_then(parse_int).map(|v| v as i32);
    let y = args.get("y").and_then(parse_int).map(|v| v as i32);
    let w = args.get("width").and_then(parse_int).map(|v| v as i32);
    let h = args.get("height").and_then(parse_int).map(|v| v as i32);

    if x.is_none() && y.is_none() && w.is_none() && h.is_none() {
        return NativeToolResult::text_only("Error: at least one of x, y, width, height is required".to_string());
    }

    match win32::find_window_by_filter(filter) {
        Some((hwnd, info)) => {
            if win32::resize_window(hwnd, x, y, w, h) {
                let mut parts = Vec::new();
                if let (Some(px), Some(py)) = (x, y) {
                    parts.push(format!("moved to ({px},{py})"));
                }
                if let (Some(pw), Some(ph)) = (w, h) {
                    parts.push(format!("resized to {pw}x{ph}"));
                }
                NativeToolResult::text_only(format!(
                    "Window \"{}\": {}", info.title, parts.join(", ")
                ))
            } else {
                NativeToolResult::text_only(format!("Failed to resize/move: \"{}\"", info.title))
            }
        }
        None => NativeToolResult::text_only(format!("No visible window matches '{filter}'")),
    }
}

#[cfg(not(windows))]
pub fn tool_resize_window(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: resize_window is only available on Windows".to_string())
}

/// Get information about the currently active (foreground) window.
#[cfg(windows)]
pub fn tool_get_active_window(_args: &Value) -> NativeToolResult {
    match win32::get_active_window_info() {
        Some((_hwnd, info)) => {
            NativeToolResult::text_only(format!(
                "Active window: \"{}\" ({}) at {},{} size {}x{}{}{}",
                info.title, info.process_name, info.x, info.y, info.width, info.height,
                if info.minimized { " [minimized]" } else { "" },
                if info.maximized { " [maximized]" } else { "" },
            ))
        }
        None => NativeToolResult::text_only("No active window found".to_string()),
    }
}

#[cfg(not(windows))]
pub fn tool_get_active_window(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: get_active_window is only available on Windows".to_string())
}

/// Wait for a window to appear by title or process name (polling).
#[cfg(windows)]
pub fn tool_wait_for_window(args: &Value) -> NativeToolResult {
    let filter = match args.get("title").and_then(|v| v.as_str()) {
        Some(f) => f,
        None => return NativeToolResult::text_only("Error: 'title' argument is required".to_string()),
    };
    let timeout_ms = args.get("timeout_ms").and_then(parse_int).unwrap_or(10000).min(60000) as u64;
    let poll_ms = args.get("poll_ms").and_then(parse_int).unwrap_or(200).max(50) as u64;

    let start = std::time::Instant::now();
    loop {
        if let Some((_hwnd, info)) = win32::find_window_by_filter(filter) {
            return NativeToolResult::text_only(format!(
                "Found window: \"{}\" ({}) at {},{} size {}x{} (waited {}ms)",
                info.title, info.process_name, info.x, info.y, info.width, info.height,
                start.elapsed().as_millis()
            ));
        }
        if start.elapsed().as_millis() as u64 >= timeout_ms {
            return NativeToolResult::text_only(format!(
                "Timeout: no window matching '{}' appeared within {}ms", filter, timeout_ms
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(poll_ms));
    }
}

#[cfg(not(windows))]
pub fn tool_wait_for_window(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: wait_for_window is only available on Windows".to_string())
}

/// Get the color of a pixel at screen coordinates.
#[cfg(windows)]
pub fn tool_get_pixel_color(args: &Value) -> NativeToolResult {
    let x = match args.get("x").and_then(parse_int) {
        Some(v) => v as i32,
        None => return NativeToolResult::text_only("Error: 'x' coordinate is required".to_string()),
    };
    let y = match args.get("y").and_then(parse_int) {
        Some(v) => v as i32,
        None => return NativeToolResult::text_only("Error: 'y' coordinate is required".to_string()),
    };
    match win32::get_pixel_color(x, y) {
        Ok((r, g, b)) => NativeToolResult::text_only(format!(
            "Pixel at ({x},{y}): rgb({r},{g},{b}) = #{r:02X}{g:02X}{b:02X}"
        )),
        Err(e) => NativeToolResult::text_only(format!("Error: {e}")),
    }
}

#[cfg(not(windows))]
pub fn tool_get_pixel_color(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: get_pixel_color is only available on Windows".to_string())
}

/// Click at coordinates relative to a window's top-left corner.
#[cfg(windows)]
pub fn tool_click_window_relative(args: &Value) -> NativeToolResult {
    let filter = match args.get("title").and_then(|v| v.as_str()) {
        Some(f) => f,
        None => return NativeToolResult::text_only("Error: 'title' argument is required".to_string()),
    };
    let rel_x = match args.get("x").and_then(parse_int) {
        Some(v) => v as i32,
        None => return NativeToolResult::text_only("Error: 'x' coordinate is required".to_string()),
    };
    let rel_y = match args.get("y").and_then(parse_int) {
        Some(v) => v as i32,
        None => return NativeToolResult::text_only("Error: 'y' coordinate is required".to_string()),
    };

    match win32::find_window_by_filter(filter) {
        Some((hwnd, info)) => {
            // Focus window first
            win32::focus_window(hwnd);
            std::thread::sleep(std::time::Duration::from_millis(100));

            // Convert relative → absolute
            let abs_x = info.x + rel_x;
            let abs_y = info.y + rel_y;

            // Build args for click_screen
            let button = args.get("button").and_then(|v| v.as_str()).unwrap_or("left");
            let delay_ms = args.get("delay_ms").and_then(parse_int).unwrap_or(500);
            let click_args = serde_json::json!({
                "x": abs_x, "y": abs_y, "button": button, "delay_ms": delay_ms
            });
            let mut result = tool_click_screen(&click_args);
            result.text = format!(
                "Clicked {button} at relative ({rel_x},{rel_y}) → absolute ({abs_x},{abs_y}) in \"{}\". {}",
                info.title, result.text
            );
            result
        }
        None => NativeToolResult::text_only(format!("No visible window matches '{filter}'")),
    }
}

#[cfg(not(windows))]
pub fn tool_click_window_relative(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: click_window_relative is only available on Windows".to_string())
}

/// List all monitors with their properties.
pub fn tool_list_monitors(args: &Value) -> NativeToolResult {
    let index_filter = args.get("index").and_then(parse_int).map(|v| v as usize);

    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return NativeToolResult::text_only(format!("Error enumerating monitors: {e}")),
    };

    if monitors.is_empty() {
        return NativeToolResult::text_only("No monitors found".to_string());
    }

    if let Some(idx) = index_filter {
        if idx >= monitors.len() {
            return NativeToolResult::text_only(format!(
                "Error: monitor index {idx} out of range (0..{})", monitors.len()
            ));
        }
        let m = &monitors[idx];
        return NativeToolResult::text_only(format!(
            "Monitor {idx}: \"{}\" {}x{} at ({},{}) scale={:.1} primary={}",
            m.name().unwrap_or_else(|_| "Unknown".to_string()),
            m.width().unwrap_or(0), m.height().unwrap_or(0),
            m.x().unwrap_or(0), m.y().unwrap_or(0),
            m.scale_factor().unwrap_or(1.0),
            m.is_primary().unwrap_or(false)
        ));
    }

    let mut output = format!("Found {} monitors:\n", monitors.len());
    for (i, m) in monitors.iter().enumerate() {
        output.push_str(&format!(
            "  [{}] \"{}\" {}x{} at ({},{}) scale={:.1}{}\n",
            i, m.name().unwrap_or_else(|_| "Unknown".to_string()),
            m.width().unwrap_or(0), m.height().unwrap_or(0),
            m.x().unwrap_or(0), m.y().unwrap_or(0),
            m.scale_factor().unwrap_or(1.0),
            if m.is_primary().unwrap_or(false) { " [primary]" } else { "" }
        ));
    }
    NativeToolResult::text_only(output)
}

// ─── New tools: Group B (image processing) ───────────────────────────────────

/// Capture a screenshot of a specific screen region.
pub fn tool_screenshot_region(args: &Value) -> NativeToolResult {
    let x = match args.get("x").and_then(parse_int) {
        Some(v) => v as u32,
        None => return NativeToolResult::text_only("Error: 'x' is required".to_string()),
    };
    let y = match args.get("y").and_then(parse_int) {
        Some(v) => v as u32,
        None => return NativeToolResult::text_only("Error: 'y' is required".to_string()),
    };
    let w = match args.get("width").and_then(parse_int) {
        Some(v) => v as u32,
        None => return NativeToolResult::text_only("Error: 'width' is required".to_string()),
    };
    let h = match args.get("height").and_then(parse_int) {
        Some(v) => v as u32,
        None => return NativeToolResult::text_only("Error: 'height' is required".to_string()),
    };
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
    };
    if monitor_idx >= monitors.len() {
        return NativeToolResult::text_only(format!(
            "Error: monitor index {monitor_idx} out of range (0..{})", monitors.len()
        ));
    }

    let img = match monitors[monitor_idx].capture_image() {
        Ok(i) => i,
        Err(e) => return NativeToolResult::text_only(format!("Error capturing screen: {e}")),
    };

    // Crop the image manually using the image crate
    let img_w = img.width();
    let img_h = img.height();
    if x + w > img_w || y + h > img_h {
        return NativeToolResult::text_only(format!(
            "Error: region ({x},{y} {w}x{h}) exceeds screen size ({img_w}x{img_h})"
        ));
    }
    let cropped: image::RgbaImage = image::imageops::crop_imm(&img, x, y, w, h).to_image();

    // Encode to PNG
    let mut png_buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut png_buf);
    if let Err(e) = cropped.write_to(&mut cursor, image::ImageFormat::Png) {
        return NativeToolResult::text_only(format!("Error encoding PNG: {e}"));
    }

    NativeToolResult::with_image(
        format!("Screenshot region: ({x},{y}) {w}x{h} from monitor {monitor_idx}"),
        png_buf,
    )
}

/// Compare current screen to a saved baseline, reporting changed regions.
pub fn tool_screenshot_diff(args: &Value) -> NativeToolResult {
    use std::sync::Mutex;
    lazy_static::lazy_static! {
        static ref BASELINE: Mutex<Option<Vec<u8>>> = Mutex::new(None);
    }

    let save_baseline = args.get("save_baseline").map(|v| parse_bool(v, false)).unwrap_or(false);
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
    };
    if monitor_idx >= monitors.len() {
        return NativeToolResult::text_only(format!(
            "Error: monitor index {monitor_idx} out of range (0..{})", monitors.len()
        ));
    }

    let img = match monitors[monitor_idx].capture_image() {
        Ok(i) => i,
        Err(e) => return NativeToolResult::text_only(format!("Error capturing screen: {e}")),
    };
    let current_bytes = img.as_raw().clone();

    let w = img.width();
    let h = img.height();

    if save_baseline {
        let mut lock = BASELINE.lock().unwrap();
        *lock = Some(current_bytes);
        return NativeToolResult::text_only(format!(
            "Baseline saved: {w}x{h} ({} bytes). Call again without save_baseline to compare.",
            lock.as_ref().map(|b| b.len()).unwrap_or(0)
        ));
    }

    // Compare with baseline
    let lock = BASELINE.lock().unwrap();
    let baseline = match lock.as_ref() {
        Some(b) => b,
        None => return NativeToolResult::text_only(
            "Error: no baseline saved. Call with save_baseline=true first.".to_string()
        ),
    };

    if current_bytes.len() != baseline.len() {
        return NativeToolResult::text_only(format!(
            "Error: screen resolution changed since baseline (baseline {} bytes, current {} bytes)",
            baseline.len(), current_bytes.len()
        ));
    }
    let mut changed_pixels = 0u64;
    let total_pixels = (w as u64) * (h as u64);
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let threshold: i16 = 10;

    // RGBA: 4 bytes per pixel
    for py in 0..h {
        for px in 0..w {
            let idx = ((py * w + px) * 4) as usize;
            let dr = (current_bytes[idx] as i16 - baseline[idx] as i16).abs();
            let dg = (current_bytes[idx + 1] as i16 - baseline[idx + 1] as i16).abs();
            let db = (current_bytes[idx + 2] as i16 - baseline[idx + 2] as i16).abs();
            if dr > threshold || dg > threshold || db > threshold {
                changed_pixels += 1;
                min_x = min_x.min(px);
                min_y = min_y.min(py);
                max_x = max_x.max(px);
                max_y = max_y.max(py);
            }
        }
    }

    if changed_pixels == 0 {
        return NativeToolResult::text_only("No changes detected — screen matches baseline.".to_string());
    }

    let pct = (changed_pixels as f64 / total_pixels as f64) * 100.0;
    NativeToolResult::text_only(format!(
        "Screen diff: {:.2}% pixels changed ({changed_pixels}/{total_pixels}). Changed region: ({min_x},{min_y}) to ({max_x},{max_y}) = {}x{}",
        pct, max_x - min_x + 1, max_y - min_y + 1
    ))
}

// ─── New tools: Group C (Windows crate features) ─────────────────────────────

/// OCR: extract text from the screen using Windows.Media.Ocr.
#[cfg(windows)]
pub fn tool_ocr_screen(args: &Value) -> NativeToolResult {
    // Region params (optional — full screen if omitted)
    let region_x = args.get("x").and_then(parse_int).map(|v| v as u32);
    let region_y = args.get("y").and_then(parse_int).map(|v| v as u32);
    let region_w = args.get("width").and_then(parse_int).map(|v| v as u32);
    let region_h = args.get("height").and_then(parse_int).map(|v| v as u32);
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    // Capture screen
    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
    };
    if monitor_idx >= monitors.len() {
        return NativeToolResult::text_only(format!(
            "Error: monitor {monitor_idx} out of range (0..{})", monitors.len()
        ));
    }
    let img = match monitors[monitor_idx].capture_image() {
        Ok(i) => i,
        Err(e) => return NativeToolResult::text_only(format!("Error capturing: {e}")),
    };

    // Crop if region specified
    let full_w = img.width();
    let full_h = img.height();
    let (work_img, crop_x, crop_y) = if let (Some(rx), Some(ry), Some(rw), Some(rh)) = (region_x, region_y, region_w, region_h) {
        if rx + rw > full_w || ry + rh > full_h {
            return NativeToolResult::text_only(format!(
                "Error: region ({rx},{ry} {rw}x{rh}) exceeds screen ({full_w}x{full_h})"
            ));
        }
        let cropped: image::RgbaImage = image::imageops::crop_imm(&img, rx, ry, rw, rh).to_image();
        (cropped, rx, ry)
    } else {
        (img, 0u32, 0u32)
    };

    let work_w = work_img.width();
    let work_h = work_img.height();

    // Run OCR on a temporary STA thread (WinRT requires STA)
    let result = std::thread::spawn(move || {
        ocr_image_winrt(&work_img)
    }).join().unwrap_or_else(|_| Err("OCR thread panicked".to_string()));

    match result {
        Ok(text) => {
            let region_info = if region_x.is_some() {
                format!(" (region {crop_x},{crop_y} {work_w}x{work_h})")
            } else {
                format!(" ({full_w}x{full_h})")
            };
            if text.is_empty() {
                NativeToolResult::text_only(format!("OCR{region_info}: no text detected"))
            } else {
                let line_count = text.lines().count();
                NativeToolResult::text_only(format!(
                    "OCR{region_info}: {line_count} lines\n{text}"
                ))
            }
        }
        Err(e) => NativeToolResult::text_only(format!("OCR error: {e}")),
    }
}

/// Internal: run OCR via Windows.Media.Ocr WinRT API. Must be called from STA thread.
#[cfg(windows)]
fn ocr_image_winrt(img: &image::RgbaImage) -> Result<String, String> {
    use windows::Media::Ocr::OcrEngine;
    use windows::Graphics::Imaging::{SoftwareBitmap, BitmapPixelFormat, BitmapAlphaMode};
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::core::HRESULT;

    // Init COM as STA
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        // S_OK (0) or S_FALSE (1, already init) are fine
        if hr.is_err() && hr != HRESULT(1) {
            return Err(format!("CoInitializeEx failed: {hr:?}"));
        }
    }

    let (w, h) = (img.width(), img.height());

    // Convert RGBA → BGRA (Windows expects BGRA8)
    let mut bgra = img.as_raw().clone();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2); // R ↔ B
    }

    // Create SoftwareBitmap from BGRA pixel data (CreateWithAlpha for 4-arg version)
    let bitmap = SoftwareBitmap::CreateWithAlpha(
        BitmapPixelFormat::Bgra8,
        w as i32,
        h as i32,
        BitmapAlphaMode::Premultiplied,
    ).map_err(|e| format!("SoftwareBitmap::CreateWithAlpha: {e}"))?;

    // Write pixel data to IBuffer via DataWriter + synchronous wait
    let stream = InMemoryRandomAccessStream::new()
        .map_err(|e| format!("InMemoryRandomAccessStream: {e}"))?;
    let writer = DataWriter::CreateDataWriter(&stream)
        .map_err(|e| format!("DataWriter: {e}"))?;
    writer.WriteBytes(&bgra)
        .map_err(|e| format!("WriteBytes: {e}"))?;

    // Store + Flush: in-memory stream ops, pump STA messages to let them complete
    let store_op = writer.StoreAsync()
        .map_err(|e| format!("StoreAsync: {e}"))?;
    pump_sta_messages(100); // in-memory, completes in <1ms
    store_op.GetResults().map_err(|e| format!("StoreAsync result: {e}"))?;

    let flush_op = writer.FlushAsync()
        .map_err(|e| format!("FlushAsync: {e}"))?;
    pump_sta_messages(100);
    flush_op.GetResults().map_err(|e| format!("FlushAsync result: {e}"))?;

    // Read back as IBuffer
    stream.Seek(0).map_err(|e| format!("Seek: {e}"))?;
    let reader = windows::Storage::Streams::DataReader::CreateDataReader(&stream)
        .map_err(|e| format!("DataReader: {e}"))?;
    let load_op = reader.LoadAsync(bgra.len() as u32)
        .map_err(|e| format!("LoadAsync: {e}"))?;
    pump_sta_messages(100);
    load_op.GetResults().map_err(|e| format!("LoadAsync result: {e}"))?;

    let buffer = reader.ReadBuffer(bgra.len() as u32)
        .map_err(|e| format!("ReadBuffer: {e}"))?;

    bitmap.CopyFromBuffer(&buffer)
        .map_err(|e| format!("CopyFromBuffer: {e}"))?;

    // Create OCR engine from user profile languages
    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|e| format!("OcrEngine: {e}"))?;

    // Run recognition — may take 50-500ms depending on image size
    let recognize_op = engine.RecognizeAsync(&bitmap)
        .map_err(|e| format!("RecognizeAsync: {e}"))?;
    pump_sta_messages(5000); // OCR can take a few seconds for large images
    let ocr_result = recognize_op.GetResults()
        .map_err(|e| format!("RecognizeAsync result: {e}"))?;

    let text = ocr_result.Text()
        .map_err(|e| format!("Text: {e}"))?
        .to_string();

    Ok(text)
}

/// Pump STA message loop for a duration (ms). Required for WinRT async ops to complete on STA.
#[cfg(windows)]
fn pump_sta_messages(duration_ms: u64) {
    #[repr(C)]
    struct MSG([u8; 48]); // sizeof(MSG) = 48 on x64

    #[link(name = "user32")]
    unsafe extern "system" {
        fn PeekMessageW(msg: *mut MSG, hwnd: isize, min: u32, max: u32, remove: u32) -> i32;
        fn TranslateMessage(msg: *const MSG) -> i32;
        fn DispatchMessageW(msg: *const MSG) -> isize;
    }

    let start = std::time::Instant::now();
    let mut msg = MSG([0u8; 48]);

    while start.elapsed().as_millis() < duration_ms as u128 {
        unsafe {
            while PeekMessageW(&mut msg, 0, 0, 0, 1) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[cfg(not(windows))]
pub fn tool_ocr_screen(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: ocr_screen is only available on Windows".to_string())
}

/// Get the UI element tree of a window using UI Automation.
#[cfg(windows)]
pub fn tool_get_ui_tree(args: &Value) -> NativeToolResult {
    let title_filter = args.get("title").and_then(|v| v.as_str());
    let max_depth = args.get("depth").and_then(parse_int).unwrap_or(3).min(8) as usize;

    // Get target window HWND
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return NativeToolResult::text_only(format!("No window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return NativeToolResult::text_only("No active window".to_string()),
        }
    };

    // Run on STA thread (COM UI Automation requires it)
    let result = std::thread::spawn(move || {
        ui_tree_winrt(hwnd, max_depth)
    }).join().unwrap_or_else(|_| Err("UI tree thread panicked".to_string()));

    match result {
        Ok(tree) => NativeToolResult::text_only(tree),
        Err(e) => NativeToolResult::text_only(format!("UI tree error: {e}")),
    }
}

/// Internal: traverse UI Automation tree via COM.
#[cfg(windows)]
fn ui_tree_winrt(hwnd: isize, max_depth: usize) -> Result<String, String> {
    use windows::Win32::UI::Accessibility::*;
    use windows::Win32::System::Com::{CoInitializeEx, CoCreateInstance, COINIT_APARTMENTTHREADED, CLSCTX_INPROC_SERVER};
    use windows::Win32::Foundation::HWND as WIN32_HWND;
    use windows::core::HRESULT;

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() && hr != HRESULT(1) {
            return Err(format!("CoInitializeEx failed: {hr:?}"));
        }
    }

    let automation: IUIAutomation = unsafe {
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
    }.map_err(|e| format!("CoCreateInstance UIAutomation: {e}"))?;

    let root = unsafe {
        automation.ElementFromHandle(WIN32_HWND(hwnd as *mut _))
    }.map_err(|e| format!("ElementFromHandle: {e}"))?;

    let walker = unsafe {
        automation.ControlViewWalker()
    }.map_err(|e| format!("ControlViewWalker: {e}"))?;

    let mut output = String::new();
    let mut total_chars = 0usize;
    const MAX_CHARS: usize = 50_000;

    // Get root element info
    if let Ok(info) = get_element_info(&root) {
        output.push_str(&info);
        output.push('\n');
        total_chars += info.len() + 1;
    }

    // Recursive traversal
    fn traverse(
        walker: &IUIAutomationTreeWalker,
        parent: &IUIAutomationElement,
        depth: usize,
        max_depth: usize,
        output: &mut String,
        total_chars: &mut usize,
    ) {
        if depth >= max_depth || *total_chars >= MAX_CHARS {
            return;
        }
        let first_child = unsafe { walker.GetFirstChildElement(parent) };
        let mut current = match first_child {
            Ok(c) => c,
            Err(_) => return,
        };
        loop {
            let indent = "  ".repeat(depth);
            if let Ok(info) = get_element_info(&current) {
                let line = format!("{indent}{info}\n");
                *total_chars += line.len();
                output.push_str(&line);
                if *total_chars >= MAX_CHARS {
                    output.push_str("... (truncated at 50KB)\n");
                    return;
                }
            }
            traverse(walker, &current, depth + 1, max_depth, output, total_chars);
            match unsafe { walker.GetNextSiblingElement(&current) } {
                Ok(next) => current = next,
                Err(_) => break,
            }
        }
    }

    traverse(&walker, &root, 1, max_depth, &mut output, &mut total_chars);

    if output.is_empty() {
        Ok("(empty UI tree)".to_string())
    } else {
        Ok(output)
    }
}

#[cfg(windows)]
fn get_element_info(elem: &windows::Win32::UI::Accessibility::IUIAutomationElement) -> Result<String, String> {
    let name = unsafe { elem.CurrentName() }
        .map(|s| s.to_string())
        .unwrap_or_default();
    let control_type = unsafe { elem.CurrentControlType() }
        .map(|ct| control_type_name(ct.0))
        .unwrap_or_else(|_| "Unknown".to_string());

    if name.is_empty() {
        Ok(format!("[{control_type}]"))
    } else {
        // Truncate long names
        let display_name = if name.len() > 80 {
            format!("{}...", &name[..80])
        } else {
            name
        };
        Ok(format!("[{control_type}] \"{display_name}\""))
    }
}

#[cfg(windows)]
fn control_type_name(id: i32) -> String {
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
    }.to_string()
}

#[cfg(not(windows))]
pub fn tool_get_ui_tree(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: get_ui_tree is only available on Windows".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_key_combo_single() {
        let (mods, key) = parse_key_combo("enter").unwrap();
        assert!(mods.is_empty());
        assert!(matches!(key, Key::Return));
    }

    #[test]
    fn test_parse_key_combo_with_modifier() {
        let (mods, key) = parse_key_combo("ctrl+c").unwrap();
        assert_eq!(mods.len(), 1);
        assert!(matches!(mods[0], Key::Control));
        assert!(matches!(key, Key::Unicode('c')));
    }

    #[test]
    fn test_parse_key_combo_multiple_modifiers() {
        let (mods, key) = parse_key_combo("ctrl+shift+s").unwrap();
        assert_eq!(mods.len(), 2);
        assert!(matches!(mods[0], Key::Control));
        assert!(matches!(mods[1], Key::Shift));
        assert!(matches!(key, Key::Unicode('s')));
    }

    #[test]
    fn test_parse_key_combo_fkey() {
        let (mods, key) = parse_key_combo("alt+f4").unwrap();
        assert_eq!(mods.len(), 1);
        assert!(matches!(mods[0], Key::Alt));
        assert!(matches!(key, Key::F4));
    }

    #[test]
    fn test_parse_key_combo_unknown() {
        assert!(parse_key_combo("ctrl+unknownkey").is_err());
    }

    #[test]
    fn test_str_to_key_special() {
        assert!(matches!(str_to_key("tab"), Ok(Key::Tab)));
        assert!(matches!(str_to_key("escape"), Ok(Key::Escape)));
        assert!(matches!(str_to_key("backspace"), Ok(Key::Backspace)));
        assert!(matches!(str_to_key("space"), Ok(Key::Space)));
    }

    #[test]
    fn test_str_to_key_char() {
        assert!(matches!(str_to_key("a"), Ok(Key::Unicode('a'))));
        assert!(matches!(str_to_key("1"), Ok(Key::Unicode('1'))));
    }
}
