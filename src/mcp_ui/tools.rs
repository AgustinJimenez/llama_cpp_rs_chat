//! Tool schema definitions for the embedded MCP UI server.
//!
//! `build_tools()` returns the full list of `Tool` objects that the server
//! advertises to MCP clients. Schemas are plain JSON objects — no runtime logic.

use std::sync::Arc;

use rmcp::model::Tool;
use serde_json::{Value, json};

pub fn build_tools() -> Vec<Tool> {
    let defs: Vec<(&str, &str, Value)> = vec![
        // ─── App UI tools ───
        ("app_click", "Click a UI element by CSS selector.", json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector (e.g. 'button.submit', '#login')" }
            },
            "required": ["selector"]
        })),
        ("app_type", "Type text into an input or textarea by CSS selector. Set submit=true to auto-click the send/submit button after.", json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector of the input" },
                "text": { "type": "string", "description": "Text to type" },
                "submit": { "type": "boolean", "description": "Click submit after typing (default: false)" }
            },
            "required": ["selector", "text"]
        })),
        ("app_read", "Read text content of a UI element by CSS selector.", json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector to read from" }
            },
            "required": ["selector"]
        })),
        ("app_list_elements", "List all interactive UI elements (buttons, inputs, links). Returns tag, text, and selector for each. Use filter to narrow results.", json!({
            "type": "object",
            "properties": {
                "filter": { "type": "string", "description": "Optional text filter to match element text or selector" }
            }
        })),
        ("app_eval", "Execute arbitrary JavaScript in the app's webview and return the result. Use for anything the other tools can't do.", json!({
            "type": "object",
            "properties": {
                "js": { "type": "string", "description": "JavaScript expression or statement to evaluate" }
            },
            "required": ["js"]
        })),
        ("app_get_state", "Get current app state: model loaded/path, generating status, loading status. No arguments needed.", json!({
            "type": "object", "properties": {}
        })),
        ("app_load_model", "Load a GGUF model by file path. Uses the app's worker bridge directly.", json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Full path to the .gguf model file" }
            },
            "required": ["path"]
        })),
        ("app_send_message", "Type a message into the chat input and send it. The model will start generating a response.", json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "Message text to send" }
            },
            "required": ["text"]
        })),
        ("app_wait_for", "Wait for a CSS selector to appear on the page (e.g. after navigation or generation). Returns when found or on timeout.", json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector to wait for" },
                "timeout_ms": { "type": "integer", "description": "Max wait time in ms (default: 10000)" }
            },
            "required": ["selector"]
        })),
        ("app_navigate_browser", "Open a URL in the app's browser view panel.", json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "URL to navigate to" }
            },
            "required": ["url"]
        })),
        ("app_screenshot", "Get the visible text content of the entire app page. Returns innerText of the body (no image).", json!({
            "type": "object", "properties": {}
        })),
        // ─── Browser panel tools ───
        ("browser_navigate", "Open a URL in the browser panel (user-visible embedded browser). Opens the browser panel if not already open.", json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "URL to navigate to" }
            },
            "required": ["url"]
        })),
        ("browser_read", "Read text content from the browser panel page. Optionally scope to a CSS selector.", json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector to read from (default: entire page)" },
                "max_length": { "type": "integer", "description": "Max characters to return (default: 30000)" }
            }
        })),
        ("browser_click", "Click an element in the browser panel by CSS selector.", json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector of the element to click" }
            },
            "required": ["selector"]
        })),
        ("browser_type", "Type text into an input field in the browser panel.", json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector of the input" },
                "text": { "type": "string", "description": "Text to type" },
                "submit": { "type": "boolean", "description": "Submit the form after typing (default: false)" }
            },
            "required": ["selector", "text"]
        })),
        ("browser_eval", "Execute JavaScript in the browser panel webview. Returns the result.", json!({
            "type": "object",
            "properties": {
                "js": { "type": "string", "description": "JavaScript to evaluate in the browser panel" }
            },
            "required": ["js"]
        })),
        ("browser_get_url", "Get the current URL of the browser panel.", json!({
            "type": "object", "properties": {}
        })),
        ("browser_list_links", "List all links on the browser panel page. Optionally filter by text or URL.", json!({
            "type": "object",
            "properties": {
                "filter": { "type": "string", "description": "Filter links by text or URL substring" }
            }
        })),
        ("browser_list_elements", "List interactive elements (buttons, inputs, links) in the browser panel.", json!({
            "type": "object",
            "properties": {
                "filter": { "type": "string", "description": "Filter by element text or selector" }
            }
        })),
        ("browser_scroll", "Scroll the browser panel page up or down.", json!({
            "type": "object",
            "properties": {
                "direction": { "type": "string", "description": "Scroll direction: 'up' or 'down' (default: 'down')" },
                "amount": { "type": "integer", "description": "Scroll amount in pixels (default: 500)" }
            }
        })),
        ("browser_screenshot", "Take a screenshot of the browser panel. Returns the file path of the saved PNG.", json!({
            "type": "object", "properties": {}
        })),
        ("browser_close", "Close the browser panel.", json!({
            "type": "object", "properties": {}
        })),
    ];

    defs.into_iter()
        .filter_map(|(name, desc, schema)| {
            let map = match schema {
                Value::Object(m) => m,
                _ => return None,
            };
            Some(Tool::new(name, desc, Arc::new(map)))
        })
        .collect()
}
