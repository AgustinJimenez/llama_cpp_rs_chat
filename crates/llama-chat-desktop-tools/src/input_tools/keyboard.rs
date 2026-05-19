//! Keyboard input tools: type text and press key combinations.

use enigo::{Direction, Keyboard};
use serde_json::Value;

use crate::helpers::{parse_bool, parse_int, parse_key_combo, tool_error, with_enigo};
use crate::trace::{finalize_action_result, prepare_screen_verification};
use crate::screenshot_tools;

// ─── Win32 Unicode input helpers ─────────────────────────────────────────────

/// Type text using Win32 SendInput with KEYEVENTF_UNICODE (IME/Unicode fallback).
#[cfg(windows)]
pub(crate) fn type_text_via_send_input(text: &str) -> Result<(), String> {
    for ch in text.chars() {
        let code = ch as u16;
        let down = crate::win32::INPUT {
            input_type: crate::win32::INPUT_KEYBOARD,
            ki: crate::win32::KEYBDINPUT {
                w_vk: 0,
                w_scan: code,
                dw_flags: crate::win32::KEYEVENTF_UNICODE,
                time: 0,
                dw_extra_info: 0,
            },
            _pad: [0; 8],
        };
        let up = crate::win32::INPUT {
            input_type: crate::win32::INPUT_KEYBOARD,
            ki: crate::win32::KEYBDINPUT {
                w_vk: 0,
                w_scan: code,
                dw_flags: crate::win32::KEYEVENTF_UNICODE | crate::win32::KEYEVENTF_KEYUP,
                time: 0,
                dw_extra_info: 0,
            },
            _pad: [0; 8],
        };
        let inputs = [down, up];
        let sent = unsafe {
            crate::win32::SendInput(2, inputs.as_ptr(), std::mem::size_of::<crate::win32::INPUT>() as i32)
        };
        if sent != 2 {
            return Err(format!("SendInput failed for character '{ch}'"));
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn windows_keyboard_lang_id() -> u32 {
    extern "system" {
        fn GetKeyboardLayout(thread_id: u32) -> isize;
    }

    let layout = unsafe { GetKeyboardLayout(0) } as u32;
    layout & 0xFFFF
}

#[cfg(windows)]
pub(crate) fn should_prefer_unicode_input(args: &Value, text: &str) -> bool {
    match args.get("method").and_then(|v| v.as_str()) {
        Some("unicode") => return true,
        Some("enigo") => return false,
        _ => {}
    }

    !text.is_ascii() || windows_keyboard_lang_id() != 0x0409
}

// ─── Keyboard tool implementations ───────────────────────────────────────────

/// Type text using keyboard simulation.
pub fn tool_type_text(args: &Value) -> crate::NativeToolResult {
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
pub fn tool_press_key(args: &Value) -> crate::NativeToolResult {
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
