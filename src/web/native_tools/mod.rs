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
use std::sync::OnceLock;
use std::collections::HashMap;
use std::sync::Mutex as StdMutex;

mod file_tools;
mod search_tools;
mod command_tools;

// Re-export public items from submodules
pub use file_tools::{truncate_text_content, read_with_encoding_detection};

/// Extract tool name from a raw command string (JSON tool call).
pub fn extract_tool_name(cmd: &str) -> Option<String> {
    serde_json::from_str::<Value>(cmd).ok()
        .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
}

/// Extract a brief summary of tool arguments for logging.
pub fn extract_tool_args_summary(cmd: &str) -> String {
    let v: Value = match serde_json::from_str(cmd) {
        Ok(v) => v,
        Err(_) => return cmd.chars().take(80).collect(),
    };
    let args = match v.get("arguments") {
        Some(a) => a,
        None => return "(no args)".to_string(),
    };
    // Pick the first string arg as summary
    if let Some(obj) = args.as_object() {
        for (key, val) in obj.iter().take(2) {
            if let Some(s) = val.as_str() {
                let truncated: String = s.chars().take(80).collect();
                return format!("{}={}", key, truncated);
            }
        }
    }
    let s = args.to_string();
    if s.chars().count() > 80 {
        let truncated: String = s.chars().take(77).collect();
        format!("{truncated}...")
    } else {
        s
    }
}

// ─── In-memory todo store (per conversation) ─────────────────────────────────
#[allow(dead_code)]
static TODO_STORE: OnceLock<StdMutex<HashMap<String, String>>> = OnceLock::new();

#[allow(dead_code)]
fn todo_store() -> &'static StdMutex<HashMap<String, String>> {
    TODO_STORE.get_or_init(|| StdMutex::new(HashMap::new()))
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
mod mcp_tools;
mod telegram;

pub use web_fetch::clear_web_fetch_cache;

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

// ─── Tool input schema validation ────────────────────────────────────────────

/// Core tools that should be validated against their schema.
/// Desktop tools and MCP tools are excluded (desktop has complex optional params,
/// MCP tools are validated by their own servers).
const VALIDATED_TOOLS: &[&str] = &[
    "read_file", "write_file", "edit_file", "execute_command", "execute_python",
    "search_files", "find_files", "list_directory", "web_search", "web_fetch",
    "camofox_click", "camofox_screenshot", "camofox_type",
    "browser_navigate", "browser_click", "browser_type", "browser_eval",
    "browser_get_html", "browser_screenshot", "browser_wait", "browser_close",
    "browser_get_text", "browser_get_links", "browser_snapshot",
    "browser_scroll", "browser_press_key",
    "open_browser_view", "close_browser_view",
    "git_status", "git_diff", "git_commit", "open_url", "send_telegram",
    "check_background_process", "lsp_query", "sleep", "todo_write",
    "use_skill", "set_response_style", "insert_text", "undo_edit",
];

/// Validate tool arguments against the tool's schema definition.
/// Returns Ok(()) if valid, Err(message) with a helpful error for the model.
fn validate_tool_args(tool_name: &str, args: &serde_json::Value) -> Result<(), String> {
    // Only validate core tools — skip MCP, desktop, and unknown tools
    if !VALIDATED_TOOLS.contains(&tool_name) {
        return Ok(());
    }

    use crate::web::chat::tool_defs::all_tool_definitions;

    // Find the tool definition
    let all_tools = all_tool_definitions();
    let tool_def = match all_tools.iter().find(|t| {
        t.get("name").and_then(|n| n.as_str()) == Some(tool_name)
    }) {
        Some(t) => t,
        None => return Ok(()), // Not in definitions — skip validation
    };

    let params = match tool_def.get("parameters") {
        Some(p) => p,
        None => return Ok(()),
    };

    // Check required fields
    if let Some(required) = params.get("required").and_then(|r| r.as_array()) {
        for req in required {
            if let Some(field_name) = req.as_str() {
                let value = args.get(field_name);
                match value {
                    None | Some(&serde_json::Value::Null) => {
                        return Err(format!(
                            "Missing required parameter '{}' for tool '{}'. Required parameters: {:?}",
                            field_name, tool_name,
                            required.iter().filter_map(|r| r.as_str()).collect::<Vec<_>>()
                        ));
                    }
                    Some(v) if v.as_str() == Some("") => {
                        return Err(format!(
                            "Required parameter '{}' is empty for tool '{}'. Please provide a value.",
                            field_name, tool_name
                        ));
                    }
                    _ => {} // value present and non-empty
                }
            }
        }
    }

    // Check types for provided parameters (lenient: accept strings for numbers/booleans)
    if let Some(properties) = params.get("properties").and_then(|p| p.as_object()) {
        for (key, schema) in properties {
            if let Some(value) = args.get(key) {
                if value.is_null() { continue; }

                let expected_type = schema.get("type").and_then(|t| t.as_str()).unwrap_or("string");
                let type_ok = match expected_type {
                    "string" => value.is_string(),
                    "integer" | "number" => {
                        value.is_number()
                            || value.as_str().map(|s| s.parse::<f64>().is_ok()).unwrap_or(false)
                    }
                    "boolean" => {
                        value.is_boolean()
                            || value.as_str().map(|s| s == "true" || s == "false").unwrap_or(false)
                    }
                    "array" => value.is_array(),
                    "object" => value.is_object(),
                    _ => true,
                };

                if !type_ok {
                    let actual = if value.is_string() { "string" }
                        else if value.is_number() { "number" }
                        else if value.is_boolean() { "boolean" }
                        else if value.is_array() { "array" }
                        else if value.is_object() { "object" }
                        else { "unknown" };
                    return Err(format!(
                        "Parameter '{}' for tool '{}' should be {} but got {}. Please fix the parameter type.",
                        key, tool_name, expected_type, actual
                    ));
                }
            }
        }
    }

    Ok(())
}

pub fn dispatch_native_tool(
    text: &str,
    web_search_provider: Option<&str>,
    web_search_api_key: Option<&str>,
    use_htmd: bool,
    browser_backend: &super::browser::BrowserBackend,
    mcp_manager: Option<&super::mcp::McpManager>,
    db: Option<&super::database::SharedDatabase>,
) -> Option<NativeToolResult> {
    let trimmed = text.trim();

    // Try all supported tool call formats (JSON, Mistral comma, Llama3 XML, GLM XML, etc.)
    let mut calls = try_parse_all_from_raw(trimmed);
    let (name, args) = if let Some(first) = calls.drain(..).next() {
        first
    } else {
        return None;
    };

    // Validate tool arguments against schema before dispatch
    if let Err(validation_error) = validate_tool_args(&name, &args) {
        return Some(NativeToolResult::text_only(validation_error));
    }

    // Desktop automation tools return NativeToolResult directly (may carry image bytes for vision)
    // Check global abort flag before any desktop action tool (excludes take_screenshot — read-only)
    if name != "take_screenshot"
        && super::desktop_tools::is_desktop_tool(&name)
        && super::desktop_tools::check_desktop_abort()
    {
        return Some(NativeToolResult::text_only(
            "Desktop action aborted by user".to_string(),
        ));
    }
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
    if name == "detect_ui_elements" {
        return Some(super::desktop_tools::yolo_detect::tool_detect_ui_elements(&args));
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

    // Tool catalog (list_tools / get_tool_details)
    if name == "list_tools" {
        let category = args.get("category").and_then(|v| v.as_str()).unwrap_or("desktop");
        let result = if category == "mcp" {
            // MCP tools: query connected servers for their tool lists
            mcp_tools::tool_list_mcp_tools(mcp_manager, db)
        } else {
            super::chat::jinja_templates::get_tool_catalog(category)
        };
        return Some(NativeToolResult::text_only(result));
    }
    if name == "get_tool_details" {
        let tool_name = args.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
        // Check native tools first, then MCP tools
        let result = super::chat::jinja_templates::get_tool_schema(tool_name)
            .or_else(|| mcp_tools::get_mcp_tool_schema(tool_name, mcp_manager))
            .unwrap_or_else(|| format!("Tool '{}' not found. Use list_tools to see available tools.", tool_name));
        return Some(NativeToolResult::text_only(result));
    }

    // Telegram notification
    if name == "send_telegram" {
        return Some(NativeToolResult::text_only(telegram::tool_send_telegram(&args, db)));
    }

    // spawn_agent is handled in command_executor.rs (needs model access).
    // Recognize the name here to prevent falling through to shell execution.
    if name == "spawn_agent" {
        return Some(NativeToolResult::text_only(
            "Error: spawn_agent must be handled by the generation pipeline".to_string()
        ));
    }

    // Camofox interaction tools (return NativeToolResult with optional images)
    if name == "camofox_click" {
        return Some(super::browser::camofox::tool_camofox_click(&args));
    }
    if name == "camofox_screenshot" {
        return Some(super::browser::camofox::tool_camofox_screenshot(&args));
    }
    if name == "camofox_type" {
        return Some(super::browser::camofox::tool_camofox_type(&args));
    }
    if name == "open_browser_view" {
        return Some(super::browser::camofox::tool_open_browser_view(&args));
    }
    if name == "close_browser_view" {
        return Some(super::browser::camofox::tool_close_browser_view(&args));
    }

    // Unified browser_* tools (work the same in web and Tauri)
    if let Some(name) = name.strip_prefix("browser_") {
        return Some(handle_browser_tool(name, &args));
    }

    // web_search may return images (CAPTCHA screenshots) when using Camofox provider
    if name == "web_search" {
        return Some(tool_web_search_with_vision(
            &args,
            web_search_provider,
            web_search_api_key,
            browser_backend,
        ));
    }

    // All other tools return text-only results
    Some(NativeToolResult::text_only(match name.as_str() {
        "read_file" => file_tools::tool_read_file(&args),
        "write_file" => file_tools::tool_write_file(&args),
        "edit_file" => file_tools::tool_edit_file(&args),
        "undo_edit" => file_tools::tool_undo_edit(&args),
        "insert_text" => file_tools::tool_insert_text(&args),
        "search_files" => search_tools::tool_search_files(&args),
        "find_files" => search_tools::tool_find_files(&args),
        "execute_python" => command_tools::tool_execute_python(&args),
        "list_directory" => command_tools::tool_list_directory(&args),
        "web_fetch" => {
            let content = tool_web_fetch(&args, use_htmd, browser_backend);
            if let Some(prompt) = args.get("prompt").and_then(|v| v.as_str()) {
                if !prompt.is_empty() && !content.starts_with("Error") {
                    let max_len = args.get("max_length").and_then(|v| v.as_u64()).unwrap_or(15_000) as usize;
                    web_fetch::apply_prompt_extraction(&content, prompt, max_len)
                } else {
                    content
                }
            } else {
                content
            }
        }
        "execute_command" => {
            let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
            if command.is_empty() {
                return Some(NativeToolResult::text_only("Error: 'command' argument is required".to_string()));
            }
            let is_background = args.get("background").and_then(|v| v.as_bool()).unwrap_or(false);
            if is_background {
                super::background::execute_command_background(command, |_| {})
            } else {
                // Use streaming with timeout (120s wall-clock) instead of blocking .output()
                // to prevent commands like `winget install` from hanging indefinitely
                let timeout = args.get("timeout").and_then(|v| v.as_u64());
                super::command::execute_command_streaming_with_timeout(command, None, timeout, &mut |_| {})
            }
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
            super::background::check_background_process(pid, wait_seconds)
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
        "lsp_query" => {
            let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("definition");
            let symbol = args.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
            let file = args.get("file").and_then(|v| v.as_str());
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");

            if symbol.is_empty() && action != "symbols" && action != "diagnostics" {
                "Error: 'symbol' is required".to_string()
            } else {
                let result = match action {
                    "definition" => {
                        // Try ctags first for real symbol indexing
                        if let Some(ctags) = command_tools::get_ctags(path) {
                            let matches: Vec<&str> = ctags.lines()
                                .filter(|line| {
                                    // ctags JSON: {"name":"symbol",...}
                                    line.contains(&format!("\"name\":\"{}\"", symbol)) ||
                                    // ctags traditional: symbol\tfile\tpattern
                                    line.starts_with(&format!("{}\t", symbol))
                                })
                                .take(10)
                                .collect();
                            if !matches.is_empty() {
                                format!("Definitions found via ctags:\n{}", matches.join("\n"))
                            } else {
                                // Fallback to ripgrep
                                command_tools::lsp_ripgrep_definition(symbol, path)
                            }
                        } else {
                            command_tools::lsp_ripgrep_definition(symbol, path)
                        }
                    }
                    "references" => {
                        // References always use ripgrep (ctags only indexes definitions)
                        let cmd = format!(
                            "rg -n -w \"{}\" \"{}\" --type-add \"code:*.{{rs,ts,tsx,js,jsx,py,go,java,c,cpp,h,hpp,nim,ex}}\" -t code --max-count 30",
                            symbol, path
                        );
                        super::command::execute_command(&cmd)
                    }
                    "symbols" => {
                        let target = file.unwrap_or(path);
                        // Try ctags for real symbol indexing
                        if let Some(ctags) = command_tools::get_ctags(target) {
                            let file_matches: Vec<&str> = ctags.lines()
                                .filter(|line| {
                                    if let Some(f) = file {
                                        line.contains(f)
                                    } else {
                                        true
                                    }
                                })
                                .take(50)
                                .collect();
                            if !file_matches.is_empty() {
                                format!("Symbols:\n{}", file_matches.join("\n"))
                            } else {
                                command_tools::lsp_ripgrep_symbols(target)
                            }
                        } else {
                            command_tools::lsp_ripgrep_symbols(target)
                        }
                    }
                    "diagnostics" => {
                        // Run language-specific diagnostic/type-checking tools
                        let ext = file.and_then(|f| std::path::Path::new(f).extension())
                            .and_then(|e| e.to_str()).unwrap_or("");
                        match ext {
                            "rs" => super::command::execute_command("cargo check --message-format=short 2>&1 | head -30"),
                            "py" => {
                                if let Some(f) = file {
                                    super::command::execute_command(&format!("python -m py_compile {} 2>&1", f))
                                } else {
                                    "Error: 'file' is required for Python diagnostics".to_string()
                                }
                            }
                            "ts" | "tsx" => super::command::execute_command("npx tsc --noEmit 2>&1 | head -30"),
                            "nim" => {
                                if let Some(f) = file {
                                    super::command::execute_command(&format!("nim check {} 2>&1 | head -30", f))
                                } else {
                                    "Error: 'file' is required for Nim diagnostics".to_string()
                                }
                            }
                            _ => "No diagnostic tool available for this file type. Use execute_command to run your build tool.".to_string(),
                        }
                    }
                    "hover" => {
                        let escaped = regex::escape(symbol);
                        let pattern = format!(r"(fn|struct|enum|trait|type|class|def|interface)\s+{}", escaped);
                        let cmd = format!(
                            "rg -n -A 5 \"{}\" \"{}\" --type-add \"code:*.{{rs,ts,tsx,js,jsx,py,go,java,c,cpp,h,hpp}}\" -t code --max-count 5",
                            pattern, path
                        );
                        super::command::execute_command(&cmd)
                    }
                    _ => format!("Unknown action '{}'. Use: definition, references, symbols, hover, diagnostics", action),
                };
                if result.trim().is_empty() {
                    format!("No results found for '{}' ({}) in {}", symbol, action, path)
                } else {
                    result
                }
            }
        }
        "git_status" => command_tools::tool_git_status(&args),
        "git_diff" => command_tools::tool_git_diff(&args),
        "git_commit" => command_tools::tool_git_commit(&args),
        // MCP server management tools
        "list_mcp_servers" => mcp_tools::tool_list_mcp_servers(mcp_manager, db),
        "add_mcp_server" => mcp_tools::tool_add_mcp_server(&args, mcp_manager, db),
        "remove_mcp_server" => mcp_tools::tool_remove_mcp_server(&args, mcp_manager, db),
        "list_background_processes" => command_tools::tool_list_background_processes(),
        "sleep" => {
            let seconds = args.get("seconds").and_then(|v| v.as_u64()).unwrap_or(5).min(30);
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            format!("Waited {} seconds", seconds)
        }
        "todo_write" => {
            let todos = args.get("todos").and_then(|v| v.as_str()).unwrap_or("[]");
            match serde_json::from_str::<serde_json::Value>(todos) {
                Ok(val) => {
                    let formatted = serde_json::to_string_pretty(&val).unwrap_or_else(|_| todos.to_string());
                    if let Ok(mut store) = todo_store().lock() {
                        store.insert("default".to_string(), formatted.clone());
                    }
                    format!("Todo list updated:\n{}", formatted)
                }
                Err(e) => format!("Error: Invalid JSON for todos: {e}. Expected array of {{id, task, status}} objects.")
            }
        }
        "todo_read" => {
            let todos = todo_store().lock().ok()
                .and_then(|store| store.get("default").cloned())
                .unwrap_or_else(|| "[]".to_string());
            if todos == "[]" {
                "No todos yet. Use todo_write to create a task checklist.".to_string()
            } else {
                format!("Current todos:\n{}", todos)
            }
        }
        "list_skills" => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let skills = super::skills::discover_skills(&cwd);
            if skills.is_empty() {
                "No skills found. Create .md files in a 'skills/' directory with YAML frontmatter (name, description).".to_string()
            } else {
                let mut output = format!("{} skills available:\n", skills.len());
                for s in &skills {
                    output.push_str(&format!("  {} — {}\n", s.name, s.description));
                }
                output
            }
        }
        "use_skill" => {
            let skill_name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let template_args = args.get("args").and_then(|v| v.as_str()).unwrap_or("{}");

            let cwd = std::env::current_dir().unwrap_or_default();
            match super::skills::get_skill(&cwd, skill_name) {
                Some(skill) => {
                    // Simple template substitution: replace {{key}} with values
                    let mut content = skill.content.clone();
                    if let Ok(args_map) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(template_args) {
                        for (key, val) in &args_map {
                            let placeholder = format!("{{{{{}}}}}", key);
                            let replacement = match val.as_str() {
                                Some(s) => s.to_string(),
                                None => val.to_string(),
                            };
                            content = content.replace(&placeholder, &replacement);
                        }
                    }
                    format!("Skill '{}' loaded:\n\n{}", skill_name, content)
                }
                None => format!("Skill '{}' not found. Use list_skills to see available skills.", skill_name),
            }
        }
        "set_response_style" => {
            let style = args.get("style").and_then(|v| v.as_str()).unwrap_or("detailed");
            match style {
                "brief" => "Response style set to BRIEF. From now on: be concise, skip explanations, show only results and actions. No preamble or summaries.".to_string(),
                "detailed" | _ => "Response style set to DETAILED. From now on: explain your reasoning, show context, and provide thorough responses.".to_string(),
            }
        }
        "open_url" => {
            let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("");
            if url.is_empty() {
                "Error: 'url' argument is required".to_string()
            } else if !url.starts_with("http://") && !url.starts_with("https://") {
                format!("Error: URL must start with http:// or https://, got: {}", url)
            } else {
                // Open URL in default browser (cross-platform)
                #[cfg(target_os = "windows")]
                let result = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
                #[cfg(target_os = "macos")]
                let result = std::process::Command::new("open").arg(url).spawn();
                #[cfg(target_os = "linux")]
                let result = std::process::Command::new("xdg-open").arg(url).spawn();
                match result {
                    Ok(_) => format!("Opened {} in the default browser", url),
                    Err(e) => format!("Failed to open URL: {}", e),
                }
            }
        }
        _ => {
            // Check if it's an MCP tool before falling back to shell
            // Lazy connect MCP servers on first tool call
            mcp_tools::ensure_mcp_connected(mcp_manager, db);
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

// Telegram tool: see telegram.rs
// MCP tools: see mcp_tools.rs

/// web_search with vision support — returns screenshot if Camofox detects a CAPTCHA.
fn tool_web_search_with_vision(
    args: &Value,
    web_search_provider: Option<&str>,
    web_search_api_key: Option<&str>,
    browser_backend: &super::browser::BrowserBackend,
) -> NativeToolResult {
    // For non-Camofox providers, return text-only as before
    if web_search_provider != Some("Camofox") {
        return NativeToolResult::text_only(
            tool_web_search(args, web_search_provider, web_search_api_key, browser_backend),
        );
    }

    // Camofox provider — may return a CAPTCHA screenshot
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => {
            return NativeToolResult::text_only(
                "Error: 'query' argument is required".to_string(),
            );
        }
    };

    let max_results = args
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as usize;

    eprintln!("[WEB_SEARCH] Using Camofox (anti-detection Google)");
    let result = super::browser::camofox::search(query, max_results);

    if result.captcha_detected {
        if let Some(screenshot) = result.screenshot {
            NativeToolResult::with_image(result.text, screenshot)
        } else {
            NativeToolResult::text_only(result.text)
        }
    } else {
        NativeToolResult::text_only(result.text)
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
        // Resize + JPEG-compress for vision models (saves tokens)
        let optimized = crate::web::desktop_tools::optimize_screenshot_for_vision(&png_bytes);
        NativeToolResult::with_image(text, optimized)
    }
}

/// Dispatcher for the unified `browser_*` tool family.
/// Operates on the active browser session (Camofox-backed for now,
/// Tauri WebView later). Tool name comes in stripped of the prefix.
fn handle_browser_tool(name: &str, args: &Value) -> NativeToolResult {
    use super::browser::session::{
        current_session, notify_tauri_browser_close, open_session, BrowserSession,
    };

    // navigate: reuse existing session if any, else open a fresh one
    if name == "navigate" {
        let url = match args.get("url").and_then(|v| v.as_str()) {
            Some(u) if !u.trim().is_empty() => u,
            _ => return NativeToolResult::text_only("Error: 'url' is required".to_string()),
        };
        let full_url = if url.starts_with("http://") || url.starts_with("https://") {
            url.to_string()
        } else {
            format!("https://{url}")
        };
        eprintln!("[BROWSER_TOOL] navigate: {full_url}");
        return match current_session() {
            Ok(mut s) => {
                eprintln!("[BROWSER_TOOL] existing session, calling navigate...");
                match s.navigate(&full_url) {
                    Ok(()) => {
                        eprintln!("[BROWSER_TOOL] navigate OK");
                        NativeToolResult::text_only(format!("Navigated to {full_url}."))
                    }
                    Err(e) => {
                        eprintln!("[BROWSER_TOOL] navigate failed: {e}, opening new session...");
                        match open_session(&full_url) {
                            Ok(s2) => NativeToolResult::text_only(format!("Opened new session at {}.", s2.url())),
                            Err(e) => NativeToolResult::text_only(format!("navigate failed: {e}")),
                        }
                    }
                }
            }
            Err(_) => {
                eprintln!("[BROWSER_TOOL] no existing session, calling open_session...");
                match open_session(&full_url) {
                    Ok(s) => {
                        eprintln!("[BROWSER_TOOL] open_session OK");
                        NativeToolResult::text_only(format!(
                            "Navigated to {}.",
                            s.url()
                        ))
                    }
                    Err(e) => {
                        eprintln!("[BROWSER_TOOL] open_session FAILED: {e}");
                        NativeToolResult::text_only(format!("navigate failed: {e}"))
                    }
                }
            }
        };
    }

    // All other tools require an active session
    let session = match current_session() {
        Ok(s) => s,
        Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
    };

    match name {
        "click" => {
            let sel = args.get("selector").and_then(|v| v.as_str()).unwrap_or("");
            if sel.is_empty() {
                return NativeToolResult::text_only("Error: 'selector' is required".into());
            }
            match session.click(sel) {
                Ok(()) => {
                    // Click may navigate — clear caches so next get_text reads fresh
                    super::browser::session::clear_cache();
                    NativeToolResult::text_only(format!("Clicked '{sel}'"))
                }
                Err(e) => NativeToolResult::text_only(format!("click failed: {e}")),
            }
        }
        "type" => {
            let sel = args.get("selector").and_then(|v| v.as_str()).unwrap_or("");
            let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let press_enter = args.get("press_enter").and_then(|v| v.as_bool()).unwrap_or(false);
            if sel.is_empty() || text.is_empty() {
                return NativeToolResult::text_only(
                    "Error: 'selector' and 'text' are required".into(),
                );
            }
            match session.type_text(sel, text, press_enter) {
                Ok(()) => NativeToolResult::text_only(format!(
                    "Typed into '{sel}'{}",
                    if press_enter { " and pressed Enter" } else { "" }
                )),
                Err(e) => NativeToolResult::text_only(format!("type failed: {e}")),
            }
        }
        "eval" => {
            let js = args.get("js").and_then(|v| v.as_str()).unwrap_or("");
            if js.is_empty() {
                return NativeToolResult::text_only("Error: 'js' is required".into());
            }
            match session.eval(js) {
                Ok(Value::String(s)) => NativeToolResult::text_only(s),
                Ok(v) => NativeToolResult::text_only(v.to_string()),
                Err(e) => NativeToolResult::text_only(format!("eval failed: {e}")),
            }
        }
        "get_html" => match session.html() {
            Ok(html) => {
                const MAX: usize = 50_000;
                let mut s = html;
                if s.len() > MAX {
                    let mut end = MAX;
                    while end > 0 && !s.is_char_boundary(end) {
                        end -= 1;
                    }
                    s.truncate(end);
                    s.push_str("\n... [truncated]");
                }
                NativeToolResult::text_only(s)
            }
            Err(e) => NativeToolResult::text_only(format!("get_html failed: {e}")),
        },
        "screenshot" => {
            // Try real screenshot first (Camofox), then return page text (HTTP mode)
            if let Ok(bytes) = session.screenshot() {
                NativeToolResult::with_image("Screenshot captured.".into(), bytes)
            } else {
                let text = session.url().to_string();
                NativeToolResult::text_only(format!(
                    "Screenshot not available in HTTP mode. Use browser_get_text or browser_get_links to read the page at {}",
                    text
                ))
            }
        },
        "wait" => {
            let sel = args.get("selector").and_then(|v| v.as_str()).unwrap_or("");
            let timeout = args
                .get("timeout_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(5000);
            if sel.is_empty() {
                return NativeToolResult::text_only("Error: 'selector' is required".into());
            }
            match session.wait_for(sel, timeout) {
                Ok(true) => NativeToolResult::text_only(format!("Element '{sel}' appeared")),
                Ok(false) => {
                    NativeToolResult::text_only(format!("Timeout waiting for '{sel}'"))
                }
                Err(e) => NativeToolResult::text_only(format!("wait failed: {e}")),
            }
        }
        "get_text" => match session.snapshot() {
            Ok(text) => {
                const MAX: usize = 30_000;
                let mut s = text;
                if s.len() > MAX {
                    let mut end = MAX;
                    while end > 0 && !s.is_char_boundary(end) { end -= 1; }
                    s.truncate(end);
                    s.push_str("\n... [truncated]");
                }
                NativeToolResult::text_only(s)
            }
            Err(e) => NativeToolResult::text_only(format!("get_text failed: {e}")),
        },
        "get_links" => match session.html() {
            Ok(html) => {
                // Extract links from HTML using simple regex
                let mut links = Vec::new();
                for cap in regex::Regex::new(r#"<a[^>]+href="([^"]*)"[^>]*>(.*?)</a>"#)
                    .unwrap()
                    .captures_iter(&html)
                {
                    if links.len() >= 200 { break; }
                    let href = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                    let text = cap.get(2).map(|m| m.as_str()).unwrap_or("");
                    // Strip HTML tags from link text
                    let clean = regex::Regex::new(r"<[^>]+>").unwrap()
                        .replace_all(text, "").trim().chars().take(80).collect::<String>();
                    if !href.is_empty() && !clean.is_empty() {
                        links.push(serde_json::json!({"text": clean, "href": href}));
                    }
                }
                NativeToolResult::text_only(serde_json::to_string(&links).unwrap_or("[]".into()))
            }
            Err(e) => NativeToolResult::text_only(format!("get_links failed: {e}")),
        }
        "snapshot" => match session.snapshot() {
            Ok(s) => {
                const MAX: usize = 20_000;
                let mut text = s;
                if text.len() > MAX {
                    let mut end = MAX;
                    while end > 0 && !text.is_char_boundary(end) { end -= 1; }
                    text.truncate(end);
                    text.push_str("\n... [truncated]");
                }
                NativeToolResult::text_only(text)
            }
            Err(e) => NativeToolResult::text_only(format!("snapshot failed: {e}")),
        },
        "scroll" => {
            let sel = args.get("selector").and_then(|v| v.as_str()).unwrap_or("");
            let amount = args.get("amount").and_then(|v| v.as_i64()).unwrap_or(0);
            // Try eval first (Camofox), fall back to no-op (HTTP mode)
            let js = if !sel.is_empty() {
                format!(
                    "(() => {{ const el = document.querySelector({sel_lit}); if (el) {{ el.scrollIntoView({{behavior:'smooth', block:'center'}}); return 'scrolled to '+{sel_lit}; }} return 'element not found'; }})()",
                    sel_lit = serde_json::to_string(sel).unwrap_or_else(|_| "''".into())
                )
            } else {
                format!("(() => {{ window.scrollBy(0, {amount}); return 'scrolled '+{amount}+' px'; }})()")
            };
            match session.eval(&js) {
                Ok(v) => {
                    let msg = v.get("result").and_then(|r| r.as_str()).unwrap_or("done");
                    NativeToolResult::text_only(msg.to_string())
                }
                Err(_) => NativeToolResult::text_only("Scroll not available in HTTP mode (content is fetched statically).".into()),
            }
        }
        "press_key" => {
            let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("");
            if key.is_empty() {
                return NativeToolResult::text_only("Error: 'key' is required".into());
            }
            match session.press_key(key) {
                Ok(()) => NativeToolResult::text_only(format!("Pressed '{key}'")),
                Err(e) => NativeToolResult::text_only(format!("press_key failed: {e}")),
            }
        }
        "close" => {
            let mut s = session;
            let _ = notify_tauri_browser_close();
            match s.close() {
                Ok(()) => NativeToolResult::text_only("Browser session closed.".into()),
                Err(e) => NativeToolResult::text_only(format!("close failed: {e}")),
            }
        }
        other => NativeToolResult::text_only(format!("Unknown browser tool: browser_{other}")),
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
        let result = dispatch_native_tool(&json, None, None, false, &crate::web::browser::BrowserBackend::Chrome, None, None);
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
        let result = dispatch_native_tool(&json, None, None, false, &crate::web::browser::BrowserBackend::Chrome, None, None);
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
        let result = dispatch_native_tool(&json, None, None, false, &crate::web::browser::BrowserBackend::Chrome, None, None);
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
        let result = dispatch_native_tool(json, None, None, false, &crate::web::browser::BrowserBackend::Chrome, None, None);
        assert!(result.is_some());
        assert!(result.unwrap().text.contains("Directory listing"));
    }

    #[test]
    fn test_dispatch_unknown_tool_returns_none() {
        let json = r#"{"name": "unknown_tool", "arguments": {}}"#;
        let result = dispatch_native_tool(json, None, None, false, &crate::web::browser::BrowserBackend::Chrome, None, None);
        assert!(result.is_none());
    }

    #[test]
    fn test_dispatch_non_json_returns_none() {
        let result = dispatch_native_tool("ls -la", None, None, false, &crate::web::browser::BrowserBackend::Chrome, None, None);
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
        let result = dispatch_native_tool(&json, None, None, false, &crate::web::browser::BrowserBackend::Chrome, None, None);
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
        let result = dispatch_native_tool(&input, None, None, false, &crate::web::browser::BrowserBackend::Chrome, None, None);
        assert!(result.is_some(), "Should parse Mistral comma format");
        assert!(result.unwrap().text.contains("comma format test"));

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_mistral_comma_execute_command() {
        // Devstral: execute_command,{"command": "echo hello"}
        let input = r#"execute_command,{"command": "echo hello"}"#;
        let result = dispatch_native_tool(input, None, None, false, &crate::web::browser::BrowserBackend::Chrome, None, None);
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
        let result = dispatch_native_tool(&input, None, None, false, &crate::web::browser::BrowserBackend::Chrome, None, None);
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
        let result = dispatch_native_tool(&input, None, None, false, &crate::web::browser::BrowserBackend::Chrome, None, None);
        assert!(result.is_some(), "Should parse Llama3 XML write_file");
        assert!(result.unwrap().text.contains("Written"));
        assert_eq!(std::fs::read_to_string(&temp).unwrap(), "hello world");

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_name_json_format() {
        // Granite outputs: list_directory{"path": "."}
        let input = r#"list_directory{"path": "."}"#;
        let result = dispatch_native_tool(input, None, None, false, &crate::web::browser::BrowserBackend::Chrome, None, None);
        assert!(result.is_some(), "Should parse name+JSON format");
        assert!(result.unwrap().text.contains("Directory listing"));
    }

    #[test]
    fn test_execute_python_simple() {
        let json = r#"{"name": "execute_python", "arguments": {"code": "print('hello from python')"}}"#;
        let result = dispatch_native_tool(json, None, None, false, &crate::web::browser::BrowserBackend::Chrome, None, None);
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
        let result = command_tools::tool_execute_python(&args);
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
        let result = dispatch_native_tool(json, None, None, false, &crate::web::browser::BrowserBackend::Chrome, None, None);
        assert!(result.is_some(), "Valid JSON should work: {:?}", result);
        let _ = std::fs::remove_file("/tmp/test-autoclose.txt");

        // Now test with missing outer brace (GLM pattern)
        let broken = "{\"name\": \"write_file\", \"arguments\": {\"path\": \"/tmp/test-autoclose2.txt\", \"content\": \"{\n  \\\"name\\\": \\\"test\\\",\n  \\\"version\\\": \\\"1.0.0\\\"\n}\"}}";
        // Remove last }
        let broken = &broken[..broken.len() - 1];
        let result = dispatch_native_tool(broken, None, None, false, &crate::web::browser::BrowserBackend::Chrome, None, None);
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
        let result = dispatch_native_tool(&json, None, None, false, &crate::web::browser::BrowserBackend::Chrome, None, None);
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
        let result = dispatch_native_tool(json, None, None, false, &crate::web::browser::BrowserBackend::Chrome, None, None);
        assert!(result.is_some());
        assert!(result.unwrap().text.contains("Error"));
    }

    #[test]
    fn test_dispatch_web_fetch_missing_url() {
        let json = r#"{"name": "web_fetch", "arguments": {}}"#;
        let result = dispatch_native_tool(json, None, None, false, &crate::web::browser::BrowserBackend::Chrome, None, None);
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
