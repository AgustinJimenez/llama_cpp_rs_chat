//! `clipboard_file_paths` tool — read or write file paths from/to clipboard.

use serde_json::Value;

use crate::NativeToolResult;

#[cfg(windows)]
use crate::win32;

/// Read or write file paths from/to clipboard.
/// Params: `action` ("read" or "write"), `paths` (array of strings, for write).
#[cfg(windows)]
pub fn tool_clipboard_file_paths(args: &Value) -> NativeToolResult {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("read");

    match action {
        "read" => {
            // Use existing win32::read_clipboard_files (CF_HDROP + DragQueryFileW)
            match win32::read_clipboard_files() {
                Ok(files) => {
                    if files.is_empty() {
                        NativeToolResult::text_only("No file paths in clipboard".to_string())
                    } else {
                        let mut output = format!("{} file path(s) in clipboard:\n", files.len());
                        for f in &files {
                            output.push_str(&format!("  {f}\n"));
                        }
                        NativeToolResult::text_only(output)
                    }
                }
                Err(e) => crate::tool_error("clipboard_file_paths", e),
            }
        }
        "write" => {
            // Writing CF_HDROP requires building a DROPFILES struct + null-terminated wide string list.
            let paths = match args.get("paths").and_then(|v| v.as_array()) {
                Some(arr) => arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>(),
                None => return crate::tool_error("clipboard_file_paths", "'paths' array is required for write"),
            };
            if paths.is_empty() {
                return crate::tool_error("clipboard_file_paths", "'paths' array must not be empty");
            }

            // DROPFILES struct is 20 bytes:
            //   DWORD pFiles (offset to file list)
            //   POINT pt (unused, {0,0})
            //   BOOL  fNC (0)
            //   BOOL  fWide (1 = Unicode)
            const DROPFILES_SIZE: usize = 20;

            // Build the wide string file list: each path null-terminated, double-null at end
            let mut wide_data: Vec<u16> = Vec::new();
            for p in &paths {
                wide_data.extend(p.encode_utf16());
                wide_data.push(0); // null terminator per path
            }
            wide_data.push(0); // double-null terminator

            let file_data_bytes = wide_data.len() * 2;
            let total_size = DROPFILES_SIZE + file_data_bytes;

            unsafe {
                let hmem = win32::GlobalAlloc(win32::GMEM_MOVEABLE, total_size);
                if hmem == 0 {
                    return crate::tool_error("clipboard_file_paths", "GlobalAlloc failed");
                }
                let ptr = win32::GlobalLock(hmem);
                if ptr.is_null() {
                    return crate::tool_error("clipboard_file_paths", "GlobalLock failed");
                }
                if total_size == 0 {
                    win32::GlobalUnlock(hmem);
                    return crate::tool_error("clipboard_file_paths", "zero-size allocation");
                }
                let buf = std::slice::from_raw_parts_mut(ptr, total_size);
                buf.fill(0);

                // DROPFILES header
                buf[0..4].copy_from_slice(&(DROPFILES_SIZE as u32).to_le_bytes()); // pFiles offset
                // pt.x, pt.y, fNC are 0 (already zeroed)
                buf[16..20].copy_from_slice(&1u32.to_le_bytes()); // fWide = TRUE

                // Copy wide string data after header
                let src_bytes = std::slice::from_raw_parts(
                    wide_data.as_ptr() as *const u8,
                    file_data_bytes,
                );
                buf[DROPFILES_SIZE..].copy_from_slice(src_bytes);

                win32::GlobalUnlock(hmem);

                if win32::OpenClipboard(0) == 0 {
                    return crate::tool_error("clipboard_file_paths", "Failed to open clipboard");
                }
                win32::EmptyClipboard();
                let result = win32::SetClipboardData(win32::CF_HDROP, hmem);
                win32::CloseClipboard();

                if result == 0 {
                    crate::tool_error("clipboard_file_paths", "SetClipboardData failed")
                } else {
                    NativeToolResult::text_only(format!("Wrote {} file path(s) to clipboard", paths.len()))
                }
            }
        }
        other => NativeToolResult::text_only(format!(
            "Unknown action '{other}'. Use 'read' or 'write'."
        )),
    }
}

#[cfg(target_os = "macos")]
pub fn tool_clipboard_file_paths(args: &Value) -> NativeToolResult {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("read");

    match action {
        "read" => {
            // Try to read file URLs from clipboard via osascript
            let output = std::process::Command::new("osascript")
                .args(["-e", "POSIX path of (the clipboard as «class furl»)"])
                .output();
            match output {
                Ok(out) if out.status.success() => {
                    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if text.is_empty() {
                        NativeToolResult::text_only("No file paths in clipboard".to_string())
                    } else {
                        let paths: Vec<&str> = text.lines().collect();
                        let mut result = format!("{} file path(s) in clipboard:\n", paths.len());
                        for p in &paths {
                            result.push_str(&format!("  {p}\n"));
                        }
                        NativeToolResult::text_only(result)
                    }
                }
                _ => NativeToolResult::text_only("No file paths in clipboard".to_string()),
            }
        }
        "write" => {
            let paths = match args.get("paths").and_then(|v| v.as_array()) {
                Some(p) => p,
                None => return crate::tool_error("clipboard_file_paths", "'paths' array required for write"),
            };
            let path_strs: Vec<&str> = paths.iter().filter_map(|v| v.as_str()).collect();
            if path_strs.is_empty() {
                return crate::tool_error("clipboard_file_paths", "no valid paths provided");
            }
            let posix_files: Vec<String> = path_strs.iter()
                .map(|p| format!("POSIX file \"{}\"", p.replace('\\', "\\\\").replace('"', "\\\"")))
                .collect();
            let script = format!("set the clipboard to {{{}}}", posix_files.join(", "));
            match std::process::Command::new("osascript").args(["-e", &script]).output() {
                Ok(out) if out.status.success() => {
                    NativeToolResult::text_only(format!("{} file path(s) written to clipboard", path_strs.len()))
                }
                Ok(out) => crate::tool_error("clipboard_file_paths",
                    format!("osascript: {}", String::from_utf8_lossy(&out.stderr).trim())),
                Err(e) => crate::tool_error("clipboard_file_paths", format!("osascript: {e}")),
            }
        }
        other => NativeToolResult::text_only(format!(
            "Unknown action '{other}'. Use 'read' or 'write'."
        )),
    }
}

#[cfg(target_os = "linux")]
pub fn tool_clipboard_file_paths(args: &Value) -> NativeToolResult {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("read");

    match action {
        "read" => {
            let output = std::process::Command::new("xclip")
                .args(["-selection", "clipboard", "-t", "text/uri-list", "-o"])
                .output();
            match output {
                Ok(out) if out.status.success() => {
                    let text = String::from_utf8_lossy(&out.stdout).to_string();
                    let paths: Vec<String> = text
                        .lines()
                        .filter(|l| l.starts_with("file://"))
                        .map(|l| l.strip_prefix("file://").unwrap_or(l).to_string())
                        .collect();
                    if paths.is_empty() {
                        NativeToolResult::text_only("No file paths in clipboard".to_string())
                    } else {
                        let mut result = format!("{} file path(s) in clipboard:\n", paths.len());
                        for p in &paths {
                            result.push_str(&format!("  {p}\n"));
                        }
                        NativeToolResult::text_only(result)
                    }
                }
                _ => NativeToolResult::text_only("No file paths in clipboard (xclip not available or no URI data)".to_string()),
            }
        }
        "write" => {
            let paths = match args.get("paths").and_then(|v| v.as_array()) {
                Some(arr) => arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>(),
                None => return crate::tool_error("clipboard_file_paths", "'paths' array is required for write"),
            };
            if paths.is_empty() {
                return crate::tool_error("clipboard_file_paths", "'paths' array must not be empty");
            }
            let uri_list: Vec<String> = paths.iter().map(|p| format!("file://{p}")).collect();
            let data = uri_list.join("\n");
            let mut child = match std::process::Command::new("xclip")
                .args(["-selection", "clipboard", "-t", "text/uri-list"])
                .stdin(std::process::Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => return crate::tool_error("clipboard_file_paths", format!("xclip: {e}")),
            };
            if let Some(stdin) = child.stdin.as_mut() {
                use std::io::Write;
                let _ = stdin.write_all(data.as_bytes());
            }
            let _ = child.wait();
            NativeToolResult::text_only(format!("Wrote {} file path(s) to clipboard", paths.len()))
        }
        other => NativeToolResult::text_only(format!(
            "Unknown action '{other}'. Use 'read' or 'write'."
        )),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_clipboard_file_paths(_args: &Value) -> NativeToolResult {
    crate::tool_error("clipboard_file_paths", "not available on this platform")
}
