//! Input tools: mouse clicks, keyboard input, scrolling, dragging, and high-level
//! helpers (paste, clear field, hover element).

use enigo::{Axis, Button, Coordinate, Direction, Keyboard, Mouse};
use serde_json::Value;

use super::helpers::{
    apply_dpi_scaling, check_modal_dialog, parse_bool, parse_int, parse_key_combo,
    snap_coordinates, tool_error, validate_coordinates, with_enigo,
};
use super::trace::{
    capture_post_action_screenshot, finalize_action_result, normalize_verification_region,
    prepare_screen_verification,
};
use super::{ensure_desktop_not_cancelled, interruptible_sleep, screenshot_tools, NativeToolResult};

// ─── Win32 Unicode input helpers ─────────────────────────────────────────────

/// Type text using Win32 SendInput with KEYEVENTF_UNICODE (IME/Unicode fallback).
#[cfg(windows)]
fn type_text_via_send_input(text: &str) -> Result<(), String> {
    for ch in text.chars() {
        let code = ch as u16;
        let down = super::win32::INPUT {
            input_type: super::win32::INPUT_KEYBOARD,
            ki: super::win32::KEYBDINPUT {
                w_vk: 0,
                w_scan: code,
                dw_flags: super::win32::KEYEVENTF_UNICODE,
                time: 0,
                dw_extra_info: 0,
            },
            _pad: [0; 8],
        };
        let up = super::win32::INPUT {
            input_type: super::win32::INPUT_KEYBOARD,
            ki: super::win32::KEYBDINPUT {
                w_vk: 0,
                w_scan: code,
                dw_flags: super::win32::KEYEVENTF_UNICODE | super::win32::KEYEVENTF_KEYUP,
                time: 0,
                dw_extra_info: 0,
            },
            _pad: [0; 8],
        };
        let inputs = [down, up];
        let sent = unsafe {
            super::win32::SendInput(2, inputs.as_ptr(), std::mem::size_of::<super::win32::INPUT>() as i32)
        };
        if sent != 2 {
            return Err(format!("SendInput failed for character '{ch}'"));
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    Ok(())
}

#[cfg(windows)]
fn windows_keyboard_lang_id() -> u32 {
    extern "system" {
        fn GetKeyboardLayout(thread_id: u32) -> isize;
    }

    let layout = unsafe { GetKeyboardLayout(0) } as u32;
    layout & 0xFFFF
}

#[cfg(windows)]
fn should_prefer_unicode_input(args: &Value, text: &str) -> bool {
    match args.get("method").and_then(|v| v.as_str()) {
        Some("unicode") => return true,
        Some("enigo") => return false,
        _ => {}
    }

    !text.is_ascii() || windows_keyboard_lang_id() != 0x0409
}

// ─── Core input tools ────────────────────────────────────────────────────────

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
    let stealth = args.get("stealth").map(|v| parse_bool(v, true)).unwrap_or(true);

    #[cfg(windows)]
    if stealth {
        let right = button_str == "right";
        if button_str == "double" || button_str == "middle" {
            return tool_error("click_screen", "Stealth mode only supports left/right clicks");
        }
        if let Err(e) = super::win32::stealth_click(x, y, right) {
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

/// Type text using keyboard simulation.
pub fn tool_type_text(args: &Value) -> NativeToolResult {
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_owned(),
        None => {
            return tool_error("type_text", "'text' argument is required")
        }
    };
    let do_screenshot = args
        .get("screenshot")
        .map(|v| parse_bool(v, true))
        .unwrap_or(true);
    let delay_ms = args
        .get("delay_ms")
        .and_then(parse_int)
        .unwrap_or(300) as u64;
    let retries = args
        .get("retry")
        .and_then(parse_int)
        .unwrap_or(0)
        .max(0)
        .min(3) as u32;
    let verification = match prepare_screen_verification(args, None) {
        Ok(v) => v,
        Err(result) => return result,
    };

    #[cfg(windows)]
    let prefer_unicode_input = should_prefer_unicode_input(args, &text);

    let text_clone = text.clone();
    let type_result = screenshot_tools::retry_on_failure(retries, 200, move || {
        #[cfg(windows)]
        if prefer_unicode_input {
            return type_text_via_send_input(&text_clone);
        }

        let res = with_enigo(|enigo| {
            enigo
                .text(&text_clone)
                .map_err(|e| format!("type_text failed: {e}"))
        });
        #[cfg(windows)]
        let res = res.or_else(|_| type_text_via_send_input(&text_clone));
        res
    });
    if let Err(e) = type_result {
        return tool_error("type_text", e);
    }

    #[allow(unused_mut)]
    let mut summary = if text.len() > 50 {
        format!("Typed {} characters: \"{}...\"", text.len(), &text[..50])
    } else {
        format!("Typed: \"{}\"", text)
    };

    // Warn if non-US keyboard layout detected (characters may differ from intent)
    #[cfg(windows)]
    {
        let lang_id = windows_keyboard_lang_id();
        if lang_id != 0x0409 {
            // 0x0409 = English (United States)
            summary.push_str(&format!(
                " (note: keyboard layout 0x{:04X}, not US-QWERTY)",
                lang_id
            ));
        }
        if prefer_unicode_input {
            summary.push_str(" (typed via Unicode input)");
        }
    }

    finalize_action_result(summary, delay_ms, do_screenshot, verification)
}

/// Press a key or key combination.
pub fn tool_press_key(args: &Value) -> NativeToolResult {
    let key_str = match args.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => {
            return tool_error("press_key", "'key' argument is required")
        }
    };
    let do_screenshot = args
        .get("screenshot")
        .map(|v| parse_bool(v, true))
        .unwrap_or(true);
    let delay_ms = args
        .get("delay_ms")
        .and_then(parse_int)
        .unwrap_or(500) as u64;
    let retries = args
        .get("retry")
        .and_then(parse_int)
        .unwrap_or(0)
        .max(0)
        .min(3) as u32;
    let verification = match prepare_screen_verification(args, None) {
        Ok(v) => v,
        Err(result) => return result,
    };

    let (modifiers, main_key) = match parse_key_combo(key_str) {
        Ok(combo) => combo,
        Err(e) => return tool_error("press_key", e),
    };

    let modifiers_clone = modifiers.clone();
    if let Err(e) = screenshot_tools::retry_on_failure(retries, 200, move || {
        with_enigo(|enigo| {
            for modifier in &modifiers_clone {
                enigo
                    .key(*modifier, Direction::Press)
                    .map_err(|e| format!("key press failed: {e}"))?;
            }
            let result = enigo.key(main_key, Direction::Click);
            // Always release modifiers
            for modifier in modifiers_clone.iter().rev() {
                let _ = enigo.key(*modifier, Direction::Release);
            }
            result.map_err(|e| format!("key press failed: {e}"))
        })
    }) {
        return tool_error("press_key", e);
    }

    let summary = format!("Pressed: {key_str}");

    finalize_action_result(summary, delay_ms, do_screenshot, verification)
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

/// Scroll repeatedly until OCR finds the target text on screen.
fn scroll_to_text(args: &Value) -> NativeToolResult {
    let target_text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return tool_error("scroll_screen", "'text' is required for mode='to_text'"),
    };
    let direction = args.get("direction").and_then(|v| v.as_str()).unwrap_or("down");
    let max_scrolls = args
        .get("max_scrolls")
        .and_then(parse_int)
        .unwrap_or(20)
        .min(50) as usize;
    let scroll_amount = if direction == "up" { -3 } else { 3 };
    let target_lower = target_text.to_lowercase();

    for i in 0..max_scrolls {
        if let Err(e) = ensure_desktop_not_cancelled() {
            return tool_error("scroll_screen", e);
        }

        // OCR current screen
        let ocr_result = super::ocr_tools::tool_ocr_screen(&serde_json::json!({"monitor": 0}));
        if ocr_result.text.to_lowercase().contains(&target_lower) {
            let screenshot = capture_post_action_screenshot(0);
            return NativeToolResult {
                text: format!("Found '{}' after {} scroll(s)", target_text, i),
                images: screenshot.images,
            };
        }
        // Scroll
        with_enigo(|enigo| {
            enigo
                .scroll(scroll_amount, enigo::Axis::Vertical)
                .map_err(|e| format!("{e}"))
        })
        .ok();
        if let Err(e) = interruptible_sleep(std::time::Duration::from_millis(400)) {
            return tool_error("scroll_screen", e);
        }
    }

    let screenshot = capture_post_action_screenshot(0);
    NativeToolResult {
        text: format!(
            "Text '{}' not found after {} scrolls {}",
            target_text, max_scrolls, direction
        ),
        images: screenshot.images,
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
        return scroll_to_text(args);
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

// ─── High-level input tools ─────────────────────────────────────────────────

/// Paste clipboard contents at the current cursor position (Ctrl+V).
pub fn tool_paste(args: &Value) -> NativeToolResult {
    let delay_ms = args
        .get("delay_ms")
        .and_then(parse_int)
        .unwrap_or(300) as u64;

    // Press Ctrl+V (Cmd+V on macOS)
    #[cfg(target_os = "macos")]
    let paste_key = "meta+v";
    #[cfg(not(target_os = "macos"))]
    let paste_key = "ctrl+v";
    let result = tool_press_key(&serde_json::json!({
        "key": paste_key,
        "delay_ms": delay_ms,
        "screenshot": true
    }));
    // The press_key tool already takes a screenshot; augment message
    NativeToolResult {
        text: format!("Pasted from clipboard. {}", result.text),
        images: result.images,
    }
}

/// Clear the currently focused field (Ctrl+A → Delete), optionally type new text.
pub fn tool_clear_field(args: &Value) -> NativeToolResult {
    let then_type = args.get("then_type").and_then(|v| v.as_str());
    let delay_ms = args
        .get("delay_ms")
        .and_then(parse_int)
        .unwrap_or(200) as u64;

    // Select all (Cmd+A on macOS)
    #[cfg(target_os = "macos")]
    let select_all_key = "meta+a";
    #[cfg(not(target_os = "macos"))]
    let select_all_key = "ctrl+a";
    tool_press_key(&serde_json::json!({"key": select_all_key, "delay_ms": 50, "screenshot": false}));
    // Delete selection
    tool_press_key(&serde_json::json!({"key": "delete", "delay_ms": 100, "screenshot": false}));

    if let Some(text) = then_type {
        let result = tool_type_text(&serde_json::json!({
            "text": text,
            "delay_ms": delay_ms,
            "screenshot": true
        }));
        NativeToolResult {
            text: format!("Cleared field and typed '{}'. {}", text, result.text),
            images: result.images,
        }
    } else {
        // Take final screenshot (selection already deleted above, just capture)
        let result = capture_post_action_screenshot(delay_ms);
        NativeToolResult {
            text: format!("Cleared field. {}", result.text),
            images: result.images,
        }
    }
}

/// Hover over a UI element by name/type, wait, and capture tooltip if present.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_hover_element(args: &Value) -> NativeToolResult {
    use super::ui_automation_tools;
    #[cfg(windows)]
    use super::win32;
    #[cfg(target_os = "macos")]
    use super::macos as win32;
    #[cfg(target_os = "linux")]
    use super::linux as win32;

    let name_filter = args.get("name").and_then(|v| v.as_str());
    let type_filter = args.get("control_type").and_then(|v| v.as_str());
    let hover_ms = args.get("hover_ms").and_then(parse_int).unwrap_or(800) as u64;

    if name_filter.is_none() && type_filter.is_none() {
        return tool_error("hover_element", "'name' or 'control_type' is required");
    }

    // Find window
    let title_filter = args.get("title").and_then(|v| v.as_str());
    let hwnd = if let Some(filter) = title_filter {
        match win32::find_window_by_filter(filter) {
            Some((h, _)) => h,
            None => return tool_error("hover_element", format!("no window matches '{filter}'")),
        }
    } else {
        match win32::get_active_window_info() {
            Some((h, _)) => h,
            None => return tool_error("hover_element", "no active window"),
        }
    };

    if let Some(r) = ui_automation_tools::check_gpu_app_guard(hwnd, "hover_element") { return r; }

    let name_owned = name_filter.map(|s| s.to_lowercase());
    let type_owned = type_filter.map(|s| s.to_lowercase());

    let element = match super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || {
        ui_automation_tools::find_ui_element(hwnd, name_owned.as_deref(), type_owned.as_deref())
    }).and_then(|r| r)
    {
        Ok(el) => el,
        Err(e) => return tool_error("hover_element", e),
    };

    // Move mouse to element center
    tool_move_mouse(&serde_json::json!({
        "x": element.cx,
        "y": element.cy,
        "screenshot": false
    }));

    // Wait for hover effects (tooltip) — interruptible so it can be cancelled
    if let Err(e) = interruptible_sleep(std::time::Duration::from_millis(hover_ms)) {
        return tool_error("hover_element", e);
    }

    // Try to find tooltip in UI tree
    let tooltip_text = super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || -> String {
        match ui_automation_tools::find_ui_elements_all(hwnd, None, Some("tooltip"), 1) {
            Ok(tips) if !tips.is_empty() => format!(" Tooltip: '{}'", tips[0].name),
            _ => String::new(),
        }
    })
    .unwrap_or_default();

    // Take screenshot showing hover state
    let screenshot = capture_post_action_screenshot(0);

    NativeToolResult {
        text: format!(
            "Hovering over '{}' [{}] at ({}, {}).{}",
            element.name, element.control_type, element.cx, element.cy, tooltip_text
        ),
        images: screenshot.images,
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_hover_element(_args: &Value) -> NativeToolResult {
    tool_error("hover_element", "not available on this platform")
}
