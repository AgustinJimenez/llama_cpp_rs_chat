//! Window enumeration and state query tools.
//!
//! Provides: `list_windows`, `get_active_window`, `wait_for_window`,
//! `get_cursor_position`, `get_pixel_color`, `list_monitors`, and the
//! `is_fullscreen_by_rect` helper used by sibling modules.

use serde_json::Value;

use super::NativeToolResult;
use super::{parse_int, gpu_app_db};

use super::win32;

// ─── Shared helper ────────────────────────────────────────────────────────────

/// Check if a window rect covers any monitor entirely (cross-platform fullscreen detection).
pub(super) fn is_fullscreen_by_rect(x: i32, y: i32, width: i32, height: i32) -> bool {
    match xcap::Monitor::all() {
        Ok(monitors) => {
            for m in &monitors {
                let mx = m.x().unwrap_or(0);
                let my = m.y().unwrap_or(0);
                let mw = m.width().unwrap_or(0) as i32;
                let mh = m.height().unwrap_or(0) as i32;
                if x == mx && y == my && width == mw && height == mh {
                    return true;
                }
                if x <= mx && y <= my && (x + width) >= (mx + mw) && (y + height) >= (my + mh) {
                    return true;
                }
            }
            false
        }
        Err(e) => {
            eprintln!("[desktop_tools] is_fullscreen_by_rect: failed to enumerate monitors: {e}");
            false
        }
    }
}

// ─── list_windows ─────────────────────────────────────────────────────────────

/// List all visible windows on the desktop with titles, positions, sizes, and process names.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_list_windows(args: &Value) -> NativeToolResult {
    let filter = args
        .get("filter")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());
    let pid_filter = args.get("pid").and_then(parse_int).map(|v| v as u32);

    let windows = win32::enumerate_windows();

    let filtered: Vec<&win32::WindowInfo> = windows
        .iter()
        .filter(|w| {
            if let Some(target_pid) = pid_filter {
                if w.pid != target_pid {
                    return false;
                }
            }
            if let Some(ref f) = filter {
                w.title.to_lowercase().contains(f)
                    || w.process_name.to_lowercase().contains(f)
                    || w.class_name.to_lowercase().contains(f)
            } else {
                true
            }
        })
        .collect();

    if filtered.is_empty() {
        let msg = if filter.is_some() || pid_filter.is_some() {
            format!(
                "No visible windows match filter (text={}, pid={}). Total visible windows: {}",
                filter.as_deref().unwrap_or("any"),
                pid_filter.map_or("any".to_string(), |p| p.to_string()),
                windows.len()
            )
        } else {
            "No visible windows found.".to_string()
        };
        return NativeToolResult::text_only(msg);
    }

    let mut output = format!("Found {} windows:\n", filtered.len());
    for (i, w) in filtered.iter().enumerate() {
        let fullscreen = is_fullscreen_by_rect(w.x, w.y, w.width, w.height);
        let mut state_parts = Vec::new();
        if w.minimized { state_parts.push("minimized"); }
        if w.maximized { state_parts.push("maximized"); }
        if w.focused  { state_parts.push("focused"); }
        if fullscreen  { state_parts.push("fullscreen"); }
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
        let cls = if w.class_name.is_empty() {
            String::new()
        } else {
            format!(" [{}]", w.class_name)
        };
        output.push_str(&format!(
            "  [{}] \"{}\"{}{} — pid={} {},{} {}x{}{}",
            i, w.title, proc, cls, w.pid, w.x, w.y, w.width, w.height, state
        ));
        if let Some(gpu) = gpu_app_db::detect_gpu_app(&w.class_name, &w.process_name) {
            output.push_str(&format!(" [GPU: {} — use execute_app_script]", gpu.app_name));
        }
        output.push('\n');
    }

    NativeToolResult::text_only(output)
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_list_windows(_args: &Value) -> NativeToolResult {
    super::tool_error("list_windows", "not available on this platform")
}

// ─── get_active_window ────────────────────────────────────────────────────────

/// Get information about the currently active (foreground) window.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_get_active_window(_args: &Value) -> NativeToolResult {
    match win32::get_active_window_info() {
        Some((_hwnd, info)) => {
            let fullscreen = is_fullscreen_by_rect(info.x, info.y, info.width, info.height);
            let mut state_parts = Vec::new();
            if info.minimized { state_parts.push("minimized"); }
            if info.maximized { state_parts.push("maximized"); }
            if fullscreen     { state_parts.push("fullscreen"); }
            let state = if state_parts.is_empty() {
                String::new()
            } else {
                format!(" [{}]", state_parts.join(", "))
            };
            NativeToolResult::text_only(format!(
                "Active window: \"{}\" ({}) pid={} at {},{} size {}x{}{}",
                info.title, info.process_name, info.pid,
                info.x, info.y, info.width, info.height, state
            ))
        }
        None => NativeToolResult::text_only("No active window found".to_string()),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_get_active_window(_args: &Value) -> NativeToolResult {
    super::tool_error("get_active_window", "not available on this platform")
}

// ─── wait_for_window ─────────────────────────────────────────────────────────

/// Wait for a window to appear by title or process name (polling).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_wait_for_window(args: &Value) -> NativeToolResult {
    let title_filter = args.get("title").and_then(|v| v.as_str());
    let pid_filter = args.get("pid").and_then(parse_int).map(|v| v as u32);
    let timeout_ms = args.get("timeout_ms").and_then(parse_int).unwrap_or(10000).min(60000) as u64;
    let poll_ms = args.get("poll_ms").and_then(parse_int).unwrap_or(200).max(50) as u64;

    if title_filter.is_none() && pid_filter.is_none() {
        return super::tool_error("wait_for_window", "'title' or 'pid' argument is required");
    }

    let start = std::time::Instant::now();
    loop {
        if let Err(e) = super::ensure_desktop_not_cancelled() {
            return super::tool_error("wait_for_window", e);
        }

        let matched = if let Some(target_pid) = pid_filter {
            win32::find_window_by_pid(target_pid)
        } else {
            None
        }
        .or_else(|| title_filter.and_then(win32::find_window_by_filter));

        if let Some((_hwnd, info)) = matched {
            return NativeToolResult::text_only(format!(
                "Found window: \"{}\" ({}) at {},{} size {}x{} (waited {}ms)",
                info.title, info.process_name, info.x, info.y, info.width, info.height,
                start.elapsed().as_millis()
            ));
        }
        if start.elapsed().as_millis() as u64 >= timeout_ms {
            let target = pid_filter
                .map(|pid| format!("pid={pid}"))
                .or_else(|| title_filter.map(|title| format!("title='{title}'")))
                .unwrap_or_else(|| "unknown target".to_string());
            return NativeToolResult::text_only(format!(
                "Timeout: no window matching {target} appeared within {}ms",
                timeout_ms
            ));
        }
        if let Err(e) = super::interruptible_sleep(std::time::Duration::from_millis(poll_ms)) {
            return super::tool_error("wait_for_window", e);
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_wait_for_window(_args: &Value) -> NativeToolResult {
    super::tool_error("wait_for_window", "not available on this platform")
}

// ─── get_cursor_position ─────────────────────────────────────────────────────

/// Get the current mouse cursor position.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_get_cursor_position(_args: &Value) -> NativeToolResult {
    let (x, y) = win32::get_cursor_position();
    NativeToolResult::text_only(format!("Cursor position: ({x}, {y})"))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_get_cursor_position(_args: &Value) -> NativeToolResult {
    super::tool_error("get_cursor_position", "not available on this platform")
}

// ─── get_pixel_color ─────────────────────────────────────────────────────────

/// Get the color of a pixel at screen coordinates.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_get_pixel_color(args: &Value) -> NativeToolResult {
    let x = match args.get("x").and_then(parse_int) {
        Some(v) => v as i32,
        None => return super::tool_error("get_pixel_color", "'x' coordinate is required"),
    };
    let y = match args.get("y").and_then(parse_int) {
        Some(v) => v as i32,
        None => return super::tool_error("get_pixel_color", "'y' coordinate is required"),
    };
    match win32::get_pixel_color(x, y) {
        Ok((r, g, b)) => NativeToolResult::text_only(format!(
            "Pixel at ({x},{y}): rgb({r},{g},{b}) = #{r:02X}{g:02X}{b:02X}"
        )),
        Err(e) => super::tool_error("get_pixel_color", e),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_get_pixel_color(_args: &Value) -> NativeToolResult {
    super::tool_error("get_pixel_color", "not available on this platform")
}

// ─── list_monitors ────────────────────────────────────────────────────────────

/// List all monitors with their properties.
pub fn tool_list_monitors(args: &Value) -> NativeToolResult {
    let index_filter = args.get("index").and_then(parse_int).map(|v| v as usize);

    let monitors = if let Some(idx) = index_filter {
        match super::validated_monitors("list_monitors", idx) {
            Ok(m) => m,
            Err(e) => return e,
        }
    } else {
        match xcap::Monitor::all() {
            Ok(m) => m,
            Err(e) => return super::tool_error("list_monitors", format!("enumerating monitors: {e}")),
        }
    };

    if monitors.is_empty() {
        return NativeToolResult::text_only("No monitors found".to_string());
    }

    if let Some(idx) = index_filter {
        if idx >= monitors.len() {
            return super::tool_error("list_monitors", format!(
                "monitor index {} out of range (0..{})", idx, monitors.len()
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
