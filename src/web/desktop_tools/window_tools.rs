//! Window management, clipboard, and process tools.

use serde_json::Value;

use super::NativeToolResult;
use super::{parse_bool, parse_int, parse_key_combo, tool_click_screen};

#[cfg(windows)]
use super::win32;
#[cfg(target_os = "macos")]
use super::macos as win32;
#[cfg(target_os = "linux")]
use super::linux as win32;

/// List all visible windows on the desktop with titles, positions, sizes, and process names.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
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
                    || w.class_name.to_lowercase().contains(f)
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
        let cls = if w.class_name.is_empty() {
            String::new()
        } else {
            format!(" [{}]", w.class_name)
        };
        output.push_str(&format!(
            "  [{}] \"{}\"{}{} — {},{} {}x{}{}\n",
            i, w.title, proc, cls, w.x, w.y, w.width, w.height, state
        ));
    }

    NativeToolResult::text_only(output)
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_list_windows(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: list_windows is not available on this platform".to_string())
}

/// Get the current mouse cursor position.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_get_cursor_position(_args: &Value) -> NativeToolResult {
    let (x, y) = win32::get_cursor_position();
    NativeToolResult::text_only(format!("Cursor position: ({x}, {y})"))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_get_cursor_position(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: get_cursor_position is not available on this platform".to_string())
}

/// Focus (bring to front) a window by title or process name.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
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

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_focus_window(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: focus_window is not available on this platform".to_string())
}

/// Minimize a window by title or process name.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
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

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_minimize_window(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: minimize_window is not available on this platform".to_string())
}

/// Maximize a window by title or process name.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
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

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_maximize_window(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: maximize_window is not available on this platform".to_string())
}

/// Close a window by title or process name (sends WM_CLOSE).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
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

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_close_window(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: close_window is not available on this platform".to_string())
}

/// Read text from the system clipboard, reporting format info.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_read_clipboard(_args: &Value) -> NativeToolResult {
    // Report available formats
    let formats = win32::get_clipboard_formats();
    let format_str = if formats.is_empty() { "empty".to_string() } else { formats.join("+") };

    // Check for file drop (CF_HDROP) first
    if let Ok(files) = win32::read_clipboard_files() {
        if !files.is_empty() {
            let mut output = format!("Format: {format_str}. Clipboard contains {} file(s):\n", files.len());
            for f in &files {
                output.push_str(&format!("  {f}\n"));
            }
            return NativeToolResult::text_only(output);
        }
    }
    // Fall back to text
    match win32::read_clipboard() {
        Ok(text) => {
            let summary = if text.len() > 200 {
                format!("Format: {format_str}. Clipboard ({} chars): \"{}...\"", text.len(), &text[..200])
            } else {
                format!("Format: {format_str}. Clipboard: \"{text}\"")
            };
            NativeToolResult::text_only(summary)
        }
        Err(e) => NativeToolResult::text_only(format!("Format: {format_str}. Error: {e}")),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_read_clipboard(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: read_clipboard is not available on this platform".to_string())
}

/// Write text to the system clipboard.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
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

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_write_clipboard(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: write_clipboard is not available on this platform".to_string())
}

/// Resize and/or move a window by title or process name.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
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

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_resize_window(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: resize_window is not available on this platform".to_string())
}

/// Get information about the currently active (foreground) window.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
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

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_get_active_window(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: get_active_window is not available on this platform".to_string())
}

/// Wait for a window to appear by title or process name (polling).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
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

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_wait_for_window(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: wait_for_window is not available on this platform".to_string())
}

/// Get the color of a pixel at screen coordinates.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
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

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_get_pixel_color(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: get_pixel_color is not available on this platform".to_string())
}

/// Click at coordinates relative to a window's top-left corner.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
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

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_click_window_relative(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: click_window_relative is not available on this platform".to_string())
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

/// Set or remove always-on-top (topmost) for a window.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_set_window_topmost(args: &Value) -> NativeToolResult {
    let title = match args.get("title").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return NativeToolResult::text_only("Error: 'title' argument is required".to_string()),
    };
    let topmost = args.get("topmost").map(|v| parse_bool(v, true)).unwrap_or(true);

    match win32::find_window_by_filter(title) {
        Some((hwnd, info)) => {
            if win32::set_topmost(hwnd, topmost) {
                let state = if topmost { "always-on-top" } else { "normal" };
                NativeToolResult::text_only(format!("Set '{}' to {state}", info.title))
            } else {
                NativeToolResult::text_only(format!("Failed to set topmost for '{}'", info.title))
            }
        }
        None => NativeToolResult::text_only(format!("No window matches '{title}'")),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_set_window_topmost(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: set_window_topmost is not available on this platform".to_string())
}

/// Snap a window to predefined screen positions (left, right, top-left, etc.).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_snap_window(args: &Value) -> NativeToolResult {
    let title = match args.get("title").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return NativeToolResult::text_only("Error: 'title' is required".to_string()),
    };
    let position = args.get("position").and_then(|v| v.as_str()).unwrap_or("left");

    let (hwnd, info) = match win32::find_window_by_filter(title) {
        Some(r) => r,
        None => return NativeToolResult::text_only(format!("No window matches '{title}'")),
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
        return NativeToolResult::text_only(format!("Maximized '{}'", info.title));
    }
    if position == "restore" {
        unsafe { win32::ShowWindow(hwnd, win32::SW_RESTORE); }
        return NativeToolResult::text_only(format!("Restored '{}'", info.title));
    }

    let work = match win32::get_monitor_work_area(hwnd) {
        Ok(r) => r,
        Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
    };

    let ww = work.right - work.left;
    let wh = work.bottom - work.top;

    let (x, y, w, h) = match position {
        "left" => (work.left, work.top, ww / 2, wh),
        "right" => (work.left + ww / 2, work.top, ww / 2, wh),
        "top-left" => (work.left, work.top, ww / 2, wh / 2),
        "top-right" => (work.left + ww / 2, work.top, ww / 2, wh / 2),
        "bottom-left" => (work.left, work.top + wh / 2, ww / 2, wh / 2),
        "bottom-right" => (work.left + ww / 2, work.top + wh / 2, ww / 2, wh / 2),
        "center" => {
            let cw = ww * 2 / 3;
            let ch = wh * 2 / 3;
            (work.left + (ww - cw) / 2, work.top + (wh - ch) / 2, cw, ch)
        }
        other => return NativeToolResult::text_only(format!(
            "Unknown position '{other}'. Use: left, right, top-left, top-right, bottom-left, bottom-right, center, maximize, restore"
        )),
    };

    unsafe {
        win32::SetWindowPos(hwnd, 0, x, y, w, h, win32::SWP_SHOWWINDOW);
    }
    NativeToolResult::text_only(format!("Snapped '{}' to {position} ({x},{y} {w}x{h})", info.title))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_snap_window(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: snap_window is not available on this platform".to_string())
}

/// Open/launch an application by name or path. With `capture_output: true`, captures stdout/stderr.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_open_application(args: &Value) -> NativeToolResult {
    let target = match args.get("target").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return NativeToolResult::text_only("Error: 'target' argument is required (app name or path)".to_string()),
    };
    let arguments = args.get("args").and_then(|v| v.as_str());
    let capture_output = super::parse_bool(
        args.get("capture_output").unwrap_or(&serde_json::json!(false)),
        false,
    );

    // If this is a known GPU-rendered app that's already running, inform the model
    // but don't block — let the model decide what to do.
    if let Some(gpu) = super::gpu_app_db::detect_gpu_app_by_target(target) {
        if super::gpu_app_db::is_gpu_app_running(gpu) {
            let guidance = super::gpu_app_db::build_guidance(gpu);
            return NativeToolResult::text_only(format!(
                "{} is already running. A new instance was not opened.\n\
                 You can interact with the existing instance.\n\n{}",
                gpu.app_name, guidance
            ));
        }
    }

    if capture_output {
        let mut cmd = std::process::Command::new(target);
        cmd.stdin(std::process::Stdio::null());
        if let Some(a) = arguments {
            for part in a.split_whitespace() {
                cmd.arg(part);
            }
        }
        match cmd.output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let mut result = format!("Exit code: {}\n", output.status.code().unwrap_or(-1));
                if !stdout.is_empty() {
                    let trunc = if stdout.len() > 4000 { &stdout[..4000] } else { &stdout };
                    result.push_str(&format!("stdout:\n{trunc}\n"));
                }
                if !stderr.is_empty() {
                    let trunc = if stderr.len() > 2000 { &stderr[..2000] } else { &stderr };
                    result.push_str(&format!("stderr:\n{trunc}\n"));
                }
                NativeToolResult::text_only(result)
            }
            Err(e) => NativeToolResult::text_only(format!("Error running '{target}': {e}")),
        }
    } else {
        match win32::shell_execute(target, arguments) {
            Ok(()) => {
                let desc = if let Some(a) = arguments {
                    format!("Launched '{target}' with args '{a}'")
                } else {
                    format!("Launched '{target}'")
                };
                NativeToolResult::text_only(desc)
            }
            Err(_) => {
                // ShellExecute failed — search for the app in common locations
                match find_application_exe(target) {
                    Some(found_path) => {
                        match win32::shell_execute(&found_path, arguments) {
                            Ok(()) => {
                                let desc = if let Some(a) = arguments {
                                    format!("Launched '{target}' from '{found_path}' with args '{a}'")
                                } else {
                                    format!("Launched '{target}' from '{found_path}'")
                                };
                                NativeToolResult::text_only(desc)
                            }
                            Err(e2) => NativeToolResult::text_only(format!("Error: found '{found_path}' but failed to launch: {e2}")),
                        }
                    }
                    None => NativeToolResult::text_only(format!(
                        "Error: '{target}' not found. Not in PATH, registry, or Program Files. \
                         Try providing the full path to the executable."
                    )),
                }
            }
        }
    }
}

/// Search for an application executable by name in common installation directories.
/// Returns the full path if found, None otherwise.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub(super) fn find_application_exe(name: &str) -> Option<String> {
    let name_lower = name.to_lowercase();
    // Normalize: strip .exe suffix for matching
    let base_name = name_lower.strip_suffix(".exe").unwrap_or(&name_lower);

    // 1. Search Program Files directories for folders matching the app name
    let search_dirs: Vec<std::path::PathBuf> = {
        let mut dirs = Vec::new();
        #[cfg(windows)]
        {
            if let Ok(pf) = std::env::var("ProgramFiles") {
                dirs.push(std::path::PathBuf::from(pf));
            }
            if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
                dirs.push(std::path::PathBuf::from(pf86));
            }
            if let Ok(local) = std::env::var("LOCALAPPDATA") {
                dirs.push(std::path::PathBuf::from(local));
            }
        }
        #[cfg(target_os = "macos")]
        {
            dirs.push(std::path::PathBuf::from("/Applications"));
            dirs.push(std::path::PathBuf::from("/usr/local/bin"));
        }
        #[cfg(target_os = "linux")]
        {
            dirs.push(std::path::PathBuf::from("/usr/bin"));
            dirs.push(std::path::PathBuf::from("/usr/local/bin"));
            dirs.push(std::path::PathBuf::from("/opt"));
            dirs.push(std::path::PathBuf::from("/snap/bin"));
        }
        dirs
    };

    #[cfg(windows)]
    let exe_name = format!("{base_name}.exe");
    #[cfg(not(windows))]
    let exe_name = base_name.to_string();

    for dir in &search_dirs {
        if !dir.exists() {
            continue;
        }
        // Check direct binary (e.g. /usr/bin/blender)
        let direct = dir.join(&exe_name);
        if direct.is_file() {
            return Some(direct.to_string_lossy().into_owned());
        }
        // Search subdirectories (e.g. C:\Program Files\Blender Foundation\Blender 5.0\blender.exe)
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let entry_name = entry.file_name().to_string_lossy().to_lowercase();
                if entry_name.contains(base_name) && entry.path().is_dir() {
                    // Found a matching folder — look for exe inside (up to 2 levels deep)
                    if let Some(found) = find_exe_in_dir(&entry.path(), &exe_name, 2) {
                        return Some(found);
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        // Check .app bundles: /Applications/Blender.app/Contents/MacOS/Blender
        let app_path = format!("/Applications/{}.app/Contents/MacOS/{}",
            capitalize_first(base_name), capitalize_first(base_name));
        if std::path::Path::new(&app_path).is_file() {
            return Some(app_path);
        }
        // Also try lowercase
        let app_path_lower = format!("/Applications/{}.app/Contents/MacOS/{}",
            capitalize_first(base_name), base_name);
        if std::path::Path::new(&app_path_lower).is_file() {
            return Some(app_path_lower);
        }
    }

    None
}

/// Recursively search for an executable file in a directory, up to max_depth levels.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn find_exe_in_dir(dir: &std::path::Path, exe_name: &str, max_depth: u32) -> Option<String> {
    // Check direct child
    let candidate = dir.join(exe_name);
    if candidate.is_file() {
        return Some(candidate.to_string_lossy().into_owned());
    }
    if max_depth == 0 {
        return None;
    }
    // Recurse into subdirs
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                if let Some(found) = find_exe_in_dir(&entry.path(), exe_name, max_depth - 1) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Capitalize the first letter of a string (for macOS .app bundle names).
#[cfg(target_os = "macos")]
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_open_application(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: open_application is not available on this platform".to_string())
}

/// List running processes, optionally filtered by name.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_list_processes(args: &Value) -> NativeToolResult {
    let filter = args.get("filter").and_then(|v| v.as_str()).map(|s| s.to_lowercase());

    match win32::enumerate_processes() {
        Ok(procs) => {
            let mut filtered: Vec<_> = procs.into_iter()
                .filter(|(_, name)| {
                    filter.as_ref().map_or(true, |f| name.to_lowercase().contains(f))
                })
                .collect();
            filtered.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));

            let total = filtered.len();
            let limited = if total > 100 { &filtered[..100] } else { &filtered };

            let lines: Vec<String> = limited.iter().map(|(pid, name)| {
                format!("  PID {pid:>6}  {name}")
            }).collect();

            let suffix = if total > 100 { format!("\n... and {} more (use filter to narrow)", total - 100) } else { String::new() };
            NativeToolResult::text_only(format!("{} process(es):\n{}{suffix}", limited.len(), lines.join("\n")))
        }
        Err(e) => NativeToolResult::text_only(format!("Error: {e}")),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_list_processes(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: list_processes is not available on this platform".to_string())
}

/// Terminate a process by name or PID. Refuses to kill system-critical processes.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_kill_process(args: &Value) -> NativeToolResult {
    let name_filter = args.get("name").and_then(|v| v.as_str());
    let pid = args.get("pid").and_then(parse_int).map(|v| v as u32);

    if name_filter.is_none() && pid.is_none() {
        return NativeToolResult::text_only("Error: 'name' or 'pid' is required".to_string());
    }

    // System-critical processes that must never be killed
    const PROTECTED: &[&str] = &[
        "csrss.exe", "lsass.exe", "smss.exe", "svchost.exe", "dwm.exe",
        "winlogon.exe", "wininit.exe", "services.exe", "system",
        "explorer.exe", "conhost.exe",
    ];

    let current_pid = std::process::id();

    if let Some(target_pid) = pid {
        if target_pid == current_pid {
            return NativeToolResult::text_only("Error: refusing to kill own process".to_string());
        }
        // Check process name against protected list
        if let Ok(procs) = win32::enumerate_processes() {
            if let Some((_, name)) = procs.iter().find(|(p, _)| *p == target_pid) {
                if PROTECTED.iter().any(|&p| name.to_lowercase() == p) {
                    return NativeToolResult::text_only(format!("Error: refusing to kill system-critical process '{name}' (PID {target_pid})"));
                }
            }
        }
        match win32::terminate_process(target_pid) {
            Ok(()) => NativeToolResult::text_only(format!("Terminated process PID {target_pid}")),
            Err(e) => NativeToolResult::text_only(format!("Error: {e}")),
        }
    } else if let Some(name) = name_filter {
        let name_lower = name.to_lowercase();
        if PROTECTED.iter().any(|&p| name_lower == p || name_lower == p.trim_end_matches(".exe")) {
            return NativeToolResult::text_only(format!("Error: refusing to kill system-critical process '{name}'"));
        }
        match win32::enumerate_processes() {
            Ok(procs) => {
                let targets: Vec<_> = procs.into_iter()
                    .filter(|(p, n)| *p != current_pid && n.to_lowercase().contains(&name_lower))
                    .collect();
                if targets.is_empty() {
                    return NativeToolResult::text_only(format!("No process matching '{name}' found"));
                }
                let mut killed = 0;
                let mut errors = Vec::new();
                for (p, n) in &targets {
                    match win32::terminate_process(*p) {
                        Ok(()) => killed += 1,
                        Err(e) => errors.push(format!("PID {p} ({n}): {e}")),
                    }
                }
                let mut msg = format!("Killed {killed}/{} process(es) matching '{name}'", targets.len());
                if !errors.is_empty() {
                    msg.push_str(&format!("\nErrors: {}", errors.join("; ")));
                }
                NativeToolResult::text_only(msg)
            }
            Err(e) => NativeToolResult::text_only(format!("Error: {e}")),
        }
    } else {
        NativeToolResult::text_only("Error: unreachable".to_string())
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_kill_process(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: kill_process is not available on this platform".to_string())
}

/// Send keystrokes to a window via PostMessageW (works in background).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_send_keys_to_window(args: &Value) -> NativeToolResult {
    let title = match args.get("title").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return NativeToolResult::text_only("Error: 'title' is required".to_string()),
    };
    let keys = args.get("keys").and_then(|v| v.as_str());
    let text = args.get("text").and_then(|v| v.as_str());
    let method = args.get("method").and_then(|v| v.as_str()).unwrap_or("post_message");

    if keys.is_none() && text.is_none() {
        return NativeToolResult::text_only("Error: 'keys' or 'text' is required".to_string());
    }

    let (hwnd, info) = match win32::find_window_by_filter(title) {
        Some(r) => r,
        None => return NativeToolResult::text_only(format!("No window matches '{title}'")),
    };

    if method == "send_input" {
        return send_keys_via_send_input(hwnd, &info, text, keys);
    }

    let mut actions = Vec::new();

    // Send text characters via WM_CHAR
    if let Some(txt) = text {
        for ch in txt.chars() {
            unsafe {
                win32::PostMessageW(hwnd, win32::WM_CHAR, ch as usize, 0);
            }
        }
        actions.push(format!("typed {} chars", txt.len()));
    }

    // Send key combos via WM_KEYDOWN/WM_KEYUP
    if let Some(key_str) = keys {
        match parse_key_combo(key_str) {
            Ok((modifiers, main_key)) => {
                // Press modifiers
                for m in &modifiers {
                    if let Some(vk) = win32::key_to_vk(m) {
                        let lparam = make_key_lparam(vk, false);
                        unsafe { win32::PostMessageW(hwnd, win32::WM_KEYDOWN, vk as usize, lparam); }
                    }
                }
                // Press main key
                if let Some(vk) = win32::key_to_vk(&main_key) {
                    let lparam_down = make_key_lparam(vk, false);
                    let lparam_up = make_key_lparam(vk, true);
                    unsafe {
                        win32::PostMessageW(hwnd, win32::WM_KEYDOWN, vk as usize, lparam_down);
                        win32::PostMessageW(hwnd, win32::WM_KEYUP, vk as usize, lparam_up);
                    }
                }
                // Release modifiers (reverse order)
                for m in modifiers.iter().rev() {
                    if let Some(vk) = win32::key_to_vk(m) {
                        let lparam = make_key_lparam(vk, true);
                        unsafe { win32::PostMessageW(hwnd, win32::WM_KEYUP, vk as usize, lparam); }
                    }
                }
                actions.push(format!("sent key '{key_str}'"));
            }
            Err(e) => return NativeToolResult::text_only(format!("Error parsing keys: {e}")),
        }
    }

    NativeToolResult::text_only(format!("Sent to '{}': {}", info.title, actions.join(", ")))
}

/// Send keys via SendInput (requires foreground focus, more reliable for games/custom UIs).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn send_keys_via_send_input(hwnd: win32::HWND, info: &win32::WindowInfo, text: Option<&str>, keys: Option<&str>) -> NativeToolResult {
    // Focus the window first
    unsafe {
        win32::SetForegroundWindow(hwnd);
    }
    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut actions = Vec::new();

    // Type text via KEYEVENTF_UNICODE
    if let Some(txt) = text {
        let mut inputs = Vec::new();
        for ch in txt.encode_utf16() {
            inputs.push(win32::INPUT {
                input_type: win32::INPUT_KEYBOARD,
                ki: win32::KEYBDINPUT {
                    w_vk: 0,
                    w_scan: ch,
                    dw_flags: win32::KEYEVENTF_UNICODE,
                    time: 0,
                    dw_extra_info: 0,
                },
                _pad: [0; 8],
            });
            inputs.push(win32::INPUT {
                input_type: win32::INPUT_KEYBOARD,
                ki: win32::KEYBDINPUT {
                    w_vk: 0,
                    w_scan: ch,
                    dw_flags: win32::KEYEVENTF_UNICODE | win32::KEYEVENTF_KEYUP,
                    time: 0,
                    dw_extra_info: 0,
                },
                _pad: [0; 8],
            });
        }
        let sent = unsafe {
            win32::SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<win32::INPUT>() as i32)
        };
        actions.push(format!("typed {} chars ({} events sent)", txt.len(), sent));
    }

    // Send key combos via VK SendInput
    if let Some(key_str) = keys {
        match parse_key_combo(key_str) {
            Ok((modifiers, main_key)) => {
                let mut inputs = Vec::new();
                // Press modifiers
                for m in &modifiers {
                    if let Some(vk) = win32::key_to_vk(m) {
                        inputs.push(win32::INPUT {
                            input_type: win32::INPUT_KEYBOARD,
                            ki: win32::KEYBDINPUT { w_vk: vk as u16, w_scan: 0, dw_flags: 0, time: 0, dw_extra_info: 0 },
                            _pad: [0; 8],
                        });
                    }
                }
                // Press+release main key
                if let Some(vk) = win32::key_to_vk(&main_key) {
                    inputs.push(win32::INPUT {
                        input_type: win32::INPUT_KEYBOARD,
                        ki: win32::KEYBDINPUT { w_vk: vk as u16, w_scan: 0, dw_flags: 0, time: 0, dw_extra_info: 0 },
                        _pad: [0; 8],
                    });
                    inputs.push(win32::INPUT {
                        input_type: win32::INPUT_KEYBOARD,
                        ki: win32::KEYBDINPUT { w_vk: vk as u16, w_scan: 0, dw_flags: win32::KEYEVENTF_KEYUP, time: 0, dw_extra_info: 0 },
                        _pad: [0; 8],
                    });
                }
                // Release modifiers (reverse)
                for m in modifiers.iter().rev() {
                    if let Some(vk) = win32::key_to_vk(m) {
                        inputs.push(win32::INPUT {
                            input_type: win32::INPUT_KEYBOARD,
                            ki: win32::KEYBDINPUT { w_vk: vk as u16, w_scan: 0, dw_flags: win32::KEYEVENTF_KEYUP, time: 0, dw_extra_info: 0 },
                            _pad: [0; 8],
                        });
                    }
                }
                let sent = unsafe {
                    win32::SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<win32::INPUT>() as i32)
                };
                actions.push(format!("sent key '{key_str}' ({sent} events)"));
            }
            Err(e) => return NativeToolResult::text_only(format!("Error parsing keys: {e}")),
        }
    }

    NativeToolResult::text_only(format!("SendInput to '{}': {}", info.title, actions.join(", ")))
}

/// Build the lParam for WM_KEYDOWN/WM_KEYUP messages.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn make_key_lparam(vk: u32, key_up: bool) -> isize {
    let scan_code = unsafe { win32::MapVirtualKeyW(vk, 0) }; // MAPVK_VK_TO_VSC = 0
    let mut lparam: isize = 1; // repeat count = 1
    lparam |= (scan_code as isize & 0xFF) << 16;
    if key_up {
        lparam |= (1 << 30) | (1 << 31); // previous key state + transition state
    }
    lparam
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_send_keys_to_window(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: send_keys_to_window is not available on this platform".to_string())
}

/// Switch virtual desktop using Ctrl+Win+Left/Right.
pub fn tool_switch_virtual_desktop(args: &Value) -> NativeToolResult {
    let direction = match args.get("direction").and_then(|v| v.as_str()) {
        Some(d) => d,
        None => {
            return NativeToolResult::text_only(
                "Error: 'direction' is required (left or right)".to_string(),
            )
        }
    };
    let key = match direction {
        "left" | "prev" | "previous" => "ctrl+win+left",
        "right" | "next" => "ctrl+win+right",
        other => {
            return NativeToolResult::text_only(format!(
                "Error: Unknown direction '{other}'. Use: left, right"
            ))
        }
    };
    super::tool_press_key(&serde_json::json!({"key": key, "delay_ms": 500}))
}

/// Get resource info (memory, CPU time) for a process by PID or name.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_get_process_info(args: &Value) -> NativeToolResult {
    let pid = args.get("pid").and_then(parse_int).map(|v| v as u32);
    let name = args.get("name").and_then(|v| v.as_str());

    let target_pid = if let Some(p) = pid {
        p
    } else if let Some(n) = name {
        // Find PID by process name
        let lower = n.to_lowercase();
        match win32::enumerate_processes() {
            Ok(procs) => {
                match procs.iter().find(|(_, pname)| pname.to_lowercase().contains(&lower)) {
                    Some((p, _)) => *p,
                    None => {
                        return NativeToolResult::text_only(format!(
                            "Error: no process matching '{n}'"
                        ))
                    }
                }
            }
            Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
        }
    } else {
        return NativeToolResult::text_only(
            "Error: 'pid' or 'name' is required".to_string(),
        );
    };

    match win32::get_process_resource_info(target_pid) {
        Ok((working_set, kernel_ms, user_ms)) => {
            let mb = working_set as f64 / (1024.0 * 1024.0);
            NativeToolResult::text_only(format!(
                "PID {target_pid}: memory={mb:.1}MB, kernel_time={kernel_ms}ms, user_time={user_ms}ms, total_cpu={}ms",
                kernel_ms + user_ms
            ))
        }
        Err(e) => NativeToolResult::text_only(format!("Error: {e}")),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_get_process_info(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: get_process_info is not available on this platform".to_string())
}
