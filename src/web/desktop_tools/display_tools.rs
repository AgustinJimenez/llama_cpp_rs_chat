//! Display and window positioning tools: move to monitor, opacity, visual highlighting.

use serde_json::Value;

use super::NativeToolResult;
use super::parse_int;

/// Move a window to a specific monitor.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_move_to_monitor(args: &Value) -> NativeToolResult {
    #[cfg(windows)]
    use super::win32;
    #[cfg(target_os = "macos")]
    use super::macos as win32;
    #[cfg(target_os = "linux")]
    use super::linux as win32;

    let title = match args.get("title").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return super::tool_error("move_to_monitor", "'title' is required"),
    };
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    let (hwnd, info) = match win32::find_window_by_filter(title) {
        Some(r) => r,
        None => return super::tool_error("move_to_monitor", format!("no window matches '{title}'")),
    };

    let monitors = match super::validated_monitors("move_to_monitor", monitor_idx) {
        Ok(m) => m,
        Err(e) => return e,
    };

    let mon = &monitors[monitor_idx];
    let mon_x = mon.x().unwrap_or(0);
    let mon_y = mon.y().unwrap_or(0);

    // Move window, preserve size
    unsafe {
        win32::SetWindowPos(
            hwnd,
            0, // no z-order change
            mon_x,
            mon_y,
            info.width,
            info.height,
            win32::SWP_NOZORDER | win32::SWP_SHOWWINDOW,
        );
    }

    let screenshot = super::capture_post_action_screenshot(300);
    NativeToolResult {
        text: format!(
            "Moved '{}' to monitor {} at ({}, {})",
            info.title, monitor_idx, mon_x, mon_y
        ),
        images: screenshot.images,
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_move_to_monitor(_args: &Value) -> NativeToolResult {
    super::tool_error("move_to_monitor", "not available on this platform")
}

/// Set window transparency (opacity 0-100).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_set_window_opacity(args: &Value) -> NativeToolResult {
    #[cfg(windows)]
    use super::win32;
    #[cfg(target_os = "macos")]
    use super::macos as win32;
    #[cfg(target_os = "linux")]
    use super::linux as win32;

    let title = match args.get("title").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return super::tool_error("set_window_opacity", "'title' is required"),
    };
    let opacity = args.get("opacity").and_then(parse_int).unwrap_or(100).clamp(0, 100);

    let (hwnd, info) = match win32::find_window_by_filter(title) {
        Some(r) => r,
        None => return super::tool_error("set_window_opacity", format!("no window matches '{title}'")),
    };

    let alpha = (opacity as f64 / 100.0 * 255.0) as u8;
    match win32::set_window_opacity(hwnd, alpha) {
        Ok(()) => NativeToolResult::text_only(format!(
            "Set '{}' opacity to {}% (alpha={})",
            info.title, opacity, alpha
        )),
        Err(e) => super::tool_error("set_window_opacity", e),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_set_window_opacity(_args: &Value) -> NativeToolResult {
    super::tool_error("set_window_opacity", "not available on this platform")
}

/// Draw a crosshair marker on a screenshot at given coordinates (debugging aid).
pub fn tool_highlight_point(args: &Value) -> NativeToolResult {
    let x = match args.get("x").and_then(parse_int) {
        Some(v) => v as u32,
        None => return super::tool_error("highlight_point", "'x' is required"),
    };
    let y = match args.get("y").and_then(parse_int) {
        Some(v) => v as u32,
        None => return super::tool_error("highlight_point", "'y' is required"),
    };
    let size = args.get("size").and_then(parse_int).unwrap_or(20) as u32;
    let color_name = args.get("color").and_then(|v| v.as_str()).unwrap_or("red");
    let monitor_idx = args.get("monitor").and_then(parse_int).unwrap_or(0) as usize;

    let color = match color_name {
        "green" => image::Rgba([0, 255, 0, 255]),
        "blue" => image::Rgba([0, 100, 255, 255]),
        "yellow" => image::Rgba([255, 255, 0, 255]),
        _ => image::Rgba([255, 0, 0, 255]), // red default
    };

    // Capture screen
    let monitors = match super::validated_monitors("highlight_point", monitor_idx) {
        Ok(m) => m,
        Err(e) => return e,
    };
    let mut screen = match monitors[monitor_idx].capture_image() {
        Ok(i) => i,
        Err(e) => return super::tool_error("highlight_point", format!("capturing screen: {e}")),
    };

    let (sw, sh) = (screen.width(), screen.height());

    // Draw crosshair lines
    let half = size;
    for dx in 0..=half * 2 {
        let px = (x as i64 - half as i64 + dx as i64) as u32;
        if px < sw {
            if y < sh {
                screen.put_pixel(px, y, color);
            }
            if y > 0 && y - 1 < sh {
                screen.put_pixel(px, y.saturating_sub(1), color);
            }
        }
    }
    for dy in 0..=half * 2 {
        let py = (y as i64 - half as i64 + dy as i64) as u32;
        if py < sh {
            if x < sw {
                screen.put_pixel(x, py, color);
            }
            if x > 0 && x - 1 < sw {
                screen.put_pixel(x.saturating_sub(1), py, color);
            }
        }
    }

    // Draw circle outline
    let radius = size / 2;
    for angle in 0..360 {
        let rad = (angle as f64).to_radians();
        let cx = x as f64 + radius as f64 * rad.cos();
        let cy = y as f64 + radius as f64 * rad.sin();
        let (px, py) = (cx as u32, cy as u32);
        if px < sw && py < sh {
            screen.put_pixel(px, py, color);
        }
    }

    // Encode to PNG
    let mut buf = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(std::io::Cursor::new(&mut buf));
    if let Err(e) = image::ImageEncoder::write_image(
        encoder,
        screen.as_raw(),
        sw,
        sh,
        image::ExtendedColorType::Rgba8,
    ) {
        return super::tool_error("highlight_point", format!("encoding: {e}"));
    }

    NativeToolResult {
        text: format!(
            "Highlighted point ({}, {}) with {} crosshair (size {})",
            x, y, color_name, size
        ),
        images: vec![buf],
    }
}
