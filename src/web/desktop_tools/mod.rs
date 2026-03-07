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
pub(crate) fn parse_int(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

/// Helper: parse bool from JSON value.
pub(crate) fn parse_bool(v: &Value, default: bool) -> bool {
    v.as_bool()
        .or_else(|| v.as_str().map(|s| s.eq_ignore_ascii_case("true")))
        .unwrap_or(default)
}

/// Parse a key string like "ctrl+shift+s" into (modifiers, main_key).
pub(crate) fn parse_key_combo(key_str: &str) -> Result<(Vec<Key>, Key), String> {
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
pub(crate) mod win32;

mod window_tools;
pub use window_tools::*;

mod ui_tools;
pub use ui_tools::*;

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
