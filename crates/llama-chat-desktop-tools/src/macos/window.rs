//! macOS window management helpers (via osascript/AppleScript).

use std::process::Command;
use super::{HWND, BOOL, RECT, WindowInfo};
use super::run_osascript;

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
    fn normalized_basename(value: &str) -> String {
        value.rsplit(['\\', '/'])
            .next()
            .unwrap_or(value)
            .trim()
            .trim_end_matches(".app")
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

    let windows = enumerate_windows();
    let lower = filter.to_lowercase();
    let mut best_match: Option<(i32, HWND, WindowInfo)> = None;

    for (i, w) in windows.into_iter().enumerate() {
        if let Some(mut score) = match_score(&lower, &w.title, &w.process_name) {
            if w.focused {
                score += 10;
            }
            if !w.minimized {
                score += 5;
            }

            let should_replace = best_match
                .as_ref()
                .map(|(best_score, _, _)| score > *best_score)
                .unwrap_or(true);
            if should_replace {
                best_match = Some((score, i as HWND, w));
            }
        }
    }

    best_match.map(|(_, hwnd, info)| (hwnd, info))
}

pub fn find_window_by_pid(pid: u32) -> Option<(HWND, WindowInfo)> {
    enumerate_windows()
        .into_iter()
        .enumerate()
        .find_map(|(idx, info)| (info.pid == pid).then_some((idx as HWND, info)))
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
        match Command::new("osascript")
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
        let mx = m.x().unwrap_or(0);
        let my = m.y().unwrap_or(0);
        let mw = m.width().unwrap_or(0) as i32;
        let mh = m.height().unwrap_or(0) as i32;
        Ok(RECT {
            left: mx,
            top: my,
            right: mx + mw,
            bottom: my + mh,
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
    let mx = (x - monitor.x().unwrap_or(0)) as u32;
    let my = (y - monitor.y().unwrap_or(0)) as u32;
    if mx < img.width() && my < img.height() {
        let pixel = img.get_pixel(mx, my);
        Ok((pixel[0], pixel[1], pixel[2]))
    } else {
        Err(format!("Coordinates ({x}, {y}) out of screen bounds"))
    }
}

pub fn get_window_class_name(_hwnd: HWND) -> String {
    String::new() // No class names on macOS
}

pub fn get_system_dpi_scale() -> f64 {
    if let Ok(monitors) = xcap::Monitor::all() {
        if let Some(m) = monitors.first() {
            return m.scale_factor().unwrap_or(1.0) as f64;
        }
    }
    1.0
}

pub fn find_child_window(_parent: HWND, _class_name: &str) -> HWND {
    0 // Not applicable on macOS
}

/// Stub: returns 0 (not maximized) on macOS.
pub unsafe fn IsZoomed(_hwnd: HWND) -> BOOL {
    0
}

/// Stub: no-op on macOS. Returns 1 (success).
pub unsafe fn ShowWindow(_hwnd: HWND, _cmd_show: i32) -> BOOL {
    1
}

/// Stub: no-op on macOS. Returns 1 (success).
pub unsafe fn SetWindowPos(_hwnd: HWND, _insert_after: isize, _x: i32, _y: i32, _cx: i32, _cy: i32, _flags: u32) -> BOOL {
    1
}

/// Stub: no-op on macOS. Returns 1 (success).
pub unsafe fn PostMessageW(_hwnd: HWND, _msg: u32, _wparam: usize, _lparam: isize) -> BOOL {
    1
}

/// Stub: no-op on macOS. Returns 1 (success).
pub unsafe fn SetForegroundWindow(_hwnd: HWND) -> BOOL {
    1
}

/// Stub: no-op on macOS. Returns 0.
pub unsafe fn GetForegroundWindow() -> HWND {
    0
}
