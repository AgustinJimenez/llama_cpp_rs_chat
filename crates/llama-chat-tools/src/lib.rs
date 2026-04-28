//! Native tool implementations for LLM agent tool calls.
//!
//! Provides safe, shell-free implementations of common operations that LLM agents
//! need: reading/writing files, running Python code, listing directories, browser
//! interaction, MCP server management, and tool call parsing for multiple formats.

// Re-export NativeToolResult from shared types crate
pub use llama_chat_types::NativeToolResult;

use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use std::sync::OnceLock;

pub mod file_tools;
pub mod search_tools;
pub mod command_tools;
pub mod parsing;
pub mod doc_extractors;
pub mod browser_tools;
pub mod browser_session;
pub mod mcp_tools;
pub mod screenshot_tool;
pub mod telegram;
pub mod tool_parser;
pub mod tool_defs;
mod utils;

// Re-export public items from submodules
pub use file_tools::{truncate_text_content, read_with_encoding_detection};
pub use parsing::*;
pub use doc_extractors::*;
#[allow(unused_imports)]
pub use screenshot_tool::tool_take_screenshot_with_image;
pub use tool_parser::{FORMAT_PRIORITY, FormatDetector, extract_balanced_json, build_model_exec_regex, EXEC_PATTERN};
pub use tool_defs::all_tool_definitions;

// ─── MCP Manager trait ──────────────────────────────────────────────────────

/// Trait for MCP manager operations needed by the tools crate.
/// The root crate implements this for its concrete `McpManager` type.
pub trait McpManagerOps: Send + Sync {
    /// Check if a tool name belongs to an MCP server.
    fn is_mcp_tool(&self, name: &str) -> bool;
    /// Call an MCP tool by qualified name with arguments.
    fn call_tool(&self, qualified_name: &str, args: Value) -> Result<String, String>;
    /// Get server statuses for display.
    fn get_server_statuses(&self) -> Vec<llama_chat_types::McpServerStatus>;
    /// Get tool definitions from all connected servers.
    fn get_tool_definitions(&self) -> Vec<McpToolDefInfo>;
    /// Get names of connected servers.
    fn get_connected_server_names(&self) -> Vec<String>;
    /// Refresh connections using the given database.
    fn refresh_connections(&self, db: &llama_chat_db::SharedDatabase) -> Result<(), String>;
}

/// Minimal tool definition info for MCP tools (avoids depending on the full McpToolDef type).
#[derive(Debug, Clone)]
pub struct McpToolDefInfo {
    pub qualified_name: String,
    pub description: String,
    pub input_schema: Value,
    pub server_name: String,
}

impl McpToolDefInfo {
    /// Convert to OpenAI function-calling format.
    pub fn to_openai_function(&self) -> Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.qualified_name,
                "description": format!("[MCP:{}] {}", self.server_name, self.description),
                "parameters": self.input_schema,
            }
        })
    }
}

// ─── Dispatch context ───────────────────────────────────────────────────────

/// External functions that the tools crate needs from the root crate.
/// These are passed as callbacks to avoid circular dependencies.
pub struct DispatchContext<'a> {
    /// Get tool catalog by category (e.g. "desktop", "file", etc.)
    pub get_tool_catalog: Option<&'a dyn Fn(&str) -> String>,
    /// Get tool schema by name
    pub get_tool_schema: Option<&'a dyn Fn(&str) -> Option<String>>,
    /// Discover skills in a directory
    pub discover_skills: Option<&'a dyn Fn(&std::path::Path) -> Vec<SkillInfo>>,
    /// Get a skill by name
    pub get_skill: Option<&'a dyn Fn(&std::path::Path, &str) -> Option<SkillInfo>>,
}

/// Minimal skill info for the tools crate.
#[derive(Debug, Clone)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub content: String,
}

// ─── Extract helpers ────────────────────────────────────────────────────────

/// Extract tool name from a raw command string (JSON tool call).
pub fn extract_tool_name(cmd: &str) -> Option<String> {
    serde_json::from_str::<Value>(cmd)
        .ok()
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
static TODO_STORE: OnceLock<StdMutex<HashMap<String, String>>> = OnceLock::new();

fn todo_store() -> &'static StdMutex<HashMap<String, String>> {
    TODO_STORE.get_or_init(|| StdMutex::new(HashMap::new()))
}

/// If the text is an `execute_command` tool call, extract the command string and background flag.
/// Returns `(command, is_background)`.
/// Used by the command executor to route `execute_command` through streaming or background path.
pub fn extract_execute_command_with_opts(text: &str) -> Option<(String, bool)> {
    // First try the standard tool call format: {"name":"execute_command","arguments":{"command":"...","background":true}}
    if let Some((name, args)) = try_parse_tool_call(text) {
        if name == "execute_command" {
            let command = args.get("command").and_then(|v| v.as_str())?;
            if !command.is_empty() {
                let background = args
                    .get("background")
                    .and_then(parsing::value_as_bool_flexible)
                    .unwrap_or(false);
                return Some((command.to_string(), background));
            }
        }
        return None;
    }

    // Fallback: some models (GLM) put bare arguments without the name/arguments wrapper,
    // e.g. {"command": "...", "background": true} inside SYSTEM.EXEC tags
    let trimmed = text.trim();
    if let Some(parsed) = parsing::try_parse_with_fixups(trimmed) {
        if let Some(command) = parsed.get("command").and_then(|v| v.as_str()) {
            if !command.is_empty() {
                let background = parsed
                    .get("background")
                    .and_then(parsing::value_as_bool_flexible)
                    .unwrap_or(false);
                return Some((command.to_string(), background));
            }
        }
    }
    None
}

// ─── Tool input schema validation ────────────────────────────────────────────

/// Core tools that should be validated against their schema.
const VALIDATED_TOOLS: &[&str] = &[
    "read_file",
    "write_file",
    "edit_file",
    "execute_command",
    "execute_python",
    "search_files",
    "find_files",
    "list_directory",
    "browser_navigate",
    "browser_search",
    "browser_click",
    "browser_type",
    "browser_query",
    "browser_eval",
    "browser_get_html",
    "browser_screenshot",
    "browser_wait",
    "browser_close",
    "browser_get_text",
    "browser_get_links",
    "browser_snapshot",
    "browser_scroll",
    "browser_press_key",
    "open_browser_view",
    "close_browser_view",
    "git_status",
    "git_diff",
    "git_commit",
    "open_url",
    "send_telegram",
    "check_background_process",
    "lsp_query",
    "sleep",
    "todo_write",
    "use_skill",
    "set_response_style",
    "insert_text",
    "undo_edit",
];

/// Validate tool arguments against the tool's schema definition.
/// Returns Ok(()) if valid, Err(message) with a helpful error for the model.
fn validate_tool_args(tool_name: &str, args: &serde_json::Value) -> Result<(), String> {
    // Only validate core tools — skip MCP, desktop, and unknown tools
    if !VALIDATED_TOOLS.contains(&tool_name) {
        return Ok(());
    }

    // Find the tool definition
    let all_tools = tool_defs::all_tool_definitions();
    let tool_def = match all_tools
        .iter()
        .find(|t| t.get("name").and_then(|n| n.as_str()) == Some(tool_name))
    {
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
                            field_name,
                            tool_name,
                            required
                                .iter()
                                .filter_map(|r| r.as_str())
                                .collect::<Vec<_>>()
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
                if value.is_null() {
                    continue;
                }

                let expected_type = schema
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("string");
                let type_ok = match expected_type {
                    "string" => value.is_string(),
                    "integer" | "number" => {
                        value.is_number()
                            || value
                                .as_str()
                                .map(|s| s.parse::<f64>().is_ok())
                                .unwrap_or(false)
                    }
                    "boolean" => {
                        value.is_boolean()
                            || value
                                .as_str()
                                .map(|s| s == "true" || s == "false")
                                .unwrap_or(false)
                    }
                    "array" => value.is_array(),
                    "object" => value.is_object(),
                    _ => true,
                };

                if !type_ok {
                    let actual = if value.is_string() {
                        "string"
                    } else if value.is_number() {
                        "number"
                    } else if value.is_boolean() {
                        "boolean"
                    } else if value.is_array() {
                        "array"
                    } else if value.is_object() {
                        "object"
                    } else {
                        "unknown"
                    };
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
pub fn dispatch_native_tool(
    text: &str,
    _use_htmd: bool,
    mcp_manager: Option<&dyn McpManagerOps>,
    db: Option<&llama_chat_db::SharedDatabase>,
    ctx: &DispatchContext<'_>,
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
        && llama_chat_desktop_tools::is_desktop_tool(&name)
        && llama_chat_desktop_tools::check_desktop_abort()
    {
        return Some(NativeToolResult::text_only(
            "Desktop action aborted by user".to_string(),
        ));
    }
    if name == "take_screenshot" {
        return Some(screenshot_tool::tool_take_screenshot_with_image(&args));
    }
    if name == "click_screen" {
        return Some(llama_chat_desktop_tools::tool_click_screen(&args));
    }
    if name == "type_text" {
        return Some(llama_chat_desktop_tools::tool_type_text(&args));
    }
    if name == "press_key" {
        return Some(llama_chat_desktop_tools::tool_press_key(&args));
    }
    if name == "move_mouse" {
        return Some(llama_chat_desktop_tools::tool_move_mouse(&args));
    }
    if name == "scroll_screen" {
        return Some(llama_chat_desktop_tools::tool_scroll_screen(&args));
    }
    if name == "list_windows" {
        return Some(llama_chat_desktop_tools::tool_list_windows(&args));
    }
    if name == "mouse_drag" {
        return Some(llama_chat_desktop_tools::tool_mouse_drag(&args));
    }
    if name == "get_cursor_position" {
        return Some(llama_chat_desktop_tools::tool_get_cursor_position(&args));
    }
    if name == "focus_window" {
        return Some(llama_chat_desktop_tools::tool_focus_window(&args));
    }
    if name == "minimize_window" {
        return Some(llama_chat_desktop_tools::tool_minimize_window(&args));
    }
    if name == "maximize_window" {
        return Some(llama_chat_desktop_tools::tool_maximize_window(&args));
    }
    if name == "close_window" {
        return Some(llama_chat_desktop_tools::tool_close_window(&args));
    }
    if name == "read_clipboard" {
        return Some(llama_chat_desktop_tools::tool_read_clipboard(&args));
    }
    if name == "write_clipboard" {
        return Some(llama_chat_desktop_tools::tool_write_clipboard(&args));
    }
    if name == "resize_window" {
        return Some(llama_chat_desktop_tools::tool_resize_window(&args));
    }
    if name == "get_active_window" {
        return Some(llama_chat_desktop_tools::tool_get_active_window(&args));
    }
    if name == "wait_for_window" {
        return Some(llama_chat_desktop_tools::tool_wait_for_window(&args));
    }
    if name == "get_pixel_color" {
        return Some(llama_chat_desktop_tools::tool_get_pixel_color(&args));
    }
    if name == "click_window_relative" {
        return Some(llama_chat_desktop_tools::tool_click_window_relative(&args));
    }
    if name == "list_monitors" {
        return Some(llama_chat_desktop_tools::tool_list_monitors(&args));
    }
    if name == "screenshot_region" {
        return Some(llama_chat_desktop_tools::tool_screenshot_region(&args));
    }
    if name == "screenshot_diff" {
        return Some(llama_chat_desktop_tools::tool_screenshot_diff(&args));
    }
    if name == "ocr_screen" {
        return Some(llama_chat_desktop_tools::tool_ocr_screen(&args));
    }
    if name == "get_ui_tree" {
        return Some(llama_chat_desktop_tools::tool_get_ui_tree(&args));
    }
    if name == "detect_ui_elements" {
        return Some(llama_chat_desktop_tools::yolo_detect::tool_detect_ui_elements(&args));
    }
    if name == "ocr_find_text" {
        return Some(llama_chat_desktop_tools::tool_ocr_find_text(&args));
    }
    if name == "click_ui_element" {
        return Some(llama_chat_desktop_tools::tool_click_ui_element(&args));
    }
    if name == "window_screenshot" {
        return Some(llama_chat_desktop_tools::tool_window_screenshot(&args));
    }
    if name == "open_application" {
        return Some(llama_chat_desktop_tools::tool_open_application(&args));
    }
    if name == "wait_for_screen_change" {
        return Some(llama_chat_desktop_tools::tool_wait_for_screen_change(&args));
    }
    if name == "set_window_topmost" {
        return Some(llama_chat_desktop_tools::tool_set_window_topmost(&args));
    }
    if name == "invoke_ui_action" {
        return Some(llama_chat_desktop_tools::tool_invoke_ui_action(&args));
    }
    if name == "read_ui_element_value" {
        return Some(llama_chat_desktop_tools::tool_read_ui_element_value(&args));
    }
    if name == "wait_for_ui_element" {
        return Some(llama_chat_desktop_tools::tool_wait_for_ui_element(&args));
    }
    if name == "clipboard_image" {
        return Some(llama_chat_desktop_tools::tool_clipboard_image(&args));
    }
    if name == "find_ui_elements" {
        return Some(llama_chat_desktop_tools::tool_find_ui_elements(&args));
    }
    if name == "execute_app_script" {
        return Some(llama_chat_desktop_tools::tool_execute_app_script(&args));
    }
    if name == "send_keys_to_window" {
        return Some(llama_chat_desktop_tools::tool_send_keys_to_window(&args));
    }
    if name == "snap_window" {
        return Some(llama_chat_desktop_tools::tool_snap_window(&args));
    }
    if name == "list_processes" {
        return Some(llama_chat_desktop_tools::tool_list_processes(&args));
    }
    if name == "kill_process" {
        return Some(llama_chat_desktop_tools::tool_kill_process(&args));
    }

    // Compound desktop tools
    if name == "find_and_click_text" {
        return Some(llama_chat_desktop_tools::tool_find_and_click_text(&args));
    }
    if name == "type_into_element" {
        return Some(llama_chat_desktop_tools::tool_type_into_element(&args));
    }
    if name == "get_window_text" {
        return Some(llama_chat_desktop_tools::tool_get_window_text(&args));
    }
    if name == "file_dialog_navigate" {
        return Some(llama_chat_desktop_tools::tool_file_dialog_navigate(&args));
    }
    if name == "drag_and_drop_element" {
        return Some(llama_chat_desktop_tools::tool_drag_and_drop_element(&args));
    }
    if name == "wait_for_text_on_screen" {
        return Some(llama_chat_desktop_tools::tool_wait_for_text_on_screen(&args));
    }
    if name == "get_context_menu" {
        return Some(llama_chat_desktop_tools::tool_get_context_menu(&args));
    }
    if name == "scroll_element" {
        return Some(llama_chat_desktop_tools::tool_scroll_element(&args));
    }
    if name == "mouse_button" {
        return Some(llama_chat_desktop_tools::tool_mouse_button(&args));
    }
    if name == "switch_virtual_desktop" {
        return Some(llama_chat_desktop_tools::tool_switch_virtual_desktop(&args));
    }
    if name == "find_image_on_screen" {
        return Some(llama_chat_desktop_tools::tool_find_image_on_screen(&args));
    }
    if name == "get_process_info" {
        return Some(llama_chat_desktop_tools::tool_get_process_info(&args));
    }
    if name == "paste" {
        return Some(llama_chat_desktop_tools::tool_paste(&args));
    }
    if name == "clear_field" {
        return Some(llama_chat_desktop_tools::tool_clear_field(&args));
    }
    if name == "hover_element" {
        return Some(llama_chat_desktop_tools::tool_hover_element(&args));
    }
    if name == "handle_dialog" {
        return Some(llama_chat_desktop_tools::tool_handle_dialog(&args));
    }
    if name == "wait_for_element_state" {
        return Some(llama_chat_desktop_tools::tool_wait_for_element_state(&args));
    }
    if name == "fill_form" {
        return Some(llama_chat_desktop_tools::tool_fill_form(&args));
    }
    if name == "run_action_sequence" {
        return Some(llama_chat_desktop_tools::tool_run_action_sequence(&args));
    }
    if name == "move_to_monitor" {
        return Some(llama_chat_desktop_tools::tool_move_to_monitor(&args));
    }
    if name == "set_window_opacity" {
        return Some(llama_chat_desktop_tools::tool_set_window_opacity(&args));
    }
    if name == "highlight_point" {
        return Some(llama_chat_desktop_tools::tool_highlight_point(&args));
    }
    if name == "annotate_screenshot" {
        return Some(llama_chat_desktop_tools::tool_annotate_screenshot(&args));
    }
    if name == "ocr_region" {
        return Some(llama_chat_desktop_tools::tool_ocr_region(&args));
    }
    if name == "find_color_on_screen" {
        return Some(llama_chat_desktop_tools::tool_find_color_on_screen(&args));
    }
    if name == "read_registry" {
        return Some(llama_chat_desktop_tools::tool_read_registry(&args));
    }
    if name == "click_tray_icon" {
        return Some(llama_chat_desktop_tools::tool_click_tray_icon(&args));
    }
    if name == "watch_window" {
        return Some(llama_chat_desktop_tools::tool_watch_window(&args));
    }

    // Tool catalog (list_tools / get_tool_details)
    if name == "list_tools" {
        let category = args
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("desktop");
        let result = if category == "mcp" {
            mcp_tools::tool_list_mcp_tools(mcp_manager, db)
        } else if let Some(get_catalog) = ctx.get_tool_catalog {
            get_catalog(category)
        } else {
            "Tool catalog not available".to_string()
        };
        return Some(NativeToolResult::text_only(result));
    }
    if name == "get_tool_details" {
        let tool_name = args
            .get("tool_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let native_schema = ctx.get_tool_schema.and_then(|f| f(tool_name));
        let result = native_schema
            .or_else(|| mcp_tools::get_mcp_tool_schema(tool_name, mcp_manager))
            .unwrap_or_else(|| {
                format!(
                    "Tool '{}' not found. Use list_tools to see available tools.",
                    tool_name
                )
            });
        return Some(NativeToolResult::text_only(result));
    }

    // Telegram notification
    if name == "send_telegram" {
        return Some(NativeToolResult::text_only(telegram::tool_send_telegram(
            &args, db,
        )));
    }

    // spawn_agent is handled in command_executor.rs (needs model access).
    if name == "spawn_agent" {
        return Some(NativeToolResult::text_only(
            "Error: spawn_agent must be handled by the generation pipeline".to_string(),
        ));
    }

    // browser_search: navigate to Google, extract results
    if name == "browser_search" {
        let query = match args.get("query").and_then(|v| v.as_str()) {
            Some(q) if !q.trim().is_empty() => q.trim(),
            _ => {
                return Some(NativeToolResult::text_only(
                    "Error: 'query' is required".into(),
                ))
            }
        };
        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(8) as usize;
        let encoded = urlencoding::encode(query);
        let search_url =
            format!("https://www.google.com/search?q={encoded}&num={max_results}&hl=en");

        let _ = browser_session::notify_tauri_browser_navigate(&search_url);
        std::thread::sleep(std::time::Duration::from_millis(2500));

        let js = format!(
            r#"Array.from(document.querySelectorAll('a')).filter(a => a.querySelector('h3')).slice(0, {max}).map(a => {{
                const title = a.querySelector('h3')?.textContent || '';
                const url = a.href || '';
                const parent = a.closest('[data-sokoban-container], [data-hveid], [jscontroller]');
                const snippet = parent?.querySelector('[data-sncf], .VwiC3b, [style*="-webkit-line-clamp"], span:not(:has(*))')?.textContent || '';
                return {{ title, url, snippet }};
            }}).filter(r => r.url && !r.url.includes('google.com/search'))"#,
            max = max_results,
        );
        match browser_session::eval_in_browser_panel(&js) {
            Ok(text) => {
                if let Ok(results) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
                    if results.is_empty() {
                        return Some(NativeToolResult::text_only(format!(
                            "No results found for '{query}'. Try a different query."
                        )));
                    }
                    let mut output = format!("Search results for '{query}':\n\n");
                    for (i, r) in results.iter().enumerate() {
                        let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("");
                        let url = r.get("url").and_then(|v| v.as_str()).unwrap_or("");
                        let snippet = r.get("snippet").and_then(|v| v.as_str()).unwrap_or("");
                        output.push_str(&format!(
                            "{}. {}\n   URL: {}\n   {}\n\n",
                            i + 1,
                            title,
                            url,
                            snippet
                        ));
                    }
                    return Some(NativeToolResult::text_only(output));
                }
                NativeToolResult::text_only(text)
            }
            Err(e) => NativeToolResult::text_only(format!("Search failed: {e}")),
        };
        return Some(NativeToolResult::text_only(format!(
            "Search for '{query}' completed."
        )));
    }

    // Browser view tools (open/close the in-app browser panel via Tauri)
    if name == "open_browser_view" {
        let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("");
        if url.is_empty() {
            return Some(NativeToolResult::text_only(
                "Error: 'url' argument is required".to_string(),
            ));
        }
        let full_url = if url.starts_with("http://") || url.starts_with("https://") {
            url.to_string()
        } else {
            format!("https://{url}")
        };
        let _ = browser_session::notify_tauri_browser_navigate(&full_url);
        return Some(NativeToolResult::text_only(format!(
            "Opened browser view for {full_url}."
        )));
    }
    if name == "close_browser_view" {
        let _ = browser_session::notify_tauri_browser_close();
        return Some(NativeToolResult::text_only(
            "Browser view closed.".to_string(),
        ));
    }

    // Unified browser_* tools
    if let Some(browser_name) = name.strip_prefix("browser_") {
        return Some(browser_tools::handle_browser_tool(browser_name, &args));
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
        "execute_command" => {
            let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
            if command.is_empty() {
                return Some(NativeToolResult::text_only(
                    "Error: 'command' argument is required".to_string(),
                ));
            }
            let is_background = args
                .get("background")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if is_background {
                llama_chat_command::background::execute_command_background(command, |_| {})
            } else {
                let timeout = args.get("timeout").and_then(|v| v.as_u64());
                llama_chat_command::execute_command_streaming_with_timeout(
                    command,
                    None,
                    timeout,
                    &mut |_| {},
                )
            }
        }
        "check_background_process" => {
            let pid = args
                .get("pid")
                .and_then(|v| {
                    v.as_u64()
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
                })
                .unwrap_or(0) as u32;
            if pid == 0 {
                return Some(NativeToolResult::text_only(
                    "Error: 'pid' argument is required and must be a positive integer".to_string(),
                ));
            }
            let wait_seconds = args
                .get("wait_seconds")
                .and_then(|v| {
                    v.as_u64()
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
                })
                .unwrap_or(0);
            llama_chat_command::background::check_background_process(pid, wait_seconds)
        }
        "wait" => {
            let seconds = args
                .get("seconds")
                .and_then(|v| {
                    v.as_u64()
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
                })
                .unwrap_or(10);
            let seconds = seconds.min(30);
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            format!(
                "Waited {} seconds. You can now check on background processes or continue.",
                seconds
            )
        }
        "lsp_query" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("definition");
            let symbol = args.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
            let file = args.get("file").and_then(|v| v.as_str());
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");

            if symbol.is_empty() && action != "symbols" && action != "diagnostics" {
                "Error: 'symbol' is required".to_string()
            } else {
                let result = match action {
                    "definition" => {
                        if let Some(ctags) = command_tools::get_ctags(path) {
                            let matches: Vec<&str> = ctags
                                .lines()
                                .filter(|line| {
                                    line.contains(&format!("\"name\":\"{}\"", symbol))
                                        || line.starts_with(&format!("{}\t", symbol))
                                })
                                .take(10)
                                .collect();
                            if !matches.is_empty() {
                                format!(
                                    "Definitions found via ctags:\n{}",
                                    matches.join("\n")
                                )
                            } else {
                                command_tools::lsp_ripgrep_definition(symbol, path)
                            }
                        } else {
                            command_tools::lsp_ripgrep_definition(symbol, path)
                        }
                    }
                    "references" => {
                        let cmd = format!(
                            "rg -n -w \"{}\" \"{}\" --type-add \"code:*.{{rs,ts,tsx,js,jsx,py,go,java,c,cpp,h,hpp,nim,ex}}\" -t code --max-count 30",
                            symbol, path
                        );
                        llama_chat_command::execute_command(&cmd)
                    }
                    "symbols" => {
                        let target = file.unwrap_or(path);
                        if let Some(ctags) = command_tools::get_ctags(target) {
                            let file_matches: Vec<&str> = ctags
                                .lines()
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
                        let ext = file
                            .and_then(|f| std::path::Path::new(f).extension())
                            .and_then(|e| e.to_str())
                            .unwrap_or("");
                        match ext {
                            "rs" => llama_chat_command::execute_command(
                                "cargo check --message-format=short 2>&1 | head -30",
                            ),
                            "py" => {
                                if let Some(f) = file {
                                    llama_chat_command::execute_command(&format!(
                                        "python -m py_compile {} 2>&1",
                                        f
                                    ))
                                } else {
                                    "Error: 'file' is required for Python diagnostics"
                                        .to_string()
                                }
                            }
                            "ts" | "tsx" => llama_chat_command::execute_command(
                                "npx tsc --noEmit 2>&1 | head -30",
                            ),
                            "nim" => {
                                if let Some(f) = file {
                                    llama_chat_command::execute_command(&format!(
                                        "nim check {} 2>&1 | head -30",
                                        f
                                    ))
                                } else {
                                    "Error: 'file' is required for Nim diagnostics"
                                        .to_string()
                                }
                            }
                            _ => "No diagnostic tool available for this file type. Use execute_command to run your build tool.".to_string(),
                        }
                    }
                    "hover" => {
                        let escaped = regex::escape(symbol);
                        let pattern = format!(
                            r"(fn|struct|enum|trait|type|class|def|interface)\s+{}",
                            escaped
                        );
                        let cmd = format!(
                            "rg -n -A 5 \"{}\" \"{}\" --type-add \"code:*.{{rs,ts,tsx,js,jsx,py,go,java,c,cpp,h,hpp}}\" -t code --max-count 5",
                            pattern, path
                        );
                        llama_chat_command::execute_command(&cmd)
                    }
                    _ => format!(
                        "Unknown action '{}'. Use: definition, references, symbols, hover, diagnostics",
                        action
                    ),
                };
                if result.trim().is_empty() {
                    format!(
                        "No results found for '{}' ({}) in {}",
                        symbol, action, path
                    )
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
            let seconds = args
                .get("seconds")
                .and_then(|v| v.as_u64())
                .unwrap_or(5)
                .min(30);
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            format!("Waited {} seconds", seconds)
        }
        "todo_write" => {
            let todos = args
                .get("todos")
                .and_then(|v| v.as_str())
                .unwrap_or("[]");
            match serde_json::from_str::<serde_json::Value>(todos) {
                Ok(val) => {
                    let formatted =
                        serde_json::to_string_pretty(&val).unwrap_or_else(|_| todos.to_string());
                    if let Ok(mut store) = todo_store().lock() {
                        store.insert("default".to_string(), formatted.clone());
                    }
                    format!("Todo list updated:\n{}", formatted)
                }
                Err(e) => format!(
                    "Error: Invalid JSON for todos: {e}. Expected array of {{id, task, status}} objects."
                ),
            }
        }
        "todo_read" => {
            let todos = todo_store()
                .lock()
                .ok()
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
            if let Some(discover) = ctx.discover_skills {
                let skills = discover(&cwd);
                if skills.is_empty() {
                    "No skills found. Create .md files in a 'skills/' directory with YAML frontmatter (name, description).".to_string()
                } else {
                    let mut output = format!("{} skills available:\n", skills.len());
                    for s in &skills {
                        output.push_str(&format!("  {} — {}\n", s.name, s.description));
                    }
                    output
                }
            } else {
                "Skills system not available".to_string()
            }
        }
        "use_skill" => {
            let skill_name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let template_args = args.get("args").and_then(|v| v.as_str()).unwrap_or("{}");

            let cwd = std::env::current_dir().unwrap_or_default();
            if let Some(get_skill_fn) = ctx.get_skill {
                match get_skill_fn(&cwd, skill_name) {
                    Some(skill) => {
                        let mut content = skill.content.clone();
                        if let Ok(args_map) = serde_json::from_str::<
                            serde_json::Map<String, serde_json::Value>,
                        >(template_args)
                        {
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
                    None => format!(
                        "Skill '{}' not found. Use list_skills to see available skills.",
                        skill_name
                    ),
                }
            } else {
                "Skills system not available".to_string()
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
                format!(
                    "Error: URL must start with http:// or https://, got: {}",
                    url
                )
            } else {
                #[cfg(target_os = "windows")]
                let result =
                    std::process::Command::new("cmd")
                        .args(["/C", "start", "", url])
                        .spawn();
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
            mcp_tools::ensure_mcp_connected(mcp_manager, db);
            if let Some(mgr) = mcp_manager {
                if mgr.is_mcp_tool(&name) {
                    return Some(NativeToolResult::text_only(
                        match mgr.call_tool(&name, args) {
                            Ok(output) => output,
                            Err(e) => format!("MCP tool error: {e}"),
                        },
                    ));
                }
            }
            return None; // Unknown tool → fall back to shell
        }
    }))
}

/// No-op stub — web_fetch cache was removed.
pub fn clear_web_fetch_cache() {}

#[cfg(test)]
mod tests {
    use super::*;
    use super::parsing::{
        auto_close_json, escape_invalid_backslashes_in_strings, escape_newlines_in_json_strings,
    };
    use serde_json::json;

    fn empty_ctx() -> DispatchContext<'static> {
        DispatchContext {
            get_tool_catalog: None,
            get_tool_schema: None,
            discover_skills: None,
            get_skill: None,
        }
    }

    fn dispatch(text: &str) -> Option<NativeToolResult> {
        dispatch_native_tool(text, false, None, None, &empty_ctx())
    }

    #[test]
    fn test_dispatch_read_file_valid() {
        let temp = std::env::temp_dir().join("native_tools_test_read.txt");
        std::fs::write(&temp, "hello world").unwrap();

        let json = format!(
            r#"{{"name": "read_file", "arguments": {{"path": "{}"}}}}"#,
            temp.display().to_string().replace('\\', "\\\\")
        );
        let result = dispatch(&json);
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
        let result = dispatch(&json);
        assert!(result.is_some());
        assert!(result.unwrap().text.contains("Written"));
        assert_eq!(std::fs::read_to_string(&temp).unwrap(), "test content");

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_list_directory() {
        let json = r#"{"name": "list_directory", "arguments": {"path": "."}}"#;
        let result = dispatch(json);
        assert!(result.is_some());
        assert!(result.unwrap().text.contains("Directory listing"));
    }

    #[test]
    fn test_dispatch_unknown_tool_returns_none() {
        let json = r#"{"name": "unknown_tool", "arguments": {}}"#;
        let result = dispatch(json);
        assert!(result.is_none());
    }

    #[test]
    fn test_dispatch_non_json_returns_none() {
        let result = dispatch("ls -la");
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
        let result = dispatch(&json);
        assert!(result.is_some());
        assert!(result.unwrap().text.contains("mistral test"));

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_mistral_comma_format() {
        let temp = std::env::temp_dir().join("native_tools_test_comma.txt");
        std::fs::write(&temp, "comma format test").unwrap();

        let input = format!(
            r#"read_file,{{"path": "{}"}}"#,
            temp.display().to_string().replace('\\', "\\\\")
        );
        let result = dispatch(&input);
        assert!(result.is_some(), "Should parse Mistral comma format");
        assert!(result.unwrap().text.contains("comma format test"));

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_llama3_xml_format() {
        let temp = std::env::temp_dir().join("native_tools_test_xml.txt");
        std::fs::write(&temp, "xml format test").unwrap();

        let input = format!(
            "<function=read_file> <parameter=path> {} </parameter> </function>",
            temp.display()
        );
        let result = dispatch(&input);
        assert!(result.is_some(), "Should parse Llama3 XML format");
        assert!(result.unwrap().text.contains("xml format test"));

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_name_json_format() {
        let input = r#"list_directory{"path": "."}"#;
        let result = dispatch(input);
        assert!(result.is_some(), "Should parse name+JSON format");
        assert!(result.unwrap().text.contains("Directory listing"));
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
    fn test_auto_close_json_missing_brace() {
        let input = r#"{"name": "write_file", "arguments": {"path": "/tmp/test.txt", "content": "hello"}}"#;
        assert!(serde_json::from_str::<Value>(input).is_ok());
        let broken = &input[..input.len() - 1];
        assert!(serde_json::from_str::<Value>(broken).is_err());
        let fixed = auto_close_json(broken);
        assert_eq!(fixed, input);
        assert!(serde_json::from_str::<Value>(&fixed).is_ok());
    }

    #[test]
    fn test_escape_invalid_backslashes_php_namespaces() {
        let input = r#"{"name":"write_file","arguments":{"path":"Person.php","content":"namespace App\Models;\nuse Illuminate\Database\Eloquent\Model;"}}"#;
        let fixed = escape_invalid_backslashes_in_strings(input);
        assert!(fixed.contains(r"App\\Models"));
        assert!(fixed.contains(r"Illuminate\\Database\\Eloquent\\Model"));
        assert!(
            serde_json::from_str::<Value>(&fixed).is_ok(),
            "Fixed JSON should parse: {}",
            fixed
        );
    }

    #[test]
    fn test_execute_python_with_quotes_and_regex() {
        let code = r#"import re
text = "Invoice INV-2024-0847 total $1,234.56"
match = re.search(r'\$[\d,]+\.\d+', text)
print(f"Found: {match.group()}" if match else "No match")"#;

        let args = json!({"code": code});
        let result = command_tools::tool_execute_python(&args);
        if !result.contains("Error running Python") {
            assert!(result.contains("Found: $1,234.56"));
        }
    }
}
