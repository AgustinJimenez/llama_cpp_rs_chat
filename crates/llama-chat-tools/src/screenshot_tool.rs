//! Screenshot tool handler — delegates to the desktop-tools crate.

use serde_json::Value;
use super::NativeToolResult;

pub fn tool_take_screenshot_with_image(args: &Value) -> NativeToolResult {
    llama_chat_desktop_tools::tool_take_screenshot_with_image(args)
}
