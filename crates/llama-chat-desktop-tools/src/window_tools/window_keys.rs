//! Keystroke injection into windows: `send_keys_to_window` and supporting helpers.
//!
//! Three send methods are supported:
//!   - `post_message` (default): WM_CHAR / WM_KEYDOWN / WM_KEYUP via PostMessageW
//!   - `send_input`: uses SendInput with virtual keys / Unicode events
//!   - `scancode`: uses SendInput with hardware scan codes (best for games/DirectInput)

use serde_json::Value;

use super::NativeToolResult;
use super::parse_key_combo;

use super::win32;

use super::window_management::resolve_window_target;

// ─── send_keys_to_window ─────────────────────────────────────────────────────

/// Send keystrokes to a window via PostMessageW (works in background).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_send_keys_to_window(args: &Value) -> NativeToolResult {
    let keys = args.get("keys").and_then(|v| v.as_str());
    let text = args.get("text").and_then(|v| v.as_str());
    let method = args.get("method").and_then(|v| v.as_str()).unwrap_or("post_message");

    if keys.is_none() && text.is_none() {
        return super::tool_error("send_keys_to_window", "'keys' or 'text' is required");
    }

    let (hwnd, info) = match resolve_window_target(args, "send_keys_to_window") {
        Ok(result) => result,
        Err(result) => return result,
    };

    if method == "send_input" {
        return send_keys_via_send_input(hwnd, &info, text, keys);
    }

    if method == "scancode" {
        return send_keys_via_scancode(hwnd, &info, text, keys);
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
                for m in &modifiers {
                    if let Some(vk) = win32::key_to_vk(m) {
                        let lparam = make_key_lparam(vk, false);
                        unsafe { win32::PostMessageW(hwnd, win32::WM_KEYDOWN, vk as usize, lparam); }
                    }
                }
                if let Some(vk) = win32::key_to_vk(&main_key) {
                    let lparam_down = make_key_lparam(vk, false);
                    let lparam_up   = make_key_lparam(vk, true);
                    unsafe {
                        win32::PostMessageW(hwnd, win32::WM_KEYDOWN, vk as usize, lparam_down);
                        win32::PostMessageW(hwnd, win32::WM_KEYUP,   vk as usize, lparam_up);
                    }
                }
                for m in modifiers.iter().rev() {
                    if let Some(vk) = win32::key_to_vk(m) {
                        let lparam = make_key_lparam(vk, true);
                        unsafe { win32::PostMessageW(hwnd, win32::WM_KEYUP, vk as usize, lparam); }
                    }
                }
                actions.push(format!("sent key '{key_str}'"));
            }
            Err(e) => return super::tool_error("send_keys_to_window", format!("parsing keys: {e}")),
        }
    }

    NativeToolResult::text_only(format!(
        "Sent to '{}' pid={}: {}",
        info.title, info.pid, actions.join(", ")
    ))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_send_keys_to_window(_args: &Value) -> NativeToolResult {
    super::tool_error("send_keys_to_window", "not available on this platform")
}

// ─── send_input helpers ───────────────────────────────────────────────────────

/// Send keys via SendInput (requires foreground focus, more reliable for games/custom UIs).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn send_keys_via_send_input(hwnd: win32::HWND, info: &win32::WindowInfo, text: Option<&str>, keys: Option<&str>) -> NativeToolResult {
    unsafe { win32::SetForegroundWindow(hwnd); }
    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut actions = Vec::new();

    if let Some(txt) = text {
        let mut inputs = Vec::new();
        for ch in txt.encode_utf16() {
            inputs.push(win32::INPUT {
                input_type: win32::INPUT_KEYBOARD,
                ki: win32::KEYBDINPUT { w_vk: 0, w_scan: ch, dw_flags: win32::KEYEVENTF_UNICODE, time: 0, dw_extra_info: 0 },
                _pad: [0; 8],
            });
            inputs.push(win32::INPUT {
                input_type: win32::INPUT_KEYBOARD,
                ki: win32::KEYBDINPUT { w_vk: 0, w_scan: ch, dw_flags: win32::KEYEVENTF_UNICODE | win32::KEYEVENTF_KEYUP, time: 0, dw_extra_info: 0 },
                _pad: [0; 8],
            });
        }
        let sent = unsafe {
            win32::SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<win32::INPUT>() as i32)
        };
        actions.push(format!("typed {} chars ({} events sent)", txt.len(), sent));
    }

    if let Some(key_str) = keys {
        match parse_key_combo(key_str) {
            Ok((modifiers, main_key)) => {
                let mut inputs = Vec::new();
                for m in &modifiers {
                    if let Some(vk) = win32::key_to_vk(m) {
                        inputs.push(win32::INPUT {
                            input_type: win32::INPUT_KEYBOARD,
                            ki: win32::KEYBDINPUT { w_vk: vk as u16, w_scan: 0, dw_flags: 0, time: 0, dw_extra_info: 0 },
                            _pad: [0; 8],
                        });
                    }
                }
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
            Err(e) => return super::tool_error("send_keys_to_window", format!("parsing keys: {e}")),
        }
    }

    NativeToolResult::text_only(format!(
        "SendInput to '{}' pid={}: {}",
        info.title, info.pid, actions.join(", ")
    ))
}

/// Send keys via SendInput with hardware scan codes (best for games/DirectInput).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn send_keys_via_scancode(hwnd: win32::HWND, info: &win32::WindowInfo, text: Option<&str>, keys: Option<&str>) -> NativeToolResult {
    unsafe { win32::SetForegroundWindow(hwnd); }
    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut actions = Vec::new();

    // Unicode text — scan codes don't help for arbitrary Unicode
    if let Some(txt) = text {
        let mut inputs = Vec::new();
        for ch in txt.encode_utf16() {
            inputs.push(win32::INPUT {
                input_type: win32::INPUT_KEYBOARD,
                ki: win32::KEYBDINPUT { w_vk: 0, w_scan: ch, dw_flags: win32::KEYEVENTF_UNICODE, time: 0, dw_extra_info: 0 },
                _pad: [0; 8],
            });
            inputs.push(win32::INPUT {
                input_type: win32::INPUT_KEYBOARD,
                ki: win32::KEYBDINPUT { w_vk: 0, w_scan: ch, dw_flags: win32::KEYEVENTF_UNICODE | win32::KEYEVENTF_KEYUP, time: 0, dw_extra_info: 0 },
                _pad: [0; 8],
            });
        }
        let sent = unsafe {
            win32::SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<win32::INPUT>() as i32)
        };
        actions.push(format!("typed {} chars ({} events sent)", txt.len(), sent));
    }

    if let Some(key_str) = keys {
        match parse_key_combo(key_str) {
            Ok((modifiers, main_key)) => {
                let mut inputs = Vec::new();
                for m in &modifiers {
                    if let Some(vk) = win32::key_to_vk(m) {
                        let scan = unsafe { win32::MapVirtualKeyW(vk, win32::MAPVK_VK_TO_VSC) } as u16;
                        inputs.push(win32::INPUT {
                            input_type: win32::INPUT_KEYBOARD,
                            ki: win32::KEYBDINPUT { w_vk: 0, w_scan: scan, dw_flags: win32::KEYEVENTF_SCANCODE, time: 0, dw_extra_info: 0 },
                            _pad: [0; 8],
                        });
                    }
                }
                if let Some(vk) = win32::key_to_vk(&main_key) {
                    let scan = unsafe { win32::MapVirtualKeyW(vk, win32::MAPVK_VK_TO_VSC) } as u16;
                    inputs.push(win32::INPUT {
                        input_type: win32::INPUT_KEYBOARD,
                        ki: win32::KEYBDINPUT { w_vk: 0, w_scan: scan, dw_flags: win32::KEYEVENTF_SCANCODE, time: 0, dw_extra_info: 0 },
                        _pad: [0; 8],
                    });
                    inputs.push(win32::INPUT {
                        input_type: win32::INPUT_KEYBOARD,
                        ki: win32::KEYBDINPUT { w_vk: 0, w_scan: scan, dw_flags: win32::KEYEVENTF_SCANCODE | win32::KEYEVENTF_KEYUP, time: 0, dw_extra_info: 0 },
                        _pad: [0; 8],
                    });
                }
                for m in modifiers.iter().rev() {
                    if let Some(vk) = win32::key_to_vk(m) {
                        let scan = unsafe { win32::MapVirtualKeyW(vk, win32::MAPVK_VK_TO_VSC) } as u16;
                        inputs.push(win32::INPUT {
                            input_type: win32::INPUT_KEYBOARD,
                            ki: win32::KEYBDINPUT { w_vk: 0, w_scan: scan, dw_flags: win32::KEYEVENTF_SCANCODE | win32::KEYEVENTF_KEYUP, time: 0, dw_extra_info: 0 },
                            _pad: [0; 8],
                        });
                    }
                }
                let sent = unsafe {
                    win32::SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<win32::INPUT>() as i32)
                };
                actions.push(format!("sent key '{key_str}' via scancode ({sent} events)"));
            }
            Err(e) => return super::tool_error("send_keys_to_window", format!("parsing keys: {e}")),
        }
    }

    NativeToolResult::text_only(format!(
        "Scancode SendInput to '{}' pid={}: {}",
        info.title, info.pid, actions.join(", ")
    ))
}

/// Build the lParam for WM_KEYDOWN/WM_KEYUP messages.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn make_key_lparam(vk: u32, key_up: bool) -> isize {
    let scan_code = unsafe { win32::MapVirtualKeyW(vk, 0) }; // MAPVK_VK_TO_VSC = 0
    let mut lparam: isize = 1; // repeat count = 1
    lparam |= (scan_code as isize & 0xFF) << 16;
    if key_up {
        lparam |= (1 << 30) | (1 << 31);
    }
    lparam
}
