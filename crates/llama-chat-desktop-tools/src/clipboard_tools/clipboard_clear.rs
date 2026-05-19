//! `clear_clipboard` tool — clear all clipboard content.

use serde_json::Value;

use crate::NativeToolResult;

#[cfg(windows)]
use crate::win32;

/// Clear all clipboard content.
#[cfg(windows)]
pub fn tool_clear_clipboard(_args: &Value) -> NativeToolResult {
    unsafe {
        if win32::OpenClipboard(0) == 0 {
            return crate::tool_error("clear_clipboard", "Failed to open clipboard");
        }
        win32::EmptyClipboard();
        win32::CloseClipboard();
    }
    NativeToolResult::text_only("Clipboard cleared".to_string())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn tool_clear_clipboard(_args: &Value) -> NativeToolResult {
    match arboard::Clipboard::new() {
        Ok(mut cb) => {
            let _ = cb.set_text(String::new());
            NativeToolResult::text_only("Clipboard cleared".to_string())
        }
        Err(e) => crate::tool_error("clear_clipboard", e),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_clear_clipboard(_args: &Value) -> NativeToolResult {
    crate::tool_error("clear_clipboard", "not available on this platform")
}
