//! Mouse, keyboard, and pointer input tool definitions.
//! These tools use the verification parameter set and are built at runtime.

use super::{p, ParamDef, VERIFY_PARAMS};
use serde_json::{json, Value};

/// Helper: build a tool JSON with verification params merged in.
fn tool_with_verify(
    name: &str,
    description: &str,
    base_params: &[ParamDef],
    extra_params: &[ParamDef],
    required: &[&str],
) -> Value {
    let mut properties = serde_json::Map::new();
    for bp in base_params.iter().chain(extra_params.iter()) {
        properties.insert(
            bp.name.to_string(),
            json!({ "type": bp.param_type, "description": bp.description }),
        );
    }
    for vp in VERIFY_PARAMS {
        properties.insert(
            vp.name.to_string(),
            json!({ "type": vp.param_type, "description": vp.description }),
        );
    }
    let req: Vec<&str> = required.to_vec();
    json!({
        "name": name,
        "description": description,
        "parameters": {
            "type": "object",
            "properties": properties,
            "required": req,
        }
    })
}

/// Simple (non-verify) input tools — move_mouse, get_cursor_position, mouse_button,
/// switch_virtual_desktop.
pub static SIMPLE_INPUT_TOOLS: &[super::ToolDef] = &[
    // ─── move_mouse ───
    super::ToolDef {
        name: "move_mouse",
        description: "Move the mouse cursor to screen coordinates without clicking. Does not take a screenshot.",
        params: super::Params::Simple(&[
            p("x", "integer", "X coordinate in pixels from left edge of screen"),
            p("y", "integer", "Y coordinate in pixels from top edge of screen"),
        ]),
        required: &["x", "y"],
    },
    // ─── get_cursor_position ───
    super::ToolDef {
        name: "get_cursor_position",
        description: "Get the current mouse cursor position on screen. Returns x,y coordinates in pixels.",
        params: super::Params::Simple(&[]),
        required: &[],
    },
    // ─── mouse_button ───
    super::ToolDef {
        name: "mouse_button",
        description: "Press or release a mouse button independently without clicking. Useful for hold-and-drag scenarios where you need separate press and release control.",
        params: super::Params::Simple(&[
            p("action", "string", "Action to perform: press or release"),
            p("button", "string", "Mouse button: left, right, middle (default: left)"),
            p("screenshot", "boolean", "Take screenshot after action (default true)"),
        ]),
        required: &["action"],
    },
    // ─── switch_virtual_desktop ───
    super::ToolDef {
        name: "switch_virtual_desktop",
        description: "Switch to an adjacent virtual desktop using Ctrl+Win+Arrow keyboard shortcut.",
        params: super::Params::Simple(&[
            p("direction", "string", "Direction: left/prev or right/next"),
        ]),
        required: &["direction"],
    },
];

/// Input tools that include the verification parameter set (built at runtime).
pub fn complex_input_tools() -> Vec<Value> {
    vec![
        // ─── click_screen — has verification params ───
        tool_with_verify(
            "click_screen",
            "Click the mouse at screen coordinates. Takes a screenshot after clicking by default; pass screenshot=false during long automation sessions to avoid bloating context. Use take_screenshot first to see the screen and identify coordinates. Use stealth=true for non-disruptive clicks that won't interrupt the user.",
            &[
                p("x", "integer", "X coordinate in pixels from left edge of screen"),
                p("y", "integer", "Y coordinate in pixels from top edge of screen"),
                p("button", "string", "Mouse button: 'left' (default), 'right', 'middle', 'double' (double left click)"),
                p("stealth", "boolean", "Stealth mode (Windows only, default: true): saves cursor position, clicks target in <0.1ms, restores cursor. User is not interrupted. Skips if user is mid-drag. Set to false for actions that need the cursor to stay at the target (e.g. hover menus)."),
                p("screenshot", "boolean", "Take a screenshot after clicking (default: true). Set to false to avoid embedding a full-screen capture in the result."),
                p("delay_ms", "integer", "Milliseconds to wait after clicking before taking screenshot (default: 500). Increase for slow UI animations."),
                p("dpi_aware", "boolean", "If true, coordinates are logical (96 DPI basis) and will be scaled to physical pixels by the system DPI factor (default: false)"),
            ],
            &[
                p("snap_to_screen", "boolean", "Clamp off-screen coordinates to nearest monitor edge"),
                p("timeout_ms", "integer", "Operation timeout in ms (1000-60000, default 20000)"),
            ],
            &["x", "y"],
        ),
        // ─── type_text — has verification params ───
        tool_with_verify(
            "type_text",
            "Type text using the keyboard. Simulates real keyboard input character by character. Falls back to SendInput Unicode on Windows for non-Latin characters. Use click_screen first to focus the target input field.",
            &[
                p("text", "string", "The text to type"),
                p("screenshot", "boolean", "Take a screenshot after typing (default: true)"),
                p("delay_ms", "integer", "Milliseconds to wait after typing before screenshot (default: 300)"),
            ],
            &[
                p("retry", "integer", "Retry count 0-3 on failure (default 0)"),
                p("timeout_ms", "integer", "Operation timeout in ms (1000-60000, default 20000)"),
            ],
            &["text"],
        ),
        // ─── press_key — has verification params ───
        tool_with_verify(
            "press_key",
            "Press a key or key combination. Supports modifiers (ctrl, alt, shift, meta/win) and special keys (enter, tab, escape, backspace, delete, up, down, left, right, home, end, pageup, pagedown, f1-f12, space). For combinations use '+': 'ctrl+c', 'ctrl+shift+s', 'alt+tab', 'alt+f4'.",
            &[
                p("key", "string", "Key or key combination. Examples: 'enter', 'tab', 'ctrl+c', 'ctrl+shift+s', 'alt+tab', 'f5'"),
                p("screenshot", "boolean", "Take a screenshot after key press (default: true)"),
                p("delay_ms", "integer", "Milliseconds to wait after key press before screenshot (default: 500)"),
            ],
            &[
                p("retry", "integer", "Retry count 0-3 on failure (default 0)"),
                p("timeout_ms", "integer", "Operation timeout in ms (1000-60000, default 20000)"),
            ],
            &["key"],
        ),
        // ─── scroll_screen — has verification params + extra fields ───
        tool_with_verify(
            "scroll_screen",
            "Scroll the mouse wheel at the current or specified position. Positive amount scrolls down, negative scrolls up. Each unit is about 3 lines of text.",
            &[
                p("amount", "integer", "Scroll amount: positive = down, negative = up. Each unit is ~3 lines."),
                p("x", "integer", "X coordinate to scroll at (optional, uses current position if omitted)"),
                p("y", "integer", "Y coordinate to scroll at (optional, uses current position if omitted)"),
                p("horizontal", "boolean", "Scroll horizontally instead of vertically (default: false)"),
                p("screenshot", "boolean", "Take a screenshot after scrolling (default: true)"),
                p("delay_ms", "integer", "Milliseconds to wait after scrolling before screenshot (default: 300)"),
            ],
            &[
                p("mode", "string", "'amount' (default) or 'to_text' (scroll until text appears via OCR)"),
                p("text", "string", "Text to find when mode='to_text'"),
                p("max_scrolls", "integer", "Max scroll attempts for to_text mode (default 20)"),
                p("snap_to_screen", "boolean", "Clamp off-screen coordinates to nearest monitor edge"),
                p("dpi_aware", "boolean", "Apply DPI scaling to coordinates"),
            ],
            &["amount"],
        ),
        // ─── mouse_drag — has verification params ───
        tool_with_verify(
            "mouse_drag",
            "Click and drag the mouse from one position to another. Useful for resizing windows, selecting text, moving objects, or drawing.",
            &[
                p("from_x", "integer", "Starting X coordinate (pixels from left edge)"),
                p("from_y", "integer", "Starting Y coordinate (pixels from top edge)"),
                p("to_x", "integer", "Ending X coordinate"),
                p("to_y", "integer", "Ending Y coordinate"),
                p("button", "string", "Mouse button to use: left (default) or right"),
                p("screenshot", "boolean", "Take a screenshot after dragging (default: true). Set to false to avoid embedding a full-screen capture in the result."),
                p("delay_ms", "integer", "Milliseconds to wait after drag before screenshot (default: 500)"),
            ],
            &[
                p("steps", "integer", "Intermediate points for smooth drag (1=instant, max 100). Increase for drawing or slider control."),
                p("snap_to_screen", "boolean", "Clamp off-screen coordinates to nearest monitor edge"),
                p("timeout_ms", "integer", "Operation timeout in ms (1000-60000, default 20000)"),
            ],
            &["from_x", "from_y", "to_x", "to_y"],
        ),
        // ─── click_window_relative — has verification params ───
        tool_with_verify(
            "click_window_relative",
            "Click at coordinates relative to a window's top-left corner. Focuses the window first. Prefer pid when you already know the target window identity.",
            &[
                p("title", "string", "Window title or process name to match"),
                p("pid", "integer", "Specific process ID to target. Prefer this once you know the window identity."),
                p("x", "integer", "X offset from window's left edge"),
                p("y", "integer", "Y offset from window's top edge"),
                p("button", "string", "Mouse button: left, right, middle, double (default: left)"),
                p("screenshot", "boolean", "Take a screenshot after clicking (default: true). Set to false to avoid embedding a full-screen capture in the result."),
                p("delay_ms", "integer", "Delay before screenshot in ms (default 500)"),
            ],
            &[],
            &["x", "y"],
        ),
    ]
}
