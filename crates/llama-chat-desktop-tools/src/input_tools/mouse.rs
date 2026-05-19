//! Mouse input tools: click, move, drag, scroll, and button press/release.

use enigo::{Axis, Button, Coordinate, Direction, Mouse};
use serde_json::Value;

use crate::helpers::{
    apply_dpi_scaling, check_modal_dialog, parse_bool, parse_int,
    snap_coordinates, tool_error, validate_coordinates, with_enigo,
};
use crate::trace::{
    capture_post_action_screenshot, finalize_action_result, normalize_verification_region,
    prepare_screen_verification,
};
use crate::NativeToolResult;

/// Click the mouse at absolute screen coordinates.
pub fn tool_click_screen(args: &Value) -> NativeToolResult {
    let mut x = match args.get("x").and_then(parse_int) {
        Some(v) => v as i32,
        None => return tool_error("click_screen", "'x' coordinate is required"),
    };
    let mut y = match args.get("y").and_then(parse_int) {
        Some(v) => v as i32,
        None => return tool_error("click_screen", "'y' coordinate is required"),
    };
    // DPI scaling: convert logical coordinates to physical if requested
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    {
        let dpi_aware = args.get("dpi_aware").map(|v| parse_bool(v, false)).unwrap_or(false);
        let scaled = apply_dpi_scaling(x, y, dpi_aware);
        x = scaled.0;
        y = scaled.1;
    }
    let snap = args.get("snap_to_screen").map(|v| parse_bool(v, false)).unwrap_or(false);
    if snap {
        let snapped = snap_coordinates(x, y);
        x = snapped.0;
        y = snapped.1;
    }
    if let Err(e) = validate_coordinates(x, y) {
        return tool_error("click_screen", e);
    }
    // Check for modal dialog blocking the foreground window
    let modal_warning = check_modal_dialog().unwrap_or_default();

    let button_str = args
        .get("button")
        .and_then(|v| v.as_str())
        .unwrap_or("left");
    let delay_ms = args
        .get("delay_ms")
        .and_then(parse_int)
        .unwrap_or(500) as u64;
    // Auto-screenshot is on by default to match historical behavior, but can
    // be disabled to avoid bloating the caller's context with full-screen
    // captures during long UI automation sessions.
    let do_screenshot = args
        .get("screenshot")
        .map(|v| parse_bool(v, true))
        .unwrap_or(true);
    let verification_region =
        normalize_verification_region(x - 160, y - 160, 320, 320);
    let verification = match prepare_screen_verification(args, verification_region) {
        Ok(v) => v,
        Err(result) => return result,
    };

    // Stealth mode: save cursor → move → click → restore in <0.1ms
    // The user's cursor barely moves. Safe to use while user is working.
    // Stealth is ON by default — user's cursor is saved/restored in <0.1ms
    #[cfg(windows)]
    let stealth = args.get("stealth").map(|v| parse_bool(v, true)).unwrap_or(true);

    #[cfg(windows)]
    if stealth {
        let right = button_str == "right";
        if button_str == "double" || button_str == "middle" {
            return tool_error("click_screen", "Stealth mode only supports left/right clicks");
        }
        if let Err(e) = crate::win32::stealth_click(x, y, right) {
            return tool_error("click_screen", e);
        }
    }
    #[cfg(windows)]
    if !stealth {
        if let Err(e) = with_enigo(|enigo| {
            enigo
                .move_mouse(x, y, Coordinate::Abs)
                .map_err(|e| format!("move_mouse failed: {e}"))?;
            std::thread::sleep(std::time::Duration::from_millis(50));
            match button_str {
                "left" => enigo
                    .button(Button::Left, Direction::Click)
                    .map_err(|e| format!("click failed: {e}")),
                "right" => enigo
                    .button(Button::Right, Direction::Click)
                    .map_err(|e| format!("click failed: {e}")),
                "middle" => enigo
                    .button(Button::Middle, Direction::Click)
                    .map_err(|e| format!("click failed: {e}")),
                "double" => {
                    enigo
                        .button(Button::Left, Direction::Click)
                        .map_err(|e| format!("click failed: {e}"))?;
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    enigo
                        .button(Button::Left, Direction::Click)
                        .map_err(|e| format!("double-click failed: {e}"))
                }
                other => Err(format!(
                    "Unknown button '{other}'. Use: left, right, middle, double"
                )),
            }
        }) {
            return tool_error("click_screen", e);
        }
    }
    #[cfg(not(windows))]
    {
        if let Err(e) = with_enigo(|enigo| {
            enigo
                .move_mouse(x, y, Coordinate::Abs)
                .map_err(|e| format!("move_mouse failed: {e}"))?;
            std::thread::sleep(std::time::Duration::from_millis(50));
            match button_str {
                "left" => enigo
                    .button(Button::Left, Direction::Click)
                    .map_err(|e| format!("click failed: {e}")),
                "right" => enigo
                    .button(Button::Right, Direction::Click)
                    .map_err(|e| format!("click failed: {e}")),
                "middle" => enigo
                    .button(Button::Middle, Direction::Click)
                    .map_err(|e| format!("click failed: {e}")),
                "double" => {
                    enigo
                        .button(Button::Left, Direction::Click)
                        .map_err(|e| format!("click failed: {e}"))?;
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    enigo
                        .button(Button::Left, Direction::Click)
                        .map_err(|e| format!("double-click failed: {e}"))
                }
                other => Err(format!(
                    "Unknown button '{other}'. Use: left, right, middle, double"
                )),
            }
        }) {
            return tool_error("click_screen", e);
        }
    }

    finalize_action_result(
        format!("{modal_warning}Clicked {button_str} at ({x}, {y})"),
        delay_ms,
        do_screenshot,
        verification,
    )
}

/// Move the mouse cursor without clicking.
pub fn tool_move_mouse(args: &Value) -> NativeToolResult {
    let mut x = match args.get("x").and_then(parse_int) {
        Some(v) => v as i32,
        None => {
            return tool_error("move_mouse", "'x' coordinate is required")
        }
    };
    let mut y = match args.get("y").and_then(parse_int) {
        Some(v) => v as i32,
        None => {
            return tool_error("move_mouse", "'y' coordinate is required")
        }
    };
    #[cfg(windows)]
    {
        let dpi_aware = args.get("dpi_aware").map(|v| parse_bool(v, false)).unwrap_or(false);
        let scaled = apply_dpi_scaling(x, y, dpi_aware);
        x = scaled.0;
        y = scaled.1;
    }
    let snap = args.get("snap_to_screen").map(|v| parse_bool(v, false)).unwrap_or(false);
    if snap {
        let snapped = snap_coordinates(x, y);
        x = snapped.0;
        y = snapped.1;
    }
    if let Err(e) = validate_coordinates(x, y) {
        return tool_error("move_mouse", e);
    }

    if let Err(e) = with_enigo(|enigo| {
        enigo
            .move_mouse(x, y, Coordinate::Abs)
            .map_err(|e| format!("move_mouse failed: {e}"))
    }) {
        return tool_error("move_mouse", e);
    }

    NativeToolResult::text_only(format!("Mouse moved to ({x}, {y})"))
}

/// Drag the mouse from one position to another.
/// When `steps` > 1, interpolates intermediate positions with linear lerp for smooth dragging.
pub fn tool_mouse_drag(args: &Value) -> NativeToolResult {
    let x1 = match args.get("from_x").and_then(parse_int) {
        Some(v) => v as i32,
        None => return tool_error("mouse_drag", "'from_x' is required"),
    };
    let y1 = match args.get("from_y").and_then(parse_int) {
        Some(v) => v as i32,
        None => return tool_error("mouse_drag", "'from_y' is required"),
    };
    let x2 = match args.get("to_x").and_then(parse_int) {
        Some(v) => v as i32,
        None => return tool_error("mouse_drag", "'to_x' is required"),
    };
    let y2 = match args.get("to_y").and_then(parse_int) {
        Some(v) => v as i32,
        None => return tool_error("mouse_drag", "'to_y' is required"),
    };
    if let Err(e) = validate_coordinates(x1, y1) {
        return tool_error("mouse_drag", format!("start {e}"));
    }
    if let Err(e) = validate_coordinates(x2, y2) {
        return tool_error("mouse_drag", format!("end {e}"));
    }
    let delay_ms = args
        .get("delay_ms")
        .and_then(parse_int)
        .unwrap_or(500) as u64;
    let do_screenshot = args
        .get("screenshot")
        .map(|v| parse_bool(v, true))
        .unwrap_or(true);
    let steps = args
        .get("steps")
        .and_then(parse_int)
        .unwrap_or(1)
        .max(1)
        .min(100) as u32;
    let min_x = x1.min(x2) - 80;
    let min_y = y1.min(y2) - 80;
    let width = (x1.max(x2) - min_x + 80) as u32;
    let height = (y1.max(y2) - min_y + 80) as u32;
    let verification_region = normalize_verification_region(min_x, min_y, width, height);
    let verification = match prepare_screen_verification(args, verification_region) {
        Ok(v) => v,
        Err(result) => return result,
    };

    if let Err(e) = with_enigo(|enigo| {
        enigo
            .move_mouse(x1, y1, Coordinate::Abs)
            .map_err(|e| format!("move to start failed: {e}"))?;
        std::thread::sleep(std::time::Duration::from_millis(50));
        enigo
            .button(Button::Left, Direction::Press)
            .map_err(|e| format!("mouse down failed: {e}"))?;
        std::thread::sleep(std::time::Duration::from_millis(50));

        if steps > 1 {
            // Smooth interpolation: move through intermediate positions
            for i in 1..steps {
                let t = i as f64 / steps as f64;
                let ix = x1 as f64 + (x2 as f64 - x1 as f64) * t;
                let iy = y1 as f64 + (y2 as f64 - y1 as f64) * t;
                let move_result = enigo.move_mouse(ix as i32, iy as i32, Coordinate::Abs);
                if move_result.is_err() {
                    let _ = enigo.button(Button::Left, Direction::Release);
                    return move_result.map_err(|e| format!("interpolated move failed: {e}"));
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        // Final move to exact destination
        let move_result = enigo.move_mouse(x2, y2, Coordinate::Abs);
        if move_result.is_err() {
            let _ = enigo.button(Button::Left, Direction::Release);
        }
        move_result.map_err(|e| format!("move to end failed: {e}"))?;
        std::thread::sleep(std::time::Duration::from_millis(50));
        enigo
            .button(Button::Left, Direction::Release)
            .map_err(|e| format!("mouse up failed: {e}"))
    }) {
        return tool_error("mouse_drag", e);
    }

    let steps_note = if steps > 1 {
        format!(" ({steps} steps)")
    } else {
        String::new()
    };
    finalize_action_result(
        format!("Dragged from ({x1},{y1}) to ({x2},{y2}){steps_note}"),
        delay_ms,
        do_screenshot,
        verification,
    )
}

/// Press or release a mouse button independently (for hold-and-drag scenarios).
pub fn tool_mouse_button(args: &Value) -> NativeToolResult {
    let action = match args.get("action").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => {
            return tool_error("mouse_button", "'action' is required (press or release)")
        }
    };
    let button_str = args
        .get("button")
        .and_then(|v| v.as_str())
        .unwrap_or("left");
    let do_screenshot = args
        .get("screenshot")
        .map(|v| parse_bool(v, true))
        .unwrap_or(true);

    let button = match button_str {
        "left" => Button::Left,
        "right" => Button::Right,
        "middle" => Button::Middle,
        other => {
            return tool_error("mouse_button", format!("Unknown button '{other}'. Use: left, right, middle"))
        }
    };
    let direction = match action {
        "press" => Direction::Press,
        "release" => Direction::Release,
        other => {
            return tool_error("mouse_button", format!("Unknown action '{other}'. Use: press, release"))
        }
    };

    if let Err(e) = with_enigo(|enigo| {
        enigo
            .button(button, direction)
            .map_err(|e| format!("mouse button failed: {e}"))
    }) {
        return tool_error("mouse_button", e);
    }

    let past = if action == "press" {
        "pressed"
    } else {
        "released"
    };
    let summary = format!("Mouse {button_str} button {past}");
    if do_screenshot {
        let mut result = capture_post_action_screenshot(300);
        result.text = format!("{summary}. {}", result.text);
        result
    } else {
        NativeToolResult::text_only(summary)
    }
}

/// Scroll the mouse wheel at the current or specified position.
///
/// Supports two modes via the `mode` parameter:
/// - `"amount"` (default): scroll by a fixed number of units.
/// - `"to_text"`: scroll until OCR finds the specified `text` on screen.
pub fn tool_scroll_screen(args: &Value) -> NativeToolResult {
    let mode = args
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("amount");
    if mode == "to_text" {
        return super::scroll_to_text(args);
    }

    let amount = match args.get("amount").and_then(parse_int) {
        Some(v) => v as i32,
        None => {
            return tool_error("scroll_screen", "'amount' argument is required (positive=down, negative=up)")
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
    let verification_region = if let (Some(x), Some(y)) = (
        args.get("x").and_then(parse_int),
        args.get("y").and_then(parse_int),
    ) {
        normalize_verification_region(x as i32 - 180, y as i32 - 180, 360, 360)
    } else {
        None
    };
    let verification = match prepare_screen_verification(args, verification_region) {
        Ok(v) => v,
        Err(result) => return result,
    };

    if let Err(e) = with_enigo(|enigo| {
        if let (Some(x), Some(y)) = (
            args.get("x").and_then(parse_int),
            args.get("y").and_then(parse_int),
        ) {
            let (mut x, mut y) = (x as i32, y as i32);
            #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
            {
                let dpi_aware = args
                    .get("dpi_aware")
                    .map(|v| parse_bool(v, false))
                    .unwrap_or(false);
                let scaled = apply_dpi_scaling(x, y, dpi_aware);
                x = scaled.0;
                y = scaled.1;
            }
            if args
                .get("snap_to_screen")
                .map(|v| parse_bool(v, false))
                .unwrap_or(false)
            {
                let snapped = snap_coordinates(x, y);
                x = snapped.0;
                y = snapped.1;
            }
            validate_coordinates(x, y)?;
            enigo
                .move_mouse(x, y, Coordinate::Abs)
                .map_err(|e| format!("move_mouse failed: {e}"))?;
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let axis = if horizontal {
            Axis::Horizontal
        } else {
            Axis::Vertical
        };
        enigo
            .scroll(amount, axis)
            .map_err(|e| format!("scroll failed: {e}"))
    }) {
        return tool_error("scroll_screen", e);
    }

    let direction = if horizontal {
        if amount > 0 {
            "right"
        } else {
            "left"
        }
    } else if amount > 0 {
        "down"
    } else {
        "up"
    };
    let summary = format!("Scrolled {direction} by {} units", amount.abs());

    finalize_action_result(summary, delay_ms, do_screenshot, verification)
}
