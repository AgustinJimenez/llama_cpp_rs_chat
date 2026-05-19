//! Window management tool definitions.

use super::{p, Params, ToolDef};
use serde_json::{json, Value};

pub static WINDOW_TOOLS: &[ToolDef] = &[
    // ─── list_windows ───
    ToolDef {
        name: "list_windows",
        description: "List all visible windows on the desktop with their titles, positions, sizes, process names, and state (minimized/maximized/focused). Use this to find windows before clicking or interacting with them. Returns an indexed list you can reference by number.",
        params: Params::Simple(&[
            p("filter", "string", "Optional case-insensitive filter. Only windows whose title or process name contains this string will be returned."),
            p("pid", "integer", "Filter to windows of this process ID"),
        ]),
        required: &[],
    },
    // ─── focus_window ───
    ToolDef {
        name: "focus_window",
        description: "Bring a window to the foreground and give it focus. Prefer pid when you already know the target window identity; otherwise use a case-insensitive title or process-name filter. If the window is minimized, it will be restored first.",
        params: Params::Simple(&[
            p("title", "string", "Case-insensitive filter to match window title or process name (e.g. 'chrome', 'notepad')"),
            p("pid", "integer", "Specific process ID to target. Prefer this once you know the window identity."),
        ]),
        required: &[],
    },
    // ─── minimize_window ───
    ToolDef {
        name: "minimize_window",
        description: "Minimize a window to the taskbar. Prefer pid when you already know the target window identity; otherwise use a case-insensitive title or process-name filter.",
        params: Params::Simple(&[
            p("title", "string", "Case-insensitive filter to match window title or process name"),
            p("pid", "integer", "Specific process ID to target. Prefer this once you know the window identity."),
        ]),
        required: &[],
    },
    // ─── maximize_window ───
    ToolDef {
        name: "maximize_window",
        description: "Maximize a window to fill the screen. Prefer pid when you already know the target window identity; otherwise use a case-insensitive title or process-name filter.",
        params: Params::Simple(&[
            p("title", "string", "Case-insensitive filter to match window title or process name"),
            p("pid", "integer", "Specific process ID to target. Prefer this once you know the window identity."),
        ]),
        required: &[],
    },
    // ─── close_window ───
    ToolDef {
        name: "close_window",
        description: "Close a window gracefully by sending WM_CLOSE. Prefer pid when you already know the target window identity; otherwise use a case-insensitive title or process-name filter. The application may show a save dialog before closing.",
        params: Params::Simple(&[
            p("title", "string", "Case-insensitive filter to match window title or process name"),
            p("pid", "integer", "Specific process ID to target. Prefer this once you know the window identity."),
        ]),
        required: &[],
    },
    // ─── resize_window ───
    ToolDef {
        name: "resize_window",
        description: "Move and/or resize a window by pid, title, or process name. Prefer pid when you already know the target window identity. Provide at least one of x, y, width, height.",
        params: Params::Simple(&[
            p("title", "string", "Window title or process name to match (case-insensitive substring)"),
            p("pid", "integer", "Specific process ID to target. Prefer this once you know the window identity."),
            p("x", "integer", "New X position (screen coordinates)"),
            p("y", "integer", "New Y position (screen coordinates)"),
            p("width", "integer", "New width in pixels"),
            p("height", "integer", "New height in pixels"),
        ]),
        required: &[],
    },
    // ─── get_active_window ───
    ToolDef {
        name: "get_active_window",
        description: "Get info about the currently active (foreground) window: title, process, position, size.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── wait_for_window ───
    ToolDef {
        name: "wait_for_window",
        description: "Wait for a window with matching pid, title, or process name to appear. Polls until found or timeout. Prefer pid when you already know the target window identity.",
        params: Params::Simple(&[
            p("title", "string", "Window title or process name to wait for"),
            p("pid", "integer", "Specific process ID to wait for. Prefer this once you know the window identity."),
            p("timeout_ms", "integer", "Maximum wait time in ms (default 10000, max 60000)"),
            p("poll_ms", "integer", "Polling interval in ms (default 200)"),
        ]),
        required: &[],
    },
    // ─── set_window_topmost ───
    ToolDef {
        name: "set_window_topmost",
        description: "Set a window to always-on-top or remove always-on-top. Prefer pid when you already know the target window identity. Useful for keeping reference windows visible while working.",
        params: Params::Simple(&[
            p("title", "string", "Window title to modify"),
            p("pid", "integer", "Specific process ID to target. Prefer this once you know the window identity."),
            p("topmost", "boolean", "true = always on top, false = remove (default true)"),
        ]),
        required: &[],
    },
    // ─── snap_window ───
    ToolDef {
        name: "snap_window",
        description: "Snap a window to a screen position: left, right, top-left, top-right, bottom-left, bottom-right, center, maximize, restore. Prefer pid when you already know the target window identity. Uses monitor work area (excludes taskbar).",
        params: Params::Simple(&[
            p("title", "string", "Window title to snap"),
            p("pid", "integer", "Specific process ID to target. Prefer this once you know the window identity."),
            p("position", "string", "Position: left, right, top-left, top-right, bottom-left, bottom-right, center, maximize, restore"),
        ]),
        required: &["position"],
    },
    // ─── move_to_monitor ───
    ToolDef {
        name: "move_to_monitor",
        description: "Move a window to a specific monitor by index. Preserves window size.",
        params: Params::Simple(&[
            p("title", "string", "Window title or process name filter"),
            p("monitor", "integer", "Target monitor index (default 0)"),
        ]),
        required: &["title"],
    },
    // ─── set_window_opacity ───
    ToolDef {
        name: "set_window_opacity",
        description: "Set window transparency. 0 = fully transparent, 100 = fully opaque.",
        params: Params::Simple(&[
            p("title", "string", "Window title or process name filter"),
            p("opacity", "integer", "Opacity percentage 0-100 (default 100)"),
        ]),
        required: &["title"],
    },
    // ─── watch_window ───
    ToolDef {
        name: "watch_window",
        description: "Monitor for window changes (new windows, closed windows, title changes). Returns on first change or timeout.",
        params: Params::Simple(&[
            p("timeout_ms", "integer", "Maximum wait time (default 10000)"),
            p("filter", "string", "Only report changes for windows matching this filter"),
            p("poll_ms", "integer", "Polling interval (default 500)"),
        ]),
        required: &[],
    },
    // ─── save_window_layout ───
    ToolDef {
        name: "save_window_layout",
        description: "Save positions and sizes of all open windows to a named layout file.",
        params: Params::Simple(&[
            p("name", "string", "Layout name (used as filename)"),
        ]),
        required: &["name"],
    },
    // ─── restore_window_layout ───
    ToolDef {
        name: "restore_window_layout",
        description: "Restore windows to positions saved in a named layout file.",
        params: Params::Simple(&[
            p("name", "string", "Layout name to restore"),
        ]),
        required: &["name"],
    },
];

/// Complex window tools that require runtime JSON construction.
pub fn complex_window_tools() -> Vec<Value> {
    vec![
        // ─── get_ui_tree — has array param (exclude_types) ───
        json!({
            "name": "get_ui_tree",
            "description": "Get the UI element tree of a window using UI Automation. Shows control types and names. Works best with native Windows apps (Win32, WPF, WinForms). Returns empty for GPU-rendered apps (Blender, Unity, games, Electron) — use ocr_screen or take_screenshot instead for those.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Window title or process name (default: active window)" },
                    "depth": { "type": "integer", "description": "Max tree depth 1-8 (default 3)" },
                    "exclude_types": { "type": "array", "items": { "type": "string" }, "description": "Control types to exclude (e.g. ['image','separator','thumb'])" }
                },
                "required": []
            }
        }),
        // ─── fill_form — has complex array param ───
        json!({
            "name": "fill_form",
            "description": "Fill multiple form fields by finding UI elements by label and typing values. Each field is clicked, cleared, and filled.",
            "parameters": {
                "type": "object",
                "properties": {
                    "fields": {
                        "type": "array",
                        "description": "Array of {label, value} objects. Each field object can include \"type\": \"text|checkbox|dropdown|radio\" to force field type handling.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "label": { "type": "string" },
                                "value": { "type": "string" },
                                "type": { "type": "string", "description": "Force field type: text, checkbox, dropdown, or radio" }
                            }
                        }
                    },
                    "title": { "type": "string", "description": "Window title filter" }
                },
                "required": ["fields"]
            }
        }),
        // ─── run_action_sequence — has complex array param ───
        json!({
            "name": "run_action_sequence",
            "description": "Execute a sequence of desktop actions (click, type, press_key, paste, wait, clear, scroll, move). Each action is a JSON object with an 'action' field.",
            "parameters": {
                "type": "object",
                "properties": {
                    "actions": {
                        "type": "array",
                        "description": "Array of action objects. Each has 'action' (click/type/press_key/paste/wait/clear/scroll/move) plus params. Per-action options: 'retry' (0-3), 'if_previous' ('success'|'failure'), 'abort_on_failure' (boolean), 'screenshot_mode' ('final_only'|'all'|'none').",
                        "items": { "type": "object" }
                    },
                    "delay_between_ms": { "type": "integer", "description": "Default delay between actions (default 200)" }
                },
                "required": ["actions"]
            }
        }),
    ]
}
