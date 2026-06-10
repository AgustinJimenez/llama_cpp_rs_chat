//! Input tools: mouse clicks, keyboard input, scrolling, dragging, and high-level
//! helpers (paste, clear field, hover element).

mod keyboard;
mod mouse;
mod high_level;

// ─── Public re-exports (preserve public API) ─────────────────────────────────

pub use keyboard::{tool_type_text, tool_press_key};
pub use mouse::{tool_click_screen, tool_move_mouse, tool_mouse_drag, tool_mouse_button, tool_scroll_screen};
pub use high_level::{tool_paste, tool_clear_field, tool_hover_element};

// ─── Scroll-to-text helper (used by mouse::tool_scroll_screen) ───────────────

use serde_json::Value;
use crate::helpers::{parse_int, tool_error, with_enigo};
use crate::trace::capture_post_action_screenshot;
use crate::{ensure_desktop_not_cancelled, interruptible_sleep, NativeToolResult};

/// Scroll repeatedly until OCR finds the target text on screen.
pub(super) fn scroll_to_text(args: &Value) -> NativeToolResult {
    let target_text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return tool_error("scroll_screen", "'text' is required for mode='to_text'"),
    };
    let direction = args.get("direction").and_then(|v| v.as_str()).unwrap_or("down");
    let max_scrolls = args
        .get("max_scrolls")
        .and_then(parse_int)
        .unwrap_or(20)
        .clamp(1, 50) as usize;
    let scroll_amount = if direction == "up" { -3 } else { 3 };
    let target_lower = target_text.to_lowercase();

    for i in 0..max_scrolls {
        if let Err(e) = ensure_desktop_not_cancelled() {
            return tool_error("scroll_screen", e);
        }

        // OCR current screen
        let ocr_result = crate::ocr_tools::tool_ocr_screen(&serde_json::json!({"monitor": 0}));
        if ocr_result.text.to_lowercase().contains(&target_lower) {
            let screenshot = capture_post_action_screenshot(0);
            return NativeToolResult {
                text: format!("Found '{target_text}' after {i} scroll(s)"),
                images: screenshot.images,
            };
        }
        // Scroll
        with_enigo(|enigo| {
            use enigo::Mouse;
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
        text: format!("Text '{target_text}' not found after {max_scrolls} scrolls {direction}"),
        images: screenshot.images,
    }
}

