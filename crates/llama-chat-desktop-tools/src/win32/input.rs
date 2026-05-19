//! Stealth mouse input (save/click/restore) and key virtual-code mapping.

use super::types::*;

const VK_LBUTTON: i32 = 0x01;

fn to_absolute(x: i32, y: i32) -> (i32, i32) {
    let screen_w = unsafe { GetSystemMetrics(0) } as f64;
    let screen_h = unsafe { GetSystemMetrics(1) } as f64;
    let abs_x = ((x as f64 * 65535.0) / screen_w + 0.5) as i32;
    let abs_y = ((y as f64 * 65535.0) / screen_h + 0.5) as i32;
    (abs_x, abs_y)
}

/// Perform a "stealth click": save cursor → move → click → restore.
/// Total time ~0.05ms. Returns true if the user's left button was NOT held
/// (i.e., click was safe to perform).
pub fn stealth_click(target_x: i32, target_y: i32, right_click: bool) -> Result<(), String> {
    unsafe {
        // 1. Check if user is mid-drag (left button held) — skip if so
        if GetAsyncKeyState(VK_LBUTTON) & (0x8000u16 as i16) != 0 {
            return Err("User is currently clicking/dragging, skipping stealth click".into());
        }

        // 2. Save current cursor position
        let mut saved = POINT { x: 0, y: 0 };
        GetCursorPos(&mut saved);

        // 3. Move to target (absolute coordinates)
        let (abs_x, abs_y) = to_absolute(target_x, target_y);
        let extra = GetMessageExtraInfo();

        let (down_flag, up_flag) = if right_click {
            (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP)
        } else {
            (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP)
        };

        let inputs = [
            // Move to target
            MouseINPUT {
                input_type: INPUT_MOUSE,
                _pad_union: [0; 4],
                mi: MOUSEINPUT {
                    dx: abs_x,
                    dy: abs_y,
                    mouse_data: 0,
                    dw_flags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                    time: 0,
                    dw_extra_info: extra,
                },
            },
            // Mouse down
            MouseINPUT {
                input_type: INPUT_MOUSE,
                _pad_union: [0; 4],
                mi: MOUSEINPUT {
                    dx: abs_x,
                    dy: abs_y,
                    mouse_data: 0,
                    dw_flags: down_flag | MOUSEEVENTF_ABSOLUTE,
                    time: 0,
                    dw_extra_info: extra,
                },
            },
            // Mouse up
            MouseINPUT {
                input_type: INPUT_MOUSE,
                _pad_union: [0; 4],
                mi: MOUSEINPUT {
                    dx: abs_x,
                    dy: abs_y,
                    mouse_data: 0,
                    dw_flags: up_flag | MOUSEEVENTF_ABSOLUTE,
                    time: 0,
                    dw_extra_info: extra,
                },
            },
        ];

        // 4. Send all three events at once (move + down + up)
        let sent = SendInput(
            3,
            inputs.as_ptr() as *const INPUT,
            std::mem::size_of::<MouseINPUT>() as i32,
        );

        // 5. Immediately restore cursor position
        SetCursorPos(saved.x, saved.y);

        if sent != 3 {
            return Err(format!("SendInput returned {sent}, expected 3"));
        }

        Ok(())
    }
}

/// Map an enigo Key to a Win32 virtual key code (for PostMessageW).
pub fn key_to_vk(key: &Key) -> Option<u32> {
    match key {
        Key::Return => Some(0x0D),     // VK_RETURN
        Key::Tab => Some(0x09),        // VK_TAB
        Key::Escape => Some(0x1B),     // VK_ESCAPE
        Key::Backspace => Some(0x08),  // VK_BACK
        Key::Delete => Some(0x2E),     // VK_DELETE
        Key::Space => Some(0x20),      // VK_SPACE
        Key::UpArrow => Some(0x26),    // VK_UP
        Key::DownArrow => Some(0x28),  // VK_DOWN
        Key::LeftArrow => Some(0x25),  // VK_LEFT
        Key::RightArrow => Some(0x27), // VK_RIGHT
        Key::Home => Some(0x24),       // VK_HOME
        Key::End => Some(0x23),        // VK_END
        Key::PageUp => Some(0x21),     // VK_PRIOR
        Key::PageDown => Some(0x22),   // VK_NEXT
        Key::Control => Some(0x11),    // VK_CONTROL
        Key::Alt => Some(0x12),        // VK_MENU
        Key::Shift => Some(0x10),      // VK_SHIFT
        Key::Meta => Some(0x5B),       // VK_LWIN
        Key::CapsLock => Some(0x14),   // VK_CAPITAL
        Key::F1 => Some(0x70),
        Key::F2 => Some(0x71),
        Key::F3 => Some(0x72),
        Key::F4 => Some(0x73),
        Key::F5 => Some(0x74),
        Key::F6 => Some(0x75),
        Key::F7 => Some(0x76),
        Key::F8 => Some(0x77),
        Key::F9 => Some(0x78),
        Key::F10 => Some(0x79),
        Key::F11 => Some(0x7A),
        Key::F12 => Some(0x7B),
        Key::Unicode(c) => {
            let vk = unsafe { VkKeyScanW(*c as u16) };
            if vk == -1 { None } else { Some((vk & 0xFF) as u32) }
        }
        _ => None,
    }
}
