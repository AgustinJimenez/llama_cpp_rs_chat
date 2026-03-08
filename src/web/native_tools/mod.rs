//! Native file I/O and code execution tools.
//!
//! Provides safe, shell-free implementations of common operations that LLM agents
//! need: reading/writing files, running Python code, and listing directories.
//! This eliminates shell quoting issues that break `python -c "..."` on Windows.

/// Result from a native tool, carrying text output and optional image data.
/// Image data is used by vision-capable models to "see" tool outputs (e.g., screenshots).
#[derive(Debug)]
pub struct NativeToolResult {
    pub text: String,
    /// Raw image bytes (PNG/JPEG) for vision pipeline injection.
    /// Only populated by tools like `take_screenshot` when capture succeeds.
    pub images: Vec<Vec<u8>>,
}

impl NativeToolResult {
    pub fn text_only(text: String) -> Self {
        Self { text, images: Vec::new() }
    }
    pub fn with_image(text: String, image_bytes: Vec<u8>) -> Self {
        Self { text, images: vec![image_bytes] }
    }
}

use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use lazy_static::lazy_static;

lazy_static! {
    /// Per-URL cache for web_fetch results within a session.
    /// Prevents re-fetching the same URL multiple times in a conversation.
    /// Key: URL, Value: fetched content string.
    static ref WEB_FETCH_CACHE: Mutex<HashMap<String, String>> = Mutex::new(HashMap::new());
}

/// Clear the web fetch cache (call on new conversation or model reload).
pub fn clear_web_fetch_cache() {
    if let Ok(mut cache) = WEB_FETCH_CACHE.lock() {
        cache.clear();
    }
}

mod parsing;
pub use parsing::*;
use parsing::{value_as_bool_flexible, try_parse_with_fixups};
mod web_search;
use web_search::tool_web_search;
#[cfg(test)]
use web_search::{tool_web_search_ddg_api, parse_ddg_results};
mod web_fetch;
pub use web_fetch::*;
use web_fetch::tool_web_fetch;

/// Maximum file size to return inline (100 KB).
const MAX_READ_SIZE: usize = 100 * 1024;

/// Maximum characters to return from web page fetch.
const MAX_FETCH_CHARS: usize = 15_000;

/// If the text is an `execute_command` tool call, extract the command string and background flag.
/// Returns `(command, is_background)`.
/// Used by the command executor to route `execute_command` through streaming or background path.
pub fn extract_execute_command_with_opts(text: &str) -> Option<(String, bool)> {
    // First try the standard tool call format: {"name":"execute_command","arguments":{"command":"...","background":true}}
    if let Some((name, args)) = try_parse_tool_call(text) {
        if name == "execute_command" {
            let command = args.get("command").and_then(|v| v.as_str())?;
            if !command.is_empty() {
                let background = args.get("background").and_then(value_as_bool_flexible).unwrap_or(false);
                return Some((command.to_string(), background));
            }
        }
        return None;
    }

    // Fallback: some models (GLM) put bare arguments without the name/arguments wrapper,
    // e.g. {"command": "...", "background": true} inside SYSTEM.EXEC tags
    let trimmed = text.trim();
    if let Some(parsed) = try_parse_with_fixups(trimmed) {
        if let Some(command) = parsed.get("command").and_then(|v| v.as_str()) {
            if !command.is_empty() {
                let background = parsed.get("background").and_then(value_as_bool_flexible).unwrap_or(false);
                return Some((command.to_string(), background));
            }
        }
    }
    None
}

/// Try to dispatch a tool call to a native handler.
///
/// Supports multiple formats:
/// 1. Standard JSON: `{"name": "read_file", "arguments": {"path": "..."}}`
/// 2. Mistral array:  `[{"name":"read_file","arguments":{"path":"..."}}]`
/// 3. Mistral comma:  `read_file,{"path": "..."}` (Devstral native format)
/// 4. Llama3 XML:     `<function=read_file> <parameter=path> value </parameter> </function>`
/// 5. Name+JSON:      `read_file{"path": "..."}` (Granite native format)
///
/// Returns `Some(output)` if recognized, `None` to fall back to shell.
///
/// Note: `execute_command` is handled here as a blocking fallback. The command executor
/// should prefer `extract_execute_command_with_opts()` + streaming/background path.
pub fn dispatch_native_tool(
    text: &str,
    web_search_provider: Option<&str>,
    web_search_api_key: Option<&str>,
    use_htmd: bool,
    mcp_manager: Option<&super::mcp::McpManager>,
) -> Option<NativeToolResult> {
    let trimmed = text.trim();

    // Try all supported tool call formats (JSON, Mistral comma, Llama3 XML, GLM XML, etc.)
    let mut calls = try_parse_all_from_raw(trimmed);
    let (name, args) = if let Some(first) = calls.drain(..).next() {
        first
    } else {
        return None;
    };

    // Desktop automation tools return NativeToolResult directly (may carry image bytes for vision)
    if name == "take_screenshot" {
        return Some(tool_take_screenshot_with_image(&args));
    }
    if name == "click_screen" {
        return Some(super::desktop_tools::tool_click_screen(&args));
    }
    if name == "type_text" {
        return Some(super::desktop_tools::tool_type_text(&args));
    }
    if name == "press_key" {
        return Some(super::desktop_tools::tool_press_key(&args));
    }
    if name == "move_mouse" {
        return Some(super::desktop_tools::tool_move_mouse(&args));
    }
    if name == "scroll_screen" {
        return Some(super::desktop_tools::tool_scroll_screen(&args));
    }
    if name == "list_windows" {
        return Some(super::desktop_tools::tool_list_windows(&args));
    }
    if name == "mouse_drag" {
        return Some(super::desktop_tools::tool_mouse_drag(&args));
    }
    if name == "get_cursor_position" {
        return Some(super::desktop_tools::tool_get_cursor_position(&args));
    }
    if name == "focus_window" {
        return Some(super::desktop_tools::tool_focus_window(&args));
    }
    if name == "minimize_window" {
        return Some(super::desktop_tools::tool_minimize_window(&args));
    }
    if name == "maximize_window" {
        return Some(super::desktop_tools::tool_maximize_window(&args));
    }
    if name == "close_window" {
        return Some(super::desktop_tools::tool_close_window(&args));
    }
    if name == "read_clipboard" {
        return Some(super::desktop_tools::tool_read_clipboard(&args));
    }
    if name == "write_clipboard" {
        return Some(super::desktop_tools::tool_write_clipboard(&args));
    }
    if name == "resize_window" {
        return Some(super::desktop_tools::tool_resize_window(&args));
    }
    if name == "get_active_window" {
        return Some(super::desktop_tools::tool_get_active_window(&args));
    }
    if name == "wait_for_window" {
        return Some(super::desktop_tools::tool_wait_for_window(&args));
    }
    if name == "get_pixel_color" {
        return Some(super::desktop_tools::tool_get_pixel_color(&args));
    }
    if name == "click_window_relative" {
        return Some(super::desktop_tools::tool_click_window_relative(&args));
    }
    if name == "list_monitors" {
        return Some(super::desktop_tools::tool_list_monitors(&args));
    }
    if name == "screenshot_region" {
        return Some(super::desktop_tools::tool_screenshot_region(&args));
    }
    if name == "screenshot_diff" {
        return Some(super::desktop_tools::tool_screenshot_diff(&args));
    }
    if name == "ocr_screen" {
        return Some(super::desktop_tools::tool_ocr_screen(&args));
    }
    if name == "get_ui_tree" {
        return Some(super::desktop_tools::tool_get_ui_tree(&args));
    }
    if name == "ocr_find_text" {
        return Some(super::desktop_tools::tool_ocr_find_text(&args));
    }
    if name == "click_ui_element" {
        return Some(super::desktop_tools::tool_click_ui_element(&args));
    }
    if name == "window_screenshot" {
        return Some(super::desktop_tools::tool_window_screenshot(&args));
    }
    if name == "open_application" {
        return Some(super::desktop_tools::tool_open_application(&args));
    }
    if name == "wait_for_screen_change" {
        return Some(super::desktop_tools::tool_wait_for_screen_change(&args));
    }
    if name == "set_window_topmost" {
        return Some(super::desktop_tools::tool_set_window_topmost(&args));
    }

    if name == "invoke_ui_action" {
        return Some(super::desktop_tools::tool_invoke_ui_action(&args));
    }

    if name == "read_ui_element_value" {
        return Some(super::desktop_tools::tool_read_ui_element_value(&args));
    }

    if name == "wait_for_ui_element" {
        return Some(super::desktop_tools::tool_wait_for_ui_element(&args));
    }

    if name == "clipboard_image" {
        return Some(super::desktop_tools::tool_clipboard_image(&args));
    }

    if name == "find_ui_elements" {
        return Some(super::desktop_tools::tool_find_ui_elements(&args));
    }

    if name == "execute_app_script" {
        return Some(super::desktop_tools::tool_execute_app_script(&args));
    }

    if name == "send_keys_to_window" {
        return Some(super::desktop_tools::tool_send_keys_to_window(&args));
    }

    if name == "snap_window" {
        return Some(super::desktop_tools::tool_snap_window(&args));
    }

    if name == "list_processes" {
        return Some(super::desktop_tools::tool_list_processes(&args));
    }

    if name == "kill_process" {
        return Some(super::desktop_tools::tool_kill_process(&args));
    }

    // Compound desktop tools (combine multiple primitives)
    if name == "find_and_click_text" {
        return Some(super::desktop_tools::tool_find_and_click_text(&args));
    }
    if name == "type_into_element" {
        return Some(super::desktop_tools::tool_type_into_element(&args));
    }
    if name == "get_window_text" {
        return Some(super::desktop_tools::tool_get_window_text(&args));
    }
    if name == "file_dialog_navigate" {
        return Some(super::desktop_tools::tool_file_dialog_navigate(&args));
    }
    if name == "drag_and_drop_element" {
        return Some(super::desktop_tools::tool_drag_and_drop_element(&args));
    }
    if name == "wait_for_text_on_screen" {
        return Some(super::desktop_tools::tool_wait_for_text_on_screen(&args));
    }
    if name == "get_context_menu" {
        return Some(super::desktop_tools::tool_get_context_menu(&args));
    }
    if name == "scroll_element" {
        return Some(super::desktop_tools::tool_scroll_element(&args));
    }
    if name == "mouse_button" {
        return Some(super::desktop_tools::tool_mouse_button(&args));
    }
    if name == "switch_virtual_desktop" {
        return Some(super::desktop_tools::tool_switch_virtual_desktop(&args));
    }
    if name == "find_image_on_screen" {
        return Some(super::desktop_tools::tool_find_image_on_screen(&args));
    }
    if name == "get_process_info" {
        return Some(super::desktop_tools::tool_get_process_info(&args));
    }
    if name == "paste" {
        return Some(super::desktop_tools::tool_paste(&args));
    }
    if name == "clear_field" {
        return Some(super::desktop_tools::tool_clear_field(&args));
    }
    if name == "hover_element" {
        return Some(super::desktop_tools::tool_hover_element(&args));
    }
    if name == "handle_dialog" {
        return Some(super::desktop_tools::tool_handle_dialog(&args));
    }
    if name == "wait_for_element_state" {
        return Some(super::desktop_tools::tool_wait_for_element_state(&args));
    }
    if name == "fill_form" {
        return Some(super::desktop_tools::tool_fill_form(&args));
    }
    if name == "run_action_sequence" {
        return Some(super::desktop_tools::tool_run_action_sequence(&args));
    }
    if name == "move_to_monitor" {
        return Some(super::desktop_tools::tool_move_to_monitor(&args));
    }
    if name == "set_window_opacity" {
        return Some(super::desktop_tools::tool_set_window_opacity(&args));
    }
    if name == "highlight_point" {
        return Some(super::desktop_tools::tool_highlight_point(&args));
    }
    if name == "annotate_screenshot" {
        return Some(super::desktop_tools::tool_annotate_screenshot(&args));
    }
    if name == "ocr_region" {
        return Some(super::desktop_tools::tool_ocr_region(&args));
    }
    if name == "find_color_on_screen" {
        return Some(super::desktop_tools::tool_find_color_on_screen(&args));
    }
    if name == "read_registry" {
        return Some(super::desktop_tools::tool_read_registry(&args));
    }
    if name == "click_tray_icon" {
        return Some(super::desktop_tools::tool_click_tray_icon(&args));
    }
    if name == "watch_window" {
        return Some(super::desktop_tools::tool_watch_window(&args));
    }

    // All other tools return text-only results
    Some(NativeToolResult::text_only(match name.as_str() {
        "read_file" => tool_read_file(&args),
        "write_file" => tool_write_file(&args),
        "edit_file" => tool_edit_file(&args),
        "undo_edit" => tool_undo_edit(&args),
        "insert_text" => tool_insert_text(&args),
        "search_files" => tool_search_files(&args),
        "find_files" => tool_find_files(&args),
        "execute_python" => tool_execute_python(&args),
        "list_directory" => tool_list_directory(&args),
        "web_search" => tool_web_search(&args, web_search_provider, web_search_api_key),
        "web_fetch" => tool_web_fetch(&args, use_htmd),
        "execute_command" => {
            // Delegate to shell execution via command.rs
            let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
            if command.is_empty() {
                return Some(NativeToolResult::text_only("Error: 'command' argument is required".to_string()));
            }
            super::command::execute_command(command)
        }
        "check_background_process" => {
            // PID may be a JSON number or a string (Llama3 XML format returns strings)
            let pid = args.get("pid").and_then(|v| {
                v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
            }).unwrap_or(0) as u32;
            if pid == 0 {
                return Some(NativeToolResult::text_only("Error: 'pid' argument is required and must be a positive integer".to_string()));
            }
            // Optional wait_seconds: sleep before checking (merges wait + check into one call)
            let wait_seconds = args.get("wait_seconds").and_then(|v| {
                v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
            }).unwrap_or(0);
            super::command::check_background_process(pid, wait_seconds)
        }
        "wait" => {
            // Legacy: still supported but models should prefer check_background_process(wait_seconds=N)
            let seconds = args.get("seconds").and_then(|v| {
                v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
            }).unwrap_or(10);
            let seconds = seconds.min(30);
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            format!("Waited {} seconds. You can now check on background processes or continue.", seconds)
        }
        "git_status" => tool_git_status(&args),
        "git_diff" => tool_git_diff(&args),
        "git_commit" => tool_git_commit(&args),
        _ => {
            // Check if it's an MCP tool before falling back to shell
            if let Some(mgr) = mcp_manager {
                if mgr.is_mcp_tool(&name) {
                    return Some(NativeToolResult::text_only(match mgr.call_tool(&name, args) {
                        Ok(output) => output,
                        Err(e) => format!("MCP tool error: {e}"),
                    }));
                }
            }
            return None; // Unknown tool → fall back to shell
        }
    }))
}

/// Read a file and return its contents.
fn tool_read_file(args: &Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument is required".to_string(),
    };

    let path_lower = path.to_ascii_lowercase();

    // PDF files: extract text instead of returning binary garbage
    if path_lower.ends_with(".pdf") {
        return match std::fs::read(path) {
            Ok(bytes) => extract_pdf_text(&bytes, MAX_READ_SIZE),
            Err(e) => format!("Error reading '{path}': {e}"),
        };
    }

    // DOCX files: extract text from ZIP/XML structure
    if path_lower.ends_with(".docx") {
        return match std::fs::read(path) {
            Ok(bytes) => extract_docx_text(&bytes, MAX_READ_SIZE),
            Err(e) => format!("Error reading '{path}': {e}"),
        };
    }

    // PPTX files: extract text from ZIP/XML structure
    if path_lower.ends_with(".pptx") {
        return match std::fs::read(path) {
            Ok(bytes) => extract_pptx_text(&bytes, MAX_READ_SIZE),
            Err(e) => format!("Error reading '{path}': {e}"),
        };
    }

    // XLSX/XLS files: extract spreadsheet data as tab-separated text
    if path_lower.ends_with(".xlsx") || path_lower.ends_with(".xls") || path_lower.ends_with(".xlsm") {
        return match std::fs::read(path) {
            Ok(bytes) => extract_xlsx_text(&bytes, MAX_READ_SIZE),
            Err(e) => format!("Error reading '{path}': {e}"),
        };
    }

    // EPUB ebooks: extract text from XHTML content files
    if path_lower.ends_with(".epub") {
        return match std::fs::read(path) {
            Ok(bytes) => extract_epub_text(&bytes, MAX_READ_SIZE),
            Err(e) => format!("Error reading '{path}': {e}"),
        };
    }

    // ODT (LibreOffice Writer): extract text from content.xml
    if path_lower.ends_with(".odt") {
        return match std::fs::read(path) {
            Ok(bytes) => extract_odt_text(&bytes, MAX_READ_SIZE),
            Err(e) => format!("Error reading '{path}': {e}"),
        };
    }

    // RTF: extract plain text
    if path_lower.ends_with(".rtf") {
        return match std::fs::read(path) {
            Ok(bytes) => extract_rtf_text(&bytes, MAX_READ_SIZE),
            Err(e) => format!("Error reading '{path}': {e}"),
        };
    }

    // ZIP archives: list contents
    if path_lower.ends_with(".zip") || path_lower.ends_with(".7z") || path_lower.ends_with(".tar.gz") || path_lower.ends_with(".tgz") {
        return match std::fs::read(path) {
            Ok(bytes) => extract_zip_listing(&bytes, MAX_READ_SIZE),
            Err(e) => format!("Error reading '{path}': {e}"),
        };
    }

    // CSV files: structured parsing with headers
    if path_lower.ends_with(".csv") {
        return match std::fs::read(path) {
            Ok(bytes) => extract_csv_structured(&bytes, MAX_READ_SIZE),
            Err(e) => format!("Error reading '{path}': {e}"),
        };
    }

    // Email files: extract headers, body, attachment listing
    if path_lower.ends_with(".eml") || path_lower.ends_with(".msg") {
        return match std::fs::read(path) {
            Ok(bytes) => extract_eml_text(&bytes, MAX_READ_SIZE),
            Err(e) => format!("Error reading '{path}': {e}"),
        };
    }

    // Try UTF-8 first, fall back to encoding detection for non-UTF8 files
    match std::fs::read_to_string(path) {
        Ok(content) => truncate_text_content(&content, MAX_READ_SIZE),
        Err(e) => {
            // If UTF-8 failed, try reading as raw bytes and detect encoding
            if e.kind() == std::io::ErrorKind::InvalidData {
                match std::fs::read(path) {
                    Ok(bytes) => read_with_encoding_detection(&bytes, MAX_READ_SIZE),
                    Err(e2) => format!("Error reading '{path}': {e2}"),
                }
            } else {
                format!("Error reading '{path}': {e}")
            }
        }
    }
}

/// Truncate text content to max_chars with a notice.
pub fn truncate_text_content(content: &str, max_chars: usize) -> String {
    let total_bytes = content.len();
    if total_bytes > max_chars {
        let mut end = max_chars;
        while end > 0 && !content.is_char_boundary(end) { end -= 1; }
        format!(
            "{}\n\n[Truncated: showing first {} of {} bytes]",
            &content[..end], end, total_bytes
        )
    } else {
        content.to_string()
    }
}

/// Read non-UTF8 bytes using encoding_rs auto-detection (Latin-1, Shift-JIS, etc.).
pub fn read_with_encoding_detection(bytes: &[u8], max_chars: usize) -> String {
    // Try common encodings: UTF-8 BOM, then let encoding_rs detect
    let (decoded, encoding_used, had_errors) = encoding_rs::Encoding::for_bom(bytes)
        .map(|(enc, _)| enc.decode(bytes))
        .unwrap_or_else(|| {
            // No BOM — try Windows-1252 (Latin-1 superset, most common non-UTF8 on Windows)
            encoding_rs::WINDOWS_1252.decode(bytes)
        });

    let label = if had_errors { " (with some decoding errors)" } else { "" };
    let header = format!("[Decoded from {} encoding{}]\n", encoding_used.name(), label);
    let content = format!("{}{}", header, decoded);
    truncate_text_content(&content, max_chars)
}

/// Write content to a file, creating parent directories as needed.
fn tool_write_file(args: &Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument is required".to_string(),
    };
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: 'content' argument is required".to_string(),
    };

    // Create parent directories if they don't exist
    if let Some(parent) = Path::new(path).parent() {
        if !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return format!("Error creating directories for '{path}': {e}");
            }
        }
    }

    match std::fs::write(path, content) {
        Ok(()) => format!("Written {} bytes to {}", content.len(), path),
        Err(e) => format!("Error writing '{path}': {e}"),
    }
}

/// Edit a file by replacing an exact string match.
/// `old_string` must appear exactly once in the file (uniqueness check).
fn tool_edit_file(args: &Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument is required".to_string(),
    };
    let old_string = match args.get("old_string").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return "Error: 'old_string' argument is required".to_string(),
    };
    let new_string = args.get("new_string").and_then(|v| v.as_str()).unwrap_or("");

    if old_string.is_empty() {
        return "Error: 'old_string' cannot be empty".to_string();
    }

    // Read the file
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return format!("Error reading '{path}': {e}"),
    };

    // Count occurrences
    let match_count = content.matches(old_string).count();
    if match_count == 0 {
        return format!("Error: old_string not found in {path}. Make sure the text matches exactly (including whitespace and newlines).");
    }
    if match_count > 1 {
        return format!("Error: old_string found {match_count} times in {path}. Include more surrounding context to make it unique.");
    }

    // Find the line number of the match for the success message
    let match_pos = content.find(old_string).unwrap();
    let line_num = content[..match_pos].lines().count().max(1);

    // Perform the replacement
    let new_content = content.replacen(old_string, new_string, 1);

    // Save backup for undo_edit
    let backup_path = format!("{path}.llama_bak");
    let _ = std::fs::write(&backup_path, &content);

    match std::fs::write(path, &new_content) {
        Ok(()) => format!("Edited {path}: replaced {} chars with {} chars at line {line_num}", old_string.len(), new_string.len()),
        Err(e) => format!("Error writing '{path}': {e}"),
    }
}

/// Undo the last edit_file operation by restoring the backup.
fn tool_undo_edit(args: &Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument is required".to_string(),
    };

    let backup_path = format!("{path}.llama_bak");
    let backup_content = match std::fs::read_to_string(&backup_path) {
        Ok(c) => c,
        Err(_) => return format!("Error: no backup found for {path}. Only the most recent edit_file can be undone."),
    };

    match std::fs::write(path, &backup_content) {
        Ok(()) => {
            let _ = std::fs::remove_file(&backup_path);
            format!("Restored {path} to its state before the last edit")
        }
        Err(e) => format!("Error restoring '{path}': {e}"),
    }
}

/// Insert text at a specific line number in a file.
fn tool_insert_text(args: &Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument is required".to_string(),
    };
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return "Error: 'text' argument is required".to_string(),
    };
    // Line number may be JSON number or string
    let line = args.get("line").and_then(|v| {
        v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
    }).unwrap_or(0) as usize;
    if line == 0 {
        return "Error: 'line' argument is required and must be >= 1".to_string();
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return format!("Error reading '{path}': {e}"),
    };

    let mut lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    // Clamp insertion point: line 1 = before first line, line N+1 = after last line
    let insert_idx = (line - 1).min(total_lines);

    // Split the inserted text into lines
    let new_lines: Vec<&str> = text.lines().collect();
    let inserted_count = new_lines.len();

    for (i, new_line) in new_lines.into_iter().enumerate() {
        lines.insert(insert_idx + i, new_line);
    }

    // Preserve trailing newline if original had one
    let mut new_content = lines.join("\n");
    if content.ends_with('\n') {
        new_content.push('\n');
    }

    match std::fs::write(path, &new_content) {
        Ok(()) => format!("Inserted {inserted_count} line(s) at line {line} in {path}"),
        Err(e) => format!("Error writing '{path}': {e}"),
    }
}

const MAX_SEARCH_MATCHES: usize = 50;
const MAX_SEARCH_OUTPUT_CHARS: usize = 8000;
const MAX_SEARCH_FILE_SIZE: u64 = 2 * 1024 * 1024; // 2MB — skip large files

/// Check if a file is binary by looking at first 512 bytes for null bytes.
fn is_binary_file(path: &std::path::Path) -> bool {
    if let Ok(mut f) = std::fs::File::open(path) {
        use std::io::Read;
        let mut buf = [0u8; 512];
        if let Ok(n) = f.read(&mut buf) {
            return buf[..n].contains(&0);
        }
    }
    false
}

/// Truncate a string to max chars.
fn truncate_line(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { s.chars().take(max).collect() }
}

/// Build an ignore::WalkBuilder with .gitignore support and optional include/exclude globs.
fn build_walker(
    search_path: &str,
    include: &str,
    exclude: &str,
) -> ignore::Walk {
    let mut builder = ignore::WalkBuilder::new(search_path);
    builder
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true);

    // Build overrides: plain patterns = whitelist, !patterns = blacklist
    let has_overrides = !include.is_empty() || !exclude.is_empty();
    if has_overrides {
        let mut overrides = ignore::overrides::OverrideBuilder::new(search_path);
        for pat in include.split(',') {
            let pat = pat.trim();
            if !pat.is_empty() {
                let _ = overrides.add(pat);
            }
        }
        for pat in exclude.split(',') {
            let pat = pat.trim();
            if !pat.is_empty() {
                let _ = overrides.add(&format!("!{pat}"));
            }
        }
        if let Ok(ov) = overrides.build() {
            builder.overrides(ov);
        }
    }

    builder.build()
}

/// Search file contents by pattern (literal or regex) across a directory.
/// Uses the `ignore` crate for .gitignore-aware traversal.
fn tool_search_files(args: &Value) -> String {
    let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'pattern' argument is required".to_string(),
    };
    let search_path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let include = args.get("include").and_then(|v| v.as_str()).unwrap_or("");
    let exclude = args.get("exclude").and_then(|v| v.as_str()).unwrap_or("");
    let context = args.get("context").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

    // Try as regex first, fall back to literal
    let re = match regex::Regex::new(pattern) {
        Ok(r) => r,
        Err(_) => match regex::Regex::new(&regex::escape(pattern)) {
            Ok(r) => r,
            Err(e) => return format!("Error: invalid pattern: {e}"),
        },
    };

    let walker = build_walker(search_path, include, exclude);

    let mut results = Vec::new();
    let mut total_matches = 0;
    let mut files_matched = 0;

    for entry in walker.filter_map(|e| e.ok()) {
        if total_matches >= MAX_SEARCH_MATCHES { break; }
        if !entry.file_type().map_or(false, |ft| ft.is_file()) { continue; }

        let path = entry.path();

        // Skip large files (>2MB) to avoid memory issues
        if std::fs::metadata(path).map_or(false, |m| m.len() > MAX_SEARCH_FILE_SIZE) { continue; }
        if is_binary_file(path) { continue; }

        if let Ok(content) = std::fs::read_to_string(path) {
            let lines: Vec<&str> = content.lines().collect();
            let display_path = path.to_string_lossy();
            let mut file_had_match = false;
            // Track last emitted line to merge overlapping context
            let mut last_emitted: usize = 0;

            for (i, line) in lines.iter().enumerate() {
                if total_matches >= MAX_SEARCH_MATCHES { break; }
                if !re.is_match(line) { continue; }

                if !file_had_match {
                    file_had_match = true;
                    files_matched += 1;
                }
                total_matches += 1;

                if context > 0 {
                    // Before context — skip lines already emitted
                    let ctx_start = i.saturating_sub(context).max(last_emitted);
                    if ctx_start > last_emitted && last_emitted > 0 {
                        results.push("--".to_string()); // gap separator
                    }
                    for ci in ctx_start..i {
                        results.push(format!(
                            "{display_path}-{}: {}", ci + 1, truncate_line(lines[ci], 200)
                        ));
                    }
                }

                // Match line
                results.push(format!(
                    "{display_path}:{}: {}", i + 1, truncate_line(line, 200)
                ));

                if context > 0 {
                    // After context
                    let end = (i + context).min(lines.len().saturating_sub(1));
                    for ci in (i + 1)..=end {
                        results.push(format!(
                            "{display_path}-{}: {}", ci + 1, truncate_line(lines[ci], 200)
                        ));
                    }
                    last_emitted = end + 1;
                } else {
                    last_emitted = i + 1;
                }
            }
        }
    }

    if results.is_empty() {
        return format!("No matches found for '{pattern}' in {search_path}");
    }

    let mut output = format!(
        "Found {total_matches} match(es) across {files_matched} file(s) for '{pattern}':\n\n"
    );
    let mut chars = output.len();
    for line in &results {
        if chars + line.len() > MAX_SEARCH_OUTPUT_CHARS {
            output.push_str(&format!(
                "\n... (output truncated, {total_matches} total matches across {files_matched} files)"
            ));
            break;
        }
        output.push_str(line);
        output.push('\n');
        chars += line.len() + 1;
    }
    output
}

const MAX_FIND_RESULTS: usize = 100;

/// Find files by glob-like pattern recursively.
/// Uses the `ignore` crate for .gitignore-aware traversal.
fn tool_find_files(args: &Value) -> String {
    let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'pattern' argument is required".to_string(),
    };
    let search_path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let exclude = args.get("exclude").and_then(|v| v.as_str()).unwrap_or("");

    let walker = build_walker(search_path, pattern, exclude);

    let mut results = Vec::new();
    for entry in walker.filter_map(|e| e.ok()) {
        if results.len() >= MAX_FIND_RESULTS { break; }
        if !entry.file_type().map_or(false, |ft| ft.is_file()) { continue; }
        results.push(entry.path().to_string_lossy().to_string());
    }

    if results.is_empty() {
        return format!("No files matching '{pattern}' found in {search_path}");
    }

    let total = results.len();
    let truncated = total >= MAX_FIND_RESULTS;
    let mut output = format!("Found {total} file(s) matching '{pattern}':\n");
    for path in &results {
        output.push_str(path);
        output.push('\n');
    }
    if truncated {
        output.push_str(&format!("... (results capped at {MAX_FIND_RESULTS})\n"));
    }
    output
}

/// Execute Python code by writing to a temp file and running it.
/// This completely bypasses shell quoting — the code goes directly to a .py file.
fn tool_execute_python(args: &Value) -> String {
    let code = match args.get("code").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: 'code' argument is required".to_string(),
    };

    // Write code to a temp file
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!(
        "llama_tool_{}.py",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    if let Err(e) = std::fs::write(&temp_file, code) {
        return format!("Error writing temp file: {e}");
    }

    // Run python on the temp file — no shell involved
    let result = Command::new("python")
        .arg(&temp_file)
        .output();

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_file);

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.is_empty() {
                format!("{stdout}\nStderr: {stderr}")
            } else if stdout.is_empty() {
                "Python script executed successfully (no output)".to_string()
            } else {
                stdout.to_string()
            }
        }
        Err(e) => format!("Error running Python: {e}"),
    }
}

/// List directory contents with name, size, and type.
fn tool_list_directory(args: &Value) -> String {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or(".");

    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) => return format!("Error reading directory '{path}': {e}"),
    };

    let mut lines = Vec::new();
    lines.push(format!("Directory listing: {path}"));
    lines.push(format!("{:<40} {:>10} {}", "Name", "Size", "Type"));
    lines.push("-".repeat(60));

    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by_key(|e| e.file_name());

    for entry in sorted {
        let name = entry.file_name().to_string_lossy().to_string();
        let metadata = entry.metadata();
        let (size, file_type) = match metadata {
            Ok(m) => {
                let ft = if m.is_dir() {
                    "<DIR>"
                } else if m.is_symlink() {
                    "<LINK>"
                } else {
                    "<FILE>"
                };
                (m.len(), ft)
            }
            Err(_) => (0, "<?>"),
        };
        lines.push(format!("{name:<40} {size:>10} {file_type}"));
    }

    lines.join("\n")
}

/// Show git status of a repository.
fn tool_git_status(args: &Value) -> String {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let mut cmd = std::process::Command::new("git");
    cmd.arg("status").arg("--short");
    cmd.current_dir(path);
    cmd.stdin(std::process::Stdio::null());
    match cmd.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !output.status.success() {
                return format!("Error (exit {}): {}", output.status.code().unwrap_or(-1), stderr.trim());
            }
            if stdout.trim().is_empty() {
                "Working tree clean (no changes)".to_string()
            } else {
                format!("Git status:\n{}", stdout)
            }
        }
        Err(e) => format!("Error running git: {e}"),
    }
}

/// Show git diff for files.
fn tool_git_diff(args: &Value) -> String {
    let path = args.get("path").and_then(|v| v.as_str());
    let staged = args.get("staged").and_then(|v| {
        v.as_bool().or_else(|| v.as_str().map(|s| s.eq_ignore_ascii_case("true")))
    }).unwrap_or(false);

    let mut cmd = std::process::Command::new("git");
    cmd.arg("diff");
    if staged {
        cmd.arg("--staged");
    }
    if let Some(p) = path {
        // If path looks like a repo dir, use current_dir; otherwise it's a file arg
        if std::path::Path::new(p).is_dir() {
            cmd.current_dir(p);
        } else {
            cmd.arg("--").arg(p);
        }
    }
    cmd.stdin(std::process::Stdio::null());
    match cmd.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !output.status.success() {
                return format!("Error (exit {}): {}", output.status.code().unwrap_or(-1), stderr.trim());
            }
            if stdout.trim().is_empty() {
                "No differences found".to_string()
            } else {
                stdout.to_string()
            }
        }
        Err(e) => format!("Error running git: {e}"),
    }
}

/// Commit staged changes with a message.
fn tool_git_commit(args: &Value) -> String {
    let message = match args.get("message").and_then(|v| v.as_str()) {
        Some(m) if !m.is_empty() => m,
        _ => return "Error: 'message' argument is required".to_string(),
    };
    let all = args.get("all").and_then(|v| {
        v.as_bool().or_else(|| v.as_str().map(|s| s.eq_ignore_ascii_case("true")))
    }).unwrap_or(false);

    let mut cmd = std::process::Command::new("git");
    cmd.arg("commit");
    if all {
        cmd.arg("-a");
    }
    cmd.arg("-m").arg(message);
    cmd.stdin(std::process::Stdio::null());
    match cmd.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !output.status.success() {
                format!("Error (exit {}): {}\n{}", output.status.code().unwrap_or(-1), stderr.trim(), stdout.trim())
            } else {
                stdout.to_string()
            }
        }
        Err(e) => format!("Error running git: {e}"),
    }
}

/// Capture a screenshot — returns NativeToolResult with image bytes for vision pipeline.
pub(crate) fn tool_take_screenshot_with_image(args: &Value) -> NativeToolResult {
    let monitor_idx = args
        .get("monitor")
        .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
        .unwrap_or(0);

    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return NativeToolResult::text_only(format!("Error: Failed to enumerate monitors: {e}")),
    };

    if monitors.is_empty() {
        return NativeToolResult::text_only("Error: No monitors detected".to_string());
    }

    // List monitors mode (no image captured)
    if monitor_idx == -1 {
        let mut result = format!("Available monitors ({}):\n", monitors.len());
        for (i, mon) in monitors.iter().enumerate() {
            let name = mon.name().unwrap_or_else(|_| "Unknown".to_string());
            let w = mon.width().unwrap_or(0);
            let h = mon.height().unwrap_or(0);
            let primary = mon.is_primary().unwrap_or(false);
            result.push_str(&format!(
                "  [{}] {} - {}x{}{}\n",
                i, name, w, h,
                if primary { " (primary)" } else { "" }
            ));
        }
        return NativeToolResult::text_only(result);
    }

    // Select monitor
    let monitor = if monitor_idx == 0 {
        monitors
            .iter()
            .find(|m| m.is_primary().unwrap_or(false))
            .unwrap_or(&monitors[0])
    } else {
        let idx = monitor_idx as usize;
        if idx >= monitors.len() {
            return NativeToolResult::text_only(format!(
                "Error: Monitor index {} out of range (0-{})",
                idx,
                monitors.len() - 1
            ));
        }
        &monitors[idx]
    };

    // Capture
    let image = match monitor.capture_image() {
        Ok(img) => img,
        Err(e) => return NativeToolResult::text_only(format!("Error: Screenshot capture failed: {e}")),
    };

    let width = image.width();
    let height = image.height();
    let mon_name = monitor.name().unwrap_or_else(|_| "Unknown".to_string());
    let is_primary = monitor.is_primary().unwrap_or(false);

    // Save to temp directory
    let screenshots_dir = std::env::temp_dir().join("llama_screenshots");
    if let Err(e) = std::fs::create_dir_all(&screenshots_dir) {
        return NativeToolResult::text_only(format!("Error: Failed to create screenshots directory: {e}"));
    }

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let filename = format!("screenshot_{timestamp}.png");
    let filepath = screenshots_dir.join(&filename);

    if let Err(e) = image.save(&filepath) {
        return NativeToolResult::text_only(format!("Error: Failed to save screenshot: {e}"));
    }

    let text = format!(
        "Screenshot saved: {}\nResolution: {}x{}\nMonitor: {} (primary: {})",
        filepath.display(),
        width,
        height,
        mon_name,
        if is_primary { "yes" } else { "no" }
    );

    // Also encode the image as PNG bytes for vision pipeline injection
    let png_bytes = std::fs::read(&filepath).unwrap_or_default();
    if png_bytes.is_empty() {
        NativeToolResult::text_only(text)
    } else {
        NativeToolResult::with_image(text, png_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::parsing::{escape_newlines_in_json_strings, auto_close_json, escape_invalid_backslashes_in_strings};
    use serde_json::json;

    #[test]
    fn test_dispatch_read_file_valid() {
        // Create a temp file to read
        let temp = std::env::temp_dir().join("native_tools_test_read.txt");
        std::fs::write(&temp, "hello world").unwrap();

        let json = format!(
            r#"{{"name": "read_file", "arguments": {{"path": "{}"}}}}"#,
            temp.display().to_string().replace('\\', "\\\\")
        );
        let result = dispatch_native_tool(&json, None, None, false, None);
        assert!(result.is_some());
        assert!(result.unwrap().text.contains("hello world"));

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_write_file() {
        let temp = std::env::temp_dir().join("native_tools_test_write.txt");
        let json = format!(
            r#"{{"name": "write_file", "arguments": {{"path": "{}", "content": "test content"}}}}"#,
            temp.display().to_string().replace('\\', "\\\\")
        );
        let result = dispatch_native_tool(&json, None, None, false, None);
        assert!(result.is_some());
        assert!(result.unwrap().text.contains("Written"));
        assert_eq!(std::fs::read_to_string(&temp).unwrap(), "test content");

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_write_file_multiline_json_content() {
        // Models often emit multiline JSON content with literal newlines
        let temp = std::env::temp_dir().join("native_tools_test_multiline.json");
        let json = format!(
            "{{\n  \"name\": \"write_file\",\n  \"arguments\": {{\n    \"path\": \"{}\",\n    \"content\": \"{{\n  \\\"name\\\": \\\"test\\\",\n  \\\"version\\\": \\\"1.0.0\\\"\n}}\"\n  }}\n}}",
            temp.display().to_string().replace('\\', "\\\\")
        );
        let result = dispatch_native_tool(&json, None, None, false, None);
        assert!(result.is_some(), "Should parse multiline JSON content: {json}");
        assert!(result.unwrap().text.contains("Written"));
        let content = std::fs::read_to_string(&temp).unwrap();
        assert!(content.contains("\"name\": \"test\""));
        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_escape_newlines_in_json_strings() {
        let input = r#"{"name": "write_file", "arguments": {"content": "line1
line2
line3"}}"#;
        let escaped = escape_newlines_in_json_strings(input);
        let parsed: Value = serde_json::from_str(&escaped).unwrap();
        let content = parsed["arguments"]["content"].as_str().unwrap();
        assert_eq!(content, "line1\nline2\nline3");
    }

    #[test]
    fn test_dispatch_list_directory() {
        let json = r#"{"name": "list_directory", "arguments": {"path": "."}}"#;
        let result = dispatch_native_tool(json, None, None, false, None);
        assert!(result.is_some());
        assert!(result.unwrap().text.contains("Directory listing"));
    }

    #[test]
    fn test_dispatch_unknown_tool_returns_none() {
        let json = r#"{"name": "unknown_tool", "arguments": {}}"#;
        let result = dispatch_native_tool(json, None, None, false, None);
        assert!(result.is_none());
    }

    #[test]
    fn test_dispatch_non_json_returns_none() {
        let result = dispatch_native_tool("ls -la", None, None, false, None);
        assert!(result.is_none());
    }

    #[test]
    fn test_dispatch_mistral_array_format() {
        let temp = std::env::temp_dir().join("native_tools_test_mistral.txt");
        std::fs::write(&temp, "mistral test").unwrap();

        let json = format!(
            r#"[{{"name": "read_file", "arguments": {{"path": "{}"}}}}]"#,
            temp.display().to_string().replace('\\', "\\\\")
        );
        let result = dispatch_native_tool(&json, None, None, false, None);
        assert!(result.is_some());
        assert!(result.unwrap().text.contains("mistral test"));

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_mistral_comma_format() {
        // Devstral outputs: read_file,{"path": "file.txt"}
        let temp = std::env::temp_dir().join("native_tools_test_comma.txt");
        std::fs::write(&temp, "comma format test").unwrap();

        let input = format!(
            r#"read_file,{{"path": "{}"}}"#,
            temp.display().to_string().replace('\\', "\\\\")
        );
        let result = dispatch_native_tool(&input, None, None, false, None);
        assert!(result.is_some(), "Should parse Mistral comma format");
        assert!(result.unwrap().text.contains("comma format test"));

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_mistral_comma_execute_command() {
        // Devstral: execute_command,{"command": "echo hello"}
        let input = r#"execute_command,{"command": "echo hello"}"#;
        let result = dispatch_native_tool(input, None, None, false, None);
        assert!(result.is_some(), "Should parse comma format execute_command");
        assert!(result.unwrap().text.contains("hello"));
    }

    #[test]
    fn test_dispatch_llama3_xml_format() {
        // Qwen3-Coder outputs: <function=read_file> <parameter=path> file.txt </parameter> </function>
        let temp = std::env::temp_dir().join("native_tools_test_xml.txt");
        std::fs::write(&temp, "xml format test").unwrap();

        let input = format!(
            "<function=read_file> <parameter=path> {} </parameter> </function>",
            temp.display()
        );
        let result = dispatch_native_tool(&input, None, None, false, None);
        assert!(result.is_some(), "Should parse Llama3 XML format");
        assert!(result.unwrap().text.contains("xml format test"));

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_llama3_xml_write_file() {
        let temp = std::env::temp_dir().join("native_tools_test_xml_write.txt");
        let input = format!(
            "<function=write_file> <parameter=path> {} </parameter> <parameter=content> hello world </parameter> </function>",
            temp.display()
        );
        let result = dispatch_native_tool(&input, None, None, false, None);
        assert!(result.is_some(), "Should parse Llama3 XML write_file");
        assert!(result.unwrap().text.contains("Written"));
        assert_eq!(std::fs::read_to_string(&temp).unwrap(), "hello world");

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_name_json_format() {
        // Granite outputs: list_directory{"path": "."}
        let input = r#"list_directory{"path": "."}"#;
        let result = dispatch_native_tool(input, None, None, false, None);
        assert!(result.is_some(), "Should parse name+JSON format");
        assert!(result.unwrap().text.contains("Directory listing"));
    }

    #[test]
    fn test_execute_python_simple() {
        let json = r#"{"name": "execute_python", "arguments": {"code": "print('hello from python')"}}"#;
        let result = dispatch_native_tool(json, None, None, false, None);
        assert!(result.is_some());
        let output = result.unwrap().text;
        // If python is available, should contain the output; if not, should contain an error
        assert!(output.contains("hello from python") || output.contains("Error"));
    }

    #[test]
    fn test_execute_python_with_quotes_and_regex() {
        // This is the exact scenario that breaks with shell execution
        let code = r#"import re
text = "Invoice INV-2024-0847 total $1,234.56"
match = re.search(r'\$[\d,]+\.\d+', text)
print(f"Found: {match.group()}" if match else "No match")"#;

        let args = json!({"code": code});
        let result = tool_execute_python(&args);
        // If python is available
        if !result.contains("Error running Python") {
            assert!(result.contains("Found: $1,234.56"));
        }
    }

    #[test]
    fn test_auto_close_json_missing_brace() {
        // GLM model pattern: emits write_file JSON missing the outer closing }
        let input = r#"{"name": "write_file", "arguments": {"path": "/tmp/test.txt", "content": "hello"}}"#;
        // Valid JSON - should parse fine
        assert!(serde_json::from_str::<Value>(input).is_ok());

        // Now remove the last } to simulate GLM's bug
        let broken = &input[..input.len() - 1]; // {"name": "write_file", "arguments": {"path": "/tmp/test.txt", "content": "hello"}}  -> missing last }
        assert!(serde_json::from_str::<Value>(broken).is_err());

        let fixed = auto_close_json(broken);
        assert_eq!(fixed, input); // Should add back the missing }
        assert!(serde_json::from_str::<Value>(&fixed).is_ok());
    }

    #[test]
    fn test_dispatch_write_file_missing_brace_with_newlines() {
        // Exact pattern GLM produces: multiline content + missing outer closing }
        let json = "{\"name\": \"write_file\", \"arguments\": {\"path\": \"/tmp/test-autoclose.txt\", \"content\": \"{\n  \\\"name\\\": \\\"test\\\",\n  \\\"version\\\": \\\"1.0.0\\\"\n}\"}}";
        // This should work (has both braces)
        let result = dispatch_native_tool(json, None, None, false, None);
        assert!(result.is_some(), "Valid JSON should work: {:?}", result);
        let _ = std::fs::remove_file("/tmp/test-autoclose.txt");

        // Now test with missing outer brace (GLM pattern)
        let broken = "{\"name\": \"write_file\", \"arguments\": {\"path\": \"/tmp/test-autoclose2.txt\", \"content\": \"{\n  \\\"name\\\": \\\"test\\\",\n  \\\"version\\\": \\\"1.0.0\\\"\n}\"}}";
        // Remove last }
        let broken = &broken[..broken.len() - 1];
        let result = dispatch_native_tool(broken, None, None, false, None);
        assert!(result.is_some(), "Should auto-close missing brace and dispatch write_file");
        let output = result.unwrap().text;
        assert!(output.contains("written") || output.contains("success") || output.contains("Written"),
            "Should write successfully: {}", output);
        let _ = std::fs::remove_file("/tmp/test-autoclose2.txt");
    }

    #[test]
    fn test_escape_invalid_backslashes_php_namespaces() {
        // PHP namespaces like Illuminate\Database produce \D which is invalid JSON escape
        let input = r#"{"name":"write_file","arguments":{"path":"Person.php","content":"namespace App\Models;\nuse Illuminate\Database\Eloquent\Model;"}}"#;
        let fixed = escape_invalid_backslashes_in_strings(input);
        // Should double the backslashes before invalid escape chars (M, D, E)
        assert!(fixed.contains(r"App\\Models"));
        assert!(fixed.contains(r"Illuminate\\Database\\Eloquent\\Model"));
        // Should now parse as valid JSON
        assert!(serde_json::from_str::<Value>(&fixed).is_ok(), "Fixed JSON should parse: {}", fixed);
    }

    #[test]
    fn test_escape_invalid_backslashes_preserves_valid_escapes() {
        // Valid JSON escapes should NOT be doubled
        let input = r#"{"content":"line1\nline2\ttab\"quoted\\"}"#;
        let fixed = escape_invalid_backslashes_in_strings(input);
        assert_eq!(input, fixed, "Valid escapes should be unchanged");
    }

    #[test]
    fn test_dispatch_write_file_php_namespaces() {
        // End-to-end: dispatch_native_tool should handle PHP namespaces via fixup chain
        let temp = std::env::temp_dir().join("native_tools_test_php_ns.php");
        let json = format!(
            r#"{{"name":"write_file","arguments":{{"path":"{}","content":"<?php\nnamespace App\Models;\nuse Illuminate\Database\Eloquent\Model;\n\nclass Person extends Model {{\n    protected $fillable = ['name'];\n}}"}}}}"#,
            temp.display()
        );
        let result = dispatch_native_tool(&json, None, None, false, None);
        assert!(result.is_some(), "Should parse PHP namespace JSON via fixup chain");
        let output = result.unwrap().text;
        assert!(output.contains("Written"), "Should write file: {}", output);

        let content = std::fs::read_to_string(&temp).unwrap();
        assert!(content.contains(r"App\Models"), "Should preserve single backslash in file content");
        assert!(content.contains(r"Illuminate\Database\Eloquent\Model"), "Should preserve namespace path");

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_parse_ddg_results_extracts_links_and_snippets() {
        let html = r#"
        <div class="result">
            <a class="result__a" href="https://example.com/page1">Example Page One</a>
            <td class="result__snippet">This is the first result snippet about example.</td>
        </div>
        <div class="result">
            <a class="result__a" href="https://example.com/page2">Example &amp; Page Two</a>
            <td class="result__snippet">Second result with <b>bold</b> text.</td>
        </div>
        "#;

        let result = parse_ddg_results(html, 10);
        assert!(result.contains("Example Page One"), "Should extract first title");
        assert!(result.contains("https://example.com/page1"), "Should extract first URL");
        assert!(result.contains("first result snippet"), "Should extract first snippet");
        assert!(result.contains("Example & Page Two"), "Should decode &amp;");
        assert!(result.contains("https://example.com/page2"), "Should extract second URL");
        assert!(result.contains("Second result with"), "Should extract second snippet");
        assert!(!result.contains("<b>"), "Should strip inner HTML tags from snippets");
    }

    #[test]
    fn test_parse_ddg_results_empty_html() {
        let result = parse_ddg_results("<html><body>no results</body></html>", 10);
        assert!(result.is_empty(), "Should return empty string for no results");
    }

    #[test]
    fn test_dispatch_web_search_missing_query() {
        let json = r#"{"name": "web_search", "arguments": {}}"#;
        let result = dispatch_native_tool(json, None, None, false, None);
        assert!(result.is_some());
        assert!(result.unwrap().text.contains("Error"));
    }

    #[test]
    fn test_dispatch_web_fetch_missing_url() {
        let json = r#"{"name": "web_fetch", "arguments": {}}"#;
        let result = dispatch_native_tool(json, None, None, false, None);
        assert!(result.is_some());
        assert!(result.unwrap().text.contains("Error"));
    }

    #[test]
    fn test_ddg_api_formats_output() {
        // Test that tool_web_search_ddg_api returns formatted output for known queries
        // This is an integration test that calls the real API
        let result = tool_web_search_ddg_api("rust programming language", 5);
        assert!(result.is_some(), "DDG API should return results for 'rust programming language'");
        let text = result.unwrap();
        assert!(text.contains("Rust"), "Should contain 'Rust' in results");
        assert!(text.contains("URL:"), "Should contain URLs");
        assert!(text.contains("Search results for"), "Should have header");
    }
}

