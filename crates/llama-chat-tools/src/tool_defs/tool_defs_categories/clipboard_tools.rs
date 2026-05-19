//! Clipboard tool definitions.

use super::{p, Params, ToolDef};
use serde_json::{json, Value};

pub static CLIPBOARD_TOOLS: &[ToolDef] = &[
    // ─── read_clipboard ───
    ToolDef {
        name: "read_clipboard",
        description: "Read the current text content from the system clipboard.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── write_clipboard ───
    ToolDef {
        name: "write_clipboard",
        description: "Write text to the system clipboard, replacing its current content.",
        params: Params::Simple(&[
            p("text", "string", "The text to write to the clipboard"),
        ]),
        required: &["text"],
    },
    // ─── clear_clipboard ───
    ToolDef {
        name: "clear_clipboard",
        description: "Clear all content from the system clipboard.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── clipboard_image ───
    ToolDef {
        name: "clipboard_image",
        description: "Read or write images from/to the clipboard. Read returns the clipboard image as PNG. Write captures the screen and copies it to clipboard.",
        params: Params::Simple(&[
            p("action", "string", "read or write (default: read)"),
            p("monitor", "integer", "Monitor index for write action (default 0)"),
        ]),
        required: &[],
    },
    // ─── paste ───
    ToolDef {
        name: "paste",
        description: "Paste clipboard contents at the current cursor position (Ctrl+V). Takes a screenshot after pasting.",
        params: Params::Simple(&[
            p("delay_ms", "integer", "Wait after paste before screenshot (default 300)"),
        ]),
        required: &[],
    },
];

/// Complex clipboard tools with array parameters.
pub fn complex_clipboard_tools() -> Vec<Value> {
    vec![
        // ─── clipboard_file_paths — has array param ───
        json!({
            "name": "clipboard_file_paths",
            "description": "Read or write file paths on the clipboard (e.g. copied files in a file manager).",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "'read' to get file paths from clipboard, 'write' to put file paths on clipboard" },
                    "paths": { "type": "array", "description": "File paths to write (required for action='write')" }
                },
                "required": ["action"]
            }
        }),
        // ─── clipboard_html ───
        json!({
            "name": "clipboard_html",
            "description": "Read or write HTML content on the clipboard.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "'read' to get HTML from clipboard, 'write' to put HTML on clipboard" },
                    "html": { "type": "string", "description": "HTML content to write (required for action='write')" }
                },
                "required": ["action"]
            }
        }),
    ]
}
