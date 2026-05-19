//! `clipboard_html` tool — read or write HTML from/to clipboard.

use serde_json::Value;

use crate::NativeToolResult;

#[cfg(windows)]
use crate::win32;

/// Read or write HTML from/to clipboard.
/// Params: `action` ("read" or "write"), `html` (string, for write).
#[cfg(windows)]
pub fn tool_clipboard_html(args: &Value) -> NativeToolResult {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("read");

    // Register the "HTML Format" clipboard format
    let format_name: Vec<u16> = "HTML Format".encode_utf16().chain(std::iter::once(0)).collect();
    let cf_html = unsafe { win32::RegisterClipboardFormatW(format_name.as_ptr()) };
    if cf_html == 0 {
        return crate::tool_error("clipboard_html", "Failed to register HTML Format clipboard format");
    }

    match action {
        "read" => {
            unsafe {
                if win32::OpenClipboard(0) == 0 {
                    return crate::tool_error("clipboard_html", "Failed to open clipboard");
                }
                let handle = win32::GetClipboardData(cf_html);
                if handle == 0 {
                    win32::CloseClipboard();
                    return crate::tool_error("clipboard_html", "No HTML data in clipboard");
                }
                let size = win32::GlobalSize(handle);
                if size == 0 {
                    win32::CloseClipboard();
                    return crate::tool_error("clipboard_html", "GlobalSize returned 0");
                }
                let ptr = win32::GlobalLock(handle);
                if ptr.is_null() {
                    win32::CloseClipboard();
                    return crate::tool_error("clipboard_html", "GlobalLock failed");
                }
                let data = std::slice::from_raw_parts(ptr, size);
                let raw_str = String::from_utf8_lossy(data);

                win32::GlobalUnlock(handle);
                win32::CloseClipboard();

                // Parse CF_HTML header to extract the fragment
                // Look for StartFragment/EndFragment byte offsets
                let start_frag = raw_str
                    .lines()
                    .find(|l| l.starts_with("StartFragment:"))
                    .and_then(|l| l.trim_start_matches("StartFragment:").trim().parse::<usize>().ok());
                let end_frag = raw_str
                    .lines()
                    .find(|l| l.starts_with("EndFragment:"))
                    .and_then(|l| l.trim_start_matches("EndFragment:").trim().parse::<usize>().ok());

                match (start_frag, end_frag) {
                    (Some(start), Some(end)) if start < end && end <= data.len() => {
                        let fragment = String::from_utf8_lossy(&data[start..end]).to_string();
                        let summary = if fragment.len() > 500 {
                            format!("HTML fragment ({} chars): {}...", fragment.len(), &fragment[..500])
                        } else {
                            format!("HTML fragment: {fragment}")
                        };
                        NativeToolResult::text_only(summary)
                    }
                    _ => {
                        // Fallback: return raw content (trimmed)
                        let trimmed = raw_str.trim_end_matches('\0').to_string();
                        let summary = if trimmed.len() > 500 {
                            format!("HTML clipboard ({} chars): {}...", trimmed.len(), &trimmed[..500])
                        } else {
                            format!("HTML clipboard: {trimmed}")
                        };
                        NativeToolResult::text_only(summary)
                    }
                }
            }
        }
        "write" => {
            let html = match args.get("html").and_then(|v| v.as_str()) {
                Some(h) => h,
                None => return crate::tool_error("clipboard_html", "'html' argument is required for write"),
            };

            // Build CF_HTML format with proper header
            let fragment = html;
            let html_body = format!(
                "<html>\r\n<body>\r\n<!--StartFragment-->{fragment}<!--EndFragment-->\r\n</body>\r\n</html>"
            );

            // Calculate offsets — header fields are fixed-width with zero-padded numbers
            // Header format: Version + StartHTML + EndHTML + StartFragment + EndFragment
            let header_template = "Version:0.9\r\nStartHTML:0000000000\r\nEndHTML:0000000000\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\n";
            let header_len = header_template.len();
            let start_html = header_len;
            let start_fragment = header_len + html_body.find("<!--StartFragment-->").unwrap_or(0) + "<!--StartFragment-->".len();
            let end_fragment = header_len + html_body.find("<!--EndFragment-->").unwrap_or(html_body.len());
            let end_html = header_len + html_body.len();

            let header = format!(
                "Version:0.9\r\nStartHTML:{:010}\r\nEndHTML:{:010}\r\nStartFragment:{:010}\r\nEndFragment:{:010}\r\n",
                start_html, end_html, start_fragment, end_fragment
            );

            let cf_html_data = format!("{header}{html_body}");
            let bytes = cf_html_data.as_bytes();

            unsafe {
                let hmem = win32::GlobalAlloc(win32::GMEM_MOVEABLE, bytes.len() + 1);
                if hmem == 0 {
                    return crate::tool_error("clipboard_html", "GlobalAlloc failed");
                }
                let ptr = win32::GlobalLock(hmem);
                if ptr.is_null() {
                    return crate::tool_error("clipboard_html", "GlobalLock failed");
                }
                if bytes.is_empty() {
                    win32::GlobalUnlock(hmem);
                    return crate::tool_error("clipboard_html", "empty HTML content");
                }
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
                *ptr.add(bytes.len()) = 0; // null terminate
                win32::GlobalUnlock(hmem);

                if win32::OpenClipboard(0) == 0 {
                    return crate::tool_error("clipboard_html", "Failed to open clipboard");
                }
                win32::EmptyClipboard();
                let result = win32::SetClipboardData(cf_html, hmem);
                win32::CloseClipboard();

                if result == 0 {
                    crate::tool_error("clipboard_html", "SetClipboardData failed")
                } else {
                    NativeToolResult::text_only(format!("Wrote {} chars of HTML to clipboard", html.len()))
                }
            }
        }
        other => NativeToolResult::text_only(format!(
            "Unknown action '{other}'. Use 'read' or 'write'."
        )),
    }
}

#[cfg(target_os = "macos")]
pub fn tool_clipboard_html(args: &Value) -> NativeToolResult {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("read");

    match action {
        "read" => {
            let output = std::process::Command::new("osascript")
                .args(["-e", "the clipboard as «class HTML»"])
                .output();
            match output {
                Ok(out) if out.status.success() => {
                    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if text.is_empty() {
                        NativeToolResult::text_only("No HTML data in clipboard".to_string())
                    } else {
                        let summary = if text.len() > 500 {
                            format!("HTML clipboard ({} chars): {}...", text.len(), &text[..500])
                        } else {
                            format!("HTML clipboard: {text}")
                        };
                        NativeToolResult::text_only(summary)
                    }
                }
                _ => NativeToolResult::text_only("No HTML data in clipboard".to_string()),
            }
        }
        "write" => {
            let html = match args.get("html").and_then(|v| v.as_str()) {
                Some(h) => h,
                None => return crate::tool_error("clipboard_html", "'html' is required for write"),
            };
            // Use AppleScriptObjC to set HTML data on NSPasteboard
            let escaped = html.replace('\\', "\\\\").replace('"', "\\\"");
            let script = format!(
                concat!(
                    "use framework \"AppKit\"\n",
                    "set htmlData to (current application's NSString's stringWithString:\"{}\")",
                    "'s dataUsingEncoding:(current application's NSUTF8StringEncoding)\n",
                    "set pb to current application's NSPasteboard's generalPasteboard()\n",
                    "pb's clearContents()\n",
                    "pb's setData:htmlData forType:\"public.html\""
                ),
                escaped
            );
            match std::process::Command::new("osascript").args(["-l", "AppleScriptObjC", "-e", &script]).output() {
                Ok(out) if out.status.success() => {
                    NativeToolResult::text_only(format!("HTML written to clipboard ({} chars)", html.len()))
                }
                Ok(out) => crate::tool_error("clipboard_html",
                    format!("osascript: {}", String::from_utf8_lossy(&out.stderr).trim())),
                Err(e) => crate::tool_error("clipboard_html", format!("osascript: {e}")),
            }
        }
        other => NativeToolResult::text_only(format!(
            "Unknown action '{other}'. Use 'read' or 'write'."
        )),
    }
}

#[cfg(target_os = "linux")]
pub fn tool_clipboard_html(args: &Value) -> NativeToolResult {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("read");

    match action {
        "read" => {
            let output = std::process::Command::new("xclip")
                .args(["-selection", "clipboard", "-t", "text/html", "-o"])
                .output();
            match output {
                Ok(out) if out.status.success() => {
                    let text = String::from_utf8_lossy(&out.stdout).to_string();
                    if text.is_empty() {
                        NativeToolResult::text_only("No HTML data in clipboard".to_string())
                    } else {
                        let summary = if text.len() > 500 {
                            format!("HTML clipboard ({} chars): {}...", text.len(), &text[..500])
                        } else {
                            format!("HTML clipboard: {text}")
                        };
                        NativeToolResult::text_only(summary)
                    }
                }
                _ => NativeToolResult::text_only("No HTML data in clipboard (xclip not available or no HTML data)".to_string()),
            }
        }
        "write" => {
            let html = match args.get("html").and_then(|v| v.as_str()) {
                Some(h) => h,
                None => return crate::tool_error("clipboard_html", "'html' argument is required for write"),
            };
            let mut child = match std::process::Command::new("xclip")
                .args(["-selection", "clipboard", "-t", "text/html"])
                .stdin(std::process::Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => return crate::tool_error("clipboard_html", format!("xclip: {e}")),
            };
            if let Some(stdin) = child.stdin.as_mut() {
                use std::io::Write;
                let _ = stdin.write_all(html.as_bytes());
            }
            let _ = child.wait();
            NativeToolResult::text_only(format!("Wrote {} chars of HTML to clipboard", html.len()))
        }
        other => NativeToolResult::text_only(format!(
            "Unknown action '{other}'. Use 'read' or 'write'."
        )),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_clipboard_html(_args: &Value) -> NativeToolResult {
    crate::tool_error("clipboard_html", "not available on this platform")
}
