//! Window management tools: focus, resize, move, snap, topmost, close, layout save/restore.

use serde_json::Value;

use super::NativeToolResult;
use super::{parse_bool, parse_int, tool_click_screen};

use super::win32;

// ─── Shared helper ────────────────────────────────────────────────────────────

/// Resolve a window HWND + info from `title` or `pid` args. Returns `Err(NativeToolResult)`
/// (an error result) if the window cannot be found or args are invalid.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub(super) fn resolve_window_target(
    args: &Value,
    tool_name: &str,
) -> Result<(win32::HWND, win32::WindowInfo), NativeToolResult> {
    let title_filter = args.get("title").and_then(|v| v.as_str());
    let pid_filter = args.get("pid").and_then(parse_int).map(|v| v as u32);

    if title_filter.is_none() && pid_filter.is_none() {
        return Err(super::tool_error(tool_name, "'title' or 'pid' argument is required"));
    }

    if let Some(target_pid) = pid_filter {
        if let Some(result) = win32::find_window_by_pid(target_pid) {
            return Ok(result);
        }
        if title_filter.is_none() {
            return Err(NativeToolResult::text_only(format!(
                "No visible window found for PID {target_pid}"
            )));
        }
    }

    if let Some(filter) = title_filter {
        return match win32::find_window_by_filter(filter) {
            Some(result) => Ok(result),
            None => Err(NativeToolResult::text_only(format!(
                "No visible window matches '{filter}'"
            ))),
        };
    }

    Err(NativeToolResult::text_only("No visible window found".to_string()))
}

// ─── focus_window ─────────────────────────────────────────────────────────────

/// Focus (bring to front) a window by title, process name, or PID.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_focus_window(args: &Value) -> NativeToolResult {
    let title_filter = args.get("title").and_then(|v| v.as_str());
    let pid_filter = args.get("pid").and_then(parse_int).map(|v| v as u32);

    if title_filter.is_none() && pid_filter.is_none() {
        return super::tool_error("focus_window", "'title' or 'pid' argument is required");
    }

    if let Some(target_pid) = pid_filter {
        if let Some((hwnd, info)) = win32::find_window_by_pid(target_pid) {
            if win32::focus_window(hwnd) {
                return NativeToolResult::text_only(format!(
                    "Focused window: \"{}\" ({}) pid={}",
                    info.title, info.process_name, target_pid
                ));
            }
            return NativeToolResult::text_only(format!(
                "Found \"{}\" (pid={}) but failed to bring to foreground",
                info.title, target_pid
            ));
        }
        return super::tool_error("focus_window", format!("no visible window for PID {target_pid}"));
    }

    let filter = match title_filter {
        Some(f) => f,
        None => return super::tool_error("focus_window", "'title' or 'pid' is required"),
    };
    match win32::find_window_by_filter(filter) {
        Some((hwnd, info)) => {
            if win32::focus_window(hwnd) {
                NativeToolResult::text_only(format!(
                    "Focused window: \"{}\" ({}) pid={}",
                    info.title, info.process_name, info.pid
                ))
            } else {
                NativeToolResult::text_only(format!(
                    "Found \"{}\" but failed to bring to foreground (OS may block focus stealing)",
                    info.title
                ))
            }
        }
        None => super::tool_error("focus_window", format!("no window matches '{filter}'")),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_focus_window(_args: &Value) -> NativeToolResult {
    super::tool_error("focus_window", "not available on this platform")
}

// ─── minimize_window ─────────────────────────────────────────────────────────

/// Minimize a window by title or process name.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_minimize_window(args: &Value) -> NativeToolResult {
    match resolve_window_target(args, "minimize_window") {
        Ok((hwnd, info)) => {
            win32::minimize_window(hwnd);
            NativeToolResult::text_only(format!("Minimized: \"{}\" pid={}", info.title, info.pid))
        }
        Err(result) => result,
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_minimize_window(_args: &Value) -> NativeToolResult {
    super::tool_error("minimize_window", "not available on this platform")
}

// ─── maximize_window ─────────────────────────────────────────────────────────

/// Maximize a window by title or process name.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_maximize_window(args: &Value) -> NativeToolResult {
    match resolve_window_target(args, "maximize_window") {
        Ok((hwnd, info)) => {
            win32::maximize_window(hwnd);
            NativeToolResult::text_only(format!("Maximized: \"{}\" pid={}", info.title, info.pid))
        }
        Err(result) => result,
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_maximize_window(_args: &Value) -> NativeToolResult {
    super::tool_error("maximize_window", "not available on this platform")
}

// ─── close_window ────────────────────────────────────────────────────────────

/// Close a window by title or process name (sends WM_CLOSE).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_close_window(args: &Value) -> NativeToolResult {
    match resolve_window_target(args, "close_window") {
        Ok((hwnd, info)) => {
            if win32::close_window(hwnd) {
                NativeToolResult::text_only(format!(
                    "Sent close to: \"{}\" pid={}",
                    info.title, info.pid
                ))
            } else {
                NativeToolResult::text_only(format!(
                    "Failed to close: \"{}\" pid={}",
                    info.title, info.pid
                ))
            }
        }
        Err(result) => result,
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_close_window(_args: &Value) -> NativeToolResult {
    super::tool_error("close_window", "not available on this platform")
}

// ─── resize_window ───────────────────────────────────────────────────────────

/// Resize and/or move a window by title or process name.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_resize_window(args: &Value) -> NativeToolResult {
    let x = args.get("x").and_then(parse_int).map(|v| v as i32);
    let y = args.get("y").and_then(parse_int).map(|v| v as i32);
    let w = args.get("width").and_then(parse_int).map(|v| v as i32);
    let h = args.get("height").and_then(parse_int).map(|v| v as i32);

    if x.is_none() && y.is_none() && w.is_none() && h.is_none() {
        return super::tool_error("resize_window", "at least one of x, y, width, height is required");
    }

    match resolve_window_target(args, "resize_window") {
        Ok((hwnd, info)) => {
            if win32::resize_window(hwnd, x, y, w, h) {
                let mut parts = Vec::new();
                if let (Some(px), Some(py)) = (x, y) {
                    parts.push(format!("moved to ({px},{py})"));
                }
                if let (Some(pw), Some(ph)) = (w, h) {
                    parts.push(format!("resized to {pw}x{ph}"));
                }
                NativeToolResult::text_only(format!(
                    "Window \"{}\" pid={}: {}", info.title, info.pid, parts.join(", ")
                ))
            } else {
                NativeToolResult::text_only(format!(
                    "Failed to resize/move: \"{}\" pid={}",
                    info.title, info.pid
                ))
            }
        }
        Err(result) => result,
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_resize_window(_args: &Value) -> NativeToolResult {
    super::tool_error("resize_window", "not available on this platform")
}

// ─── set_window_topmost ──────────────────────────────────────────────────────

/// Set or remove always-on-top (topmost) for a window.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_set_window_topmost(args: &Value) -> NativeToolResult {
    let topmost = args.get("topmost").map(|v| parse_bool(v, true)).unwrap_or(true);

    match resolve_window_target(args, "set_window_topmost") {
        Ok((hwnd, info)) => {
            if win32::set_topmost(hwnd, topmost) {
                let state = if topmost { "always-on-top" } else { "normal" };
                NativeToolResult::text_only(format!(
                    "Set '{}' pid={} to {state}",
                    info.title, info.pid
                ))
            } else {
                NativeToolResult::text_only(format!(
                    "Failed to set topmost for '{}' pid={}",
                    info.title, info.pid
                ))
            }
        }
        Err(result) => result,
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_set_window_topmost(_args: &Value) -> NativeToolResult {
    super::tool_error("set_window_topmost", "not available on this platform")
}

// ─── snap_window ─────────────────────────────────────────────────────────────

/// Snap a window to predefined screen positions (left, right, top-left, etc.).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_snap_window(args: &Value) -> NativeToolResult {
    let position = args.get("position").and_then(|v| v.as_str()).unwrap_or("left");

    let (hwnd, info) = match resolve_window_target(args, "snap_window") {
        Ok(result) => result,
        Err(result) => return result,
    };

    // Restore if maximized so SetWindowPos works
    unsafe {
        if win32::IsZoomed(hwnd) != 0 && position != "maximize" {
            win32::ShowWindow(hwnd, win32::SW_RESTORE);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    if position == "maximize" {
        unsafe { win32::ShowWindow(hwnd, win32::SW_MAXIMIZE); }
        return NativeToolResult::text_only(format!("Maximized '{}' pid={}", info.title, info.pid));
    }
    if position == "restore" {
        unsafe { win32::ShowWindow(hwnd, win32::SW_RESTORE); }
        return NativeToolResult::text_only(format!("Restored '{}' pid={}", info.title, info.pid));
    }

    let work = match win32::get_monitor_work_area(hwnd) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[snap_window] get_monitor_work_area failed: {e}, falling back to primary monitor");
            match xcap::Monitor::all() {
                Ok(monitors) => {
                    let m = monitors.iter()
                        .find(|m| m.is_primary().unwrap_or(false))
                        .or(monitors.first());
                    match m {
                        Some(m) => {
                            let mx = m.x().unwrap_or(0);
                            let my = m.y().unwrap_or(0);
                            let mw = m.width().unwrap_or(1920) as i32;
                            let mh = m.height().unwrap_or(1080) as i32;
                            win32::RECT { left: mx, top: my, right: mx + mw, bottom: my + mh }
                        }
                        None => return super::tool_error("snap_window", format!("monitor query failed: {e}")),
                    }
                }
                Err(e2) => return super::tool_error("snap_window", format!("monitor query failed: {e}, {e2}")),
            }
        }
    };

    let ww = work.right - work.left;
    let wh = work.bottom - work.top;

    let (x, y, w, h) = match position {
        "left"         => (work.left, work.top, ww / 2, wh),
        "right"        => (work.left + ww / 2, work.top, ww / 2, wh),
        "top-left"     => (work.left, work.top, ww / 2, wh / 2),
        "top-right"    => (work.left + ww / 2, work.top, ww / 2, wh / 2),
        "bottom-left"  => (work.left, work.top + wh / 2, ww / 2, wh / 2),
        "bottom-right" => (work.left + ww / 2, work.top + wh / 2, ww / 2, wh / 2),
        "center" => {
            let cw = ww * 2 / 3;
            let ch = wh * 2 / 3;
            (work.left + (ww - cw) / 2, work.top + (wh - ch) / 2, cw, ch)
        }
        other => return super::tool_error("snap_window", format!(
            "unknown position '{other}', use: left, right, top-left, top-right, bottom-left, bottom-right, center, maximize, restore"
        )),
    };

    let success = unsafe { win32::SetWindowPos(hwnd, 0, x, y, w, h, win32::SWP_SHOWWINDOW) };
    if success == 0 {
        return super::tool_error("snap_window", "SetWindowPos failed");
    }
    NativeToolResult::text_only(format!(
        "Snapped '{}' pid={} to {position} ({x},{y} {w}x{h})",
        info.title, info.pid
    ))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_snap_window(_args: &Value) -> NativeToolResult {
    super::tool_error("snap_window", "not available on this platform")
}

// ─── click_window_relative ───────────────────────────────────────────────────

/// Click at coordinates relative to a window's top-left corner.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_click_window_relative(args: &Value) -> NativeToolResult {
    let rel_x = match args.get("x").and_then(parse_int) {
        Some(v) => v as i32,
        None => return super::tool_error("click_window_relative", "'x' coordinate is required"),
    };
    let rel_y = match args.get("y").and_then(parse_int) {
        Some(v) => v as i32,
        None => return super::tool_error("click_window_relative", "'y' coordinate is required"),
    };

    match resolve_window_target(args, "click_window_relative") {
        Ok((hwnd, info)) => {
            win32::focus_window(hwnd);
            std::thread::sleep(std::time::Duration::from_millis(100));

            let abs_x = info.x + rel_x;
            let abs_y = info.y + rel_y;

            let button = args.get("button").and_then(|v| v.as_str()).unwrap_or("left");
            let delay_ms = args.get("delay_ms").and_then(parse_int).unwrap_or(500);
            let mut click_args = serde_json::json!({
                "x": abs_x, "y": abs_y, "button": button, "delay_ms": delay_ms
            });
            if let Some(obj) = click_args.as_object_mut() {
                for key in [
                    "screenshot",
                    "verify_screen_change",
                    "verify_threshold_pct",
                    "verify_timeout_ms",
                    "verify_poll_ms",
                ] {
                    if let Some(value) = args.get(key) {
                        obj.insert(key.to_string(), value.clone());
                    }
                }
            }
            let mut result = tool_click_screen(&click_args);
            result.text = format!(
                "Clicked {button} at relative ({rel_x},{rel_y}) → absolute ({abs_x},{abs_y}) in \"{}\" pid={}. {}",
                info.title, info.pid, result.text
            );
            result
        }
        Err(result) => result,
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_click_window_relative(_args: &Value) -> NativeToolResult {
    super::tool_error("click_window_relative", "not available on this platform")
}

// ─── switch_virtual_desktop ──────────────────────────────────────────────────

/// Switch virtual desktop using Ctrl+Win+Left/Right.
pub fn tool_switch_virtual_desktop(args: &Value) -> NativeToolResult {
    let direction = match args.get("direction").and_then(|v| v.as_str()) {
        Some(d) => d,
        None => return super::tool_error("switch_virtual_desktop", "'direction' is required (left or right)"),
    };
    let key = match direction {
        "left" | "prev" | "previous" => "ctrl+win+left",
        "right" | "next"             => "ctrl+win+right",
        other => return super::tool_error("switch_virtual_desktop", format!("Unknown direction '{other}'. Use: left, right")),
    };
    super::tool_press_key(&serde_json::json!({"key": key, "delay_ms": 500}))
}

// ─── Window layout save/restore ──────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedWindow {
    process_name: String,
    title: String,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    maximized: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct WindowLayout {
    saved_at: String,
    windows: Vec<SavedWindow>,
}

/// Save the current window layout (positions and sizes) to a named file.
/// Params: `name` (string, required) — layout name used as filename.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_save_window_layout(args: &Value) -> NativeToolResult {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return super::tool_error("save_window_layout", "'name' argument is required"),
    };

    let safe_name: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();

    let all_windows = win32::enumerate_windows();

    let saved: Vec<SavedWindow> = all_windows
        .into_iter()
        .filter(|w| w.width > 0 && w.height > 0)
        .map(|w| SavedWindow {
            process_name: w.process_name,
            title: w.title,
            x: w.x,
            y: w.y,
            width: w.width,
            height: w.height,
            maximized: w.maximized,
        })
        .collect();

    let count = saved.len();

    let layout = WindowLayout {
        saved_at: chrono::Utc::now().to_rfc3339(),
        windows: saved,
    };

    let json = match serde_json::to_string_pretty(&layout) {
        Ok(j) => j,
        Err(e) => return super::tool_error("save_window_layout", format!("serializing layout: {e}")),
    };

    let path = std::env::temp_dir().join(format!("desktop_layout_{safe_name}.json"));
    match std::fs::write(&path, &json) {
        Ok(()) => NativeToolResult::text_only(format!(
            "Saved {count} windows to {}",
            path.display()
        )),
        Err(e) => super::tool_error("save_window_layout", format!("writing file: {e}")),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_save_window_layout(_args: &Value) -> NativeToolResult {
    super::tool_error("save_window_layout", "not available on this platform")
}

/// Restore a previously saved window layout by name.
/// Params: `name` (string, required).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_restore_window_layout(args: &Value) -> NativeToolResult {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return super::tool_error("restore_window_layout", "'name' argument is required"),
    };

    let safe_name: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();

    let path = std::env::temp_dir().join(format!("desktop_layout_{safe_name}.json"));
    let json = match std::fs::read_to_string(&path) {
        Ok(j) => j,
        Err(e) => return super::tool_error("restore_window_layout", format!("reading '{}': {e}", path.display())),
    };

    let layout: WindowLayout = match serde_json::from_str(&json) {
        Ok(l) => l,
        Err(e) => return super::tool_error("restore_window_layout", format!("parsing layout: {e}")),
    };

    let total = layout.windows.len();
    let mut restored = 0u32;
    let mut not_found = 0u32;

    let current_windows = win32::enumerate_windows();

    for saved in &layout.windows {
        let found = current_windows.iter().enumerate().find(|(_, cw)| {
            if !saved.process_name.is_empty() && !cw.process_name.is_empty() {
                let saved_base = saved.process_name.to_lowercase();
                let current_base = cw.process_name.to_lowercase();
                if saved_base == current_base {
                    if !saved.title.is_empty() && !cw.title.is_empty() {
                        return cw.title.to_lowercase().contains(&saved.title.to_lowercase())
                            || saved.title.to_lowercase().contains(&cw.title.to_lowercase());
                    }
                    return true;
                }
            }
            false
        });

        if let Some((idx, _cw)) = found {
            let hwnd_result = if !saved.process_name.is_empty() {
                win32::find_window_by_filter(&saved.process_name)
            } else {
                win32::find_window_by_filter(&saved.title)
            };

            if let Some((hwnd, _)) = hwnd_result {
                if saved.maximized {
                    unsafe { win32::ShowWindow(hwnd, win32::SW_RESTORE); }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }

                win32::resize_window(
                    hwnd,
                    Some(saved.x),
                    Some(saved.y),
                    Some(saved.width),
                    Some(saved.height),
                );

                if saved.maximized {
                    unsafe { win32::ShowWindow(hwnd, win32::SW_MAXIMIZE); }
                }

                restored += 1;
            } else {
                let _ = idx;
                not_found += 1;
            }
        } else {
            not_found += 1;
        }
    }

    NativeToolResult::text_only(format!(
        "Restored {restored}/{total} windows ({not_found} not found). Layout saved at: {}",
        layout.saved_at
    ))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_restore_window_layout(_args: &Value) -> NativeToolResult {
    super::tool_error("restore_window_layout", "not available on this platform")
}
