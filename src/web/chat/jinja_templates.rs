use minijinja::{context, Environment, Error, ErrorKind};
use serde_json::{json, Value};

/// Preprocess a Jinja2 template string for minijinja compatibility.
///
/// Fixes Python-specific syntax that minijinja doesn't support:
/// - `tojson(ensure_ascii=False)` → `tojson` (minijinja doesn't escape non-ASCII by default)
/// - `.endswith("x")` → ` is endingwith("x")` (Python method → minijinja test)
/// - `.startswith("x")` → ` is startingwith("x")` (Python method → minijinja test)
/// - `.strip()` → ` | trim` (Python method → minijinja filter)
/// - `.items()` → ` | items` (Python dict method → minijinja filter)
fn preprocess_template(template: &str) -> String {
    use regex::Regex;

    let mut result = template
        .replace("tojson(ensure_ascii=False)", "tojson")
        .replace("tojson(ensure_ascii=True)", "tojson");

    // Convert .endswith("x") → is endingwith("x")
    // Handles: expr.endswith("...") or expr.endswith('...')
    if let Ok(re) = Regex::new(r"\.endswith\(") {
        result = re.replace_all(&result, " is endingwith(").to_string();
    }

    // Convert .startswith("x") → is startingwith("x")
    if let Ok(re) = Regex::new(r"\.startswith\(") {
        result = re.replace_all(&result, " is startingwith(").to_string();
    }

    // Convert .strip() → | trim (Python str.strip → Jinja trim filter)
    result = result.replace(".strip()", " | trim");

    // Convert .items() → | items (Python dict.items() → minijinja filter)
    // Used by Harmony templates: `for key, val in dict.items()`
    result = result.replace(".items()", " | items");

    result
}

/// Apply native Jinja2 chat template from model metadata
///
/// This function takes the raw Jinja2 template from the model's tokenizer.chat_template
/// and applies it with the provided messages, tools, and documents.
pub fn apply_native_chat_template(
    template_string: &str,
    messages: Vec<ChatMessage>,
    tools: Option<Vec<Value>>,
    documents: Option<Vec<Value>>,
    add_generation_prompt: bool,
    bos_token: &str,
    eos_token: &str,
) -> Result<String, String> {
    // Preprocess template for minijinja compatibility
    let processed_template = preprocess_template(template_string);

    // Create Jinja2 environment
    let mut env = Environment::new();

    // Register custom functions that real GGUF templates use
    // raise_exception(msg) — used by GLM-4.6, Devstral, Ministral for validation
    env.add_function("raise_exception", |msg: String| -> Result<String, Error> {
        Err(Error::new(ErrorKind::InvalidOperation, msg))
    });

    // strftime_now(fmt) — used by Mistral templates for current date
    env.add_function("strftime_now", |fmt: String| -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let secs = now.as_secs() as i64;
        // Simple date formatting without chrono dependency
        if fmt.contains("%Y") || fmt.contains("%m") || fmt.contains("%d") {
            // Convert epoch to YYYY-MM-DD (UTC)
            let days = secs / 86400;
            let (year, month, day) = epoch_days_to_ymd(days);
            fmt.replace("%Y", &format!("{year:04}"))
                .replace("%m", &format!("{month:02}"))
                .replace("%d", &format!("{day:02}"))
        } else {
            // Fallback: return ISO date
            let days = secs / 86400;
            let (year, month, day) = epoch_days_to_ymd(days);
            format!("{year:04}-{month:02}-{day:02}")
        }
    });

    // Add the template
    env.add_template("chat_template", &processed_template)
        .map_err(|e| format!("Failed to parse chat template: {e}"))?;

    // Prepare context variables that the template expects
    let tools_vec = tools.unwrap_or_default();
    let documents_vec = documents.unwrap_or_default();
    let template_context = context! {
        messages => messages,
        tools => &tools_vec,
        documents => &documents_vec,
        add_generation_prompt => add_generation_prompt,
        // Common Jinja2 template variables
        available_tools => &tools_vec,
        bos_token => bos_token,
        eos_token => eos_token,
        // Disable thinking/reasoning mode — models like GLM-4 check this variable
        // and enter <think> mode if it's undefined, causing immediate EOS
        enable_thinking => false,
    };

    // Render the template
    let template = env.get_template("chat_template")
        .map_err(|e| format!("Failed to get template: {e}"))?;

    template.render(&template_context)
        .map_err(|e| format!("Failed to render template: {e}"))
}

/// Convert epoch days (since 1970-01-01) to (year, month, day).
fn epoch_days_to_ymd(days: i64) -> (i64, u32, u32) {
    // Civil calendar algorithm from Howard Hinnant
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

/// Chat message structure for Jinja2 templates
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub tool_calls: Option<Vec<ToolCall>>,
}

/// Tool call structure for chat templates
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
    pub function: Option<ToolFunction>,
}

/// Tool function structure
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ToolFunction {
    pub name: String,
    pub arguments: String,
}

/// Get available tools in OpenAI function-calling format for Jinja templates.
///
/// Models are trained on OpenAI API format: `{"type": "function", "function": {...}}`.
/// The Jinja templates serialize these via `tools | tojson`, so the format matters
/// for model comprehension.
#[allow(dead_code)]
pub fn get_available_tools_openai() -> Vec<Value> {
    get_available_tools_openai_with_mcp(None)
}

/// Get available tools in OpenAI format, optionally including MCP tools.
pub fn get_available_tools_openai_with_mcp(mcp_tools: Option<&[super::super::mcp::McpToolDef]>) -> Vec<Value> {
    let mut tools: Vec<Value> = get_available_tools()
        .into_iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": tool
            })
        })
        .collect();

    // Append MCP tool definitions
    if let Some(mcp) = mcp_tools {
        for t in mcp {
            tools.push(t.to_openai_function());
        }
    }

    tools
}

/// Parse conversation text into ChatMessage format for Jinja rendering.
///
/// Unlike `parse_conversation_to_messages()`, this version:
/// - Replaces the stored SYSTEM: block with a provided system prompt
/// - Keeps tool calls/responses as inline text in assistant content (phase 1)
pub fn parse_conversation_for_jinja(
    conversation: &str,
    system_prompt: &str,
) -> Vec<ChatMessage> {
    let mut messages = Vec::new();

    // Always start with our behavioral system prompt
    messages.push(ChatMessage {
        role: "system".to_string(),
        content: system_prompt.to_string(),
        tool_calls: None,
    });

    let mut current_role = "";
    let mut current_content = String::new();

    for line in conversation.lines() {
        if line.ends_with(":")
            && (line.starts_with("SYSTEM:")
                || line.starts_with("USER:")
                || line.starts_with("ASSISTANT:"))
        {
            // Save previous role's content (skip SYSTEM — replaced above)
            if !current_role.is_empty()
                && current_role != "SYSTEM"
                && !current_content.trim().is_empty()
            {
                messages.push(ChatMessage {
                    role: current_role.to_lowercase(),
                    content: current_content.trim().to_string(),
                    tool_calls: None,
                });
            }

            current_role = line.trim_end_matches(':');
            current_content.clear();
        } else if !current_role.is_empty() {
            if !current_content.is_empty() {
                current_content.push('\n');
            }
            current_content.push_str(line);
        }
    }

    // Add final message (skip SYSTEM)
    if !current_role.is_empty()
        && current_role != "SYSTEM"
        && !current_content.trim().is_empty()
    {
        messages.push(ChatMessage {
            role: current_role.to_lowercase(),
            content: current_content.trim().to_string(),
            tool_calls: None,
        });
    }

    messages
}

/// Core tool names always included in the system prompt.
/// Desktop and admin tools are discoverable via `list_tools`/`get_tool_details`.
const CORE_TOOL_NAMES: &[&str] = &[
    "read_file", "write_file", "edit_file", "undo_edit", "insert_text",
    "search_files", "find_files", "lsp_query", "execute_python", "execute_command",
    "list_directory", "open_url",
    // "web_search", "web_fetch", // disabled — use browser_* tools instead
    "git_status", "git_diff", "git_commit",
    "check_background_process", "list_background_processes",
    "send_telegram", "spawn_agent",
    "list_skills", "use_skill", "set_response_style",
    "ocr_screen",
    // Unified browser session tools (Camofox-backed; Tauri-native later)
    "browser_navigate", "browser_click", "browser_type", "browser_eval",
    "browser_get_html", "browser_screenshot", "browser_wait", "browser_close",
    "browser_get_text", "browser_get_links", "browser_snapshot",
    "browser_scroll", "browser_press_key",
];

/// Admin tool names (MCP server management).
const ADMIN_TOOL_NAMES: &[&str] = &[
    "list_mcp_servers", "add_mcp_server", "remove_mcp_server",
];

/// Get available tools for the template context — core tools + catalog tools only.
/// Desktop and admin tools are discoverable on demand via `list_tools`/`get_tool_details`.
pub fn get_available_tools() -> Vec<Value> {
    let mut tools: Vec<Value> = get_all_tools()
        .into_iter()
        .filter(|tool| {
            tool.get("name")
                .and_then(|n| n.as_str())
                .map_or(false, |name| CORE_TOOL_NAMES.contains(&name))
        })
        .collect();

    // Add catalog tools for discovering desktop/admin/mcp tools
    tools.push(json!({
        "name": "list_tools",
        "description": "List available tools in a category. Categories: 'desktop' (screen automation, mouse, keyboard, windows, OCR, clipboard), 'mcp' (connected MCP server tools), 'admin' (MCP server management). Returns tool names with brief descriptions.",
        "parameters": {
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "description": "Tool category: 'desktop', 'mcp', or 'admin'"
                }
            },
            "required": ["category"]
        }
    }));
    tools.push(json!({
        "name": "get_tool_details",
        "description": "Get the full parameter schema for a specific tool. Use after list_tools to see the exact parameters for a tool you want to use.",
        "parameters": {
            "type": "object",
            "properties": {
                "tool_name": {
                    "type": "string",
                    "description": "Name of the tool to get details for"
                }
            },
            "required": ["tool_name"]
        }
    }));

    tools
}

/// Get ALL tools (core + desktop + admin). Used internally for tool lookup and MCP server.
pub fn get_all_tools() -> Vec<Value> {
    super::tool_defs::all_tool_definitions()
        .into_iter()
        .filter(|tool| {
            tool.get("name")
                .and_then(|name| name.as_str())
                .map(desktop_tool_available_on_current_platform)
                .unwrap_or(true)
        })
        .collect()
}

/// Desktop tool names exposed via the MCP server.
/// This list must stay in sync with `dispatch_desktop_tool()` in desktop_tools/mod.rs.
#[allow(dead_code)]
pub(crate) const DESKTOP_TOOL_NAMES: &[&str] = &[
    "take_screenshot", "click_screen", "type_text", "press_key", "move_mouse",
    "scroll_screen", "mouse_drag", "mouse_button", "paste", "clear_field", "hover_element",
    "screenshot_region", "screenshot_diff", "window_screenshot", "wait_for_screen_change",
    "ocr_screen", "ocr_find_text", "get_ui_tree", "click_ui_element", "invoke_ui_action",
    "read_ui_element_value", "wait_for_ui_element", "clipboard_image", "find_ui_elements",
    "list_windows", "get_active_window", "focus_window", "minimize_window", "maximize_window",
    "close_window", "resize_window", "wait_for_window", "click_window_relative", "snap_window",
    "set_window_topmost", "open_application", "list_processes", "kill_process",
    "send_keys_to_window", "switch_virtual_desktop", "get_process_info",
    "read_clipboard", "write_clipboard", "get_cursor_position", "get_pixel_color", "list_monitors",
    "find_and_click_text", "type_into_element", "get_window_text", "file_dialog_navigate",
    "drag_and_drop_element", "wait_for_text_on_screen", "get_context_menu", "scroll_element",
    "smart_wait", "click_and_verify",
    "handle_dialog", "wait_for_element_state", "fill_form", "run_action_sequence",
    "move_to_monitor", "set_window_opacity", "highlight_point",
    "annotate_screenshot", "ocr_region", "find_color_on_screen", "find_image_on_screen",
    "read_registry", "click_tray_icon", "watch_window", "execute_app_script",
    "send_notification",
    "show_status_overlay", "update_status_overlay", "hide_status_overlay",
    // Round 4-5 tools
    "get_system_volume", "set_system_volume", "set_system_mute", "list_audio_devices",
    "clear_clipboard", "clipboard_file_paths", "clipboard_html",
    "save_window_layout", "restore_window_layout",
    "wait_for_process_exit", "get_process_tree", "get_system_metrics",
    "wait_for_notification", "dismiss_all_notifications",
    "start_screen_recording", "stop_screen_recording", "capture_gif",
    "dialog_handler_start", "dialog_handler_stop",
];

fn desktop_tool_available_on_current_platform(name: &str) -> bool {
    if !DESKTOP_TOOL_NAMES.contains(&name) {
        return true;
    }

    #[cfg(windows)]
    {
        true
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        !matches!(
            name,
            "get_ui_tree"
                | "click_ui_element"
                | "invoke_ui_action"
                | "read_ui_element_value"
                | "wait_for_ui_element"
                | "find_ui_elements"
                | "file_dialog_navigate"
                | "get_context_menu"
                | "handle_dialog"
                | "wait_for_element_state"
                | "fill_form"
                | "hover_element"
                | "move_to_monitor"
                | "set_window_opacity"
                | "dialog_handler_start"
        )
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        false
    }
}

/// Get only the desktop automation tool definitions (for the MCP server).
/// Returns the subset of `get_all_tools()` that matches desktop tool names.
#[allow(dead_code)]
pub fn get_desktop_tool_definitions() -> Vec<Value> {
    get_all_tools()
        .into_iter()
        .filter(|tool| {
            tool.get("name")
                .and_then(|n| n.as_str())
                .map_or(false, |name| DESKTOP_TOOL_NAMES.contains(&name))
        })
        .collect()
}

/// Get brief descriptions for tools in a category.
/// Returns one-liner descriptions like: "click_screen: Click the mouse at screen coordinates.\n..."
pub fn get_tool_catalog(category: &str) -> String {
    let all_tools = get_all_tools();
    let filter_names: &[&str] = match category {
        "desktop" => DESKTOP_TOOL_NAMES,
        "admin" => ADMIN_TOOL_NAMES,
        "mcp" => {
            // MCP tools are not in our static list; caller should handle separately
            return "MCP tools are dynamically loaded from connected servers. Use list_mcp_servers to see connected servers and their tools.".to_string();
        }
        _ => return format!("Unknown category '{}'. Valid categories: 'desktop', 'mcp', 'admin'", category),
    };

    let mut lines = Vec::new();
    for tool in &all_tools {
        let name = match tool.get("name").and_then(|n| n.as_str()) {
            Some(n) => n,
            None => continue,
        };
        if !filter_names.contains(&name) {
            continue;
        }
        let desc = tool.get("description").and_then(|d| d.as_str()).unwrap_or("");
        // Take first sentence (up to first period)
        let brief = match desc.find('.') {
            Some(pos) => &desc[..=pos],
            None => desc,
        };
        lines.push(format!("{}: {}", name, brief));
    }

    if lines.is_empty() {
        format!("No tools found in category '{}'.", category)
    } else {
        lines.join("\n")
    }
}

/// Get full JSON schema for a specific tool by name.
/// Looks up the tool in `get_all_tools()` and returns its JSON as a pretty-printed string.
pub fn get_tool_schema(tool_name: &str) -> Option<String> {
    get_all_tools()
        .into_iter()
        .find(|tool| {
            tool.get("name").and_then(|n| n.as_str()) == Some(tool_name)
        })
        .map(|tool| serde_json::to_string_pretty(&tool).unwrap_or_else(|_| tool.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_conversation_for_jinja_replaces_system() {
        let conversation = r#"SYSTEM:
Old system prompt that should be replaced.

USER:
Hello!

ASSISTANT:
Hi there!"#;

        let messages = parse_conversation_for_jinja(conversation, "My behavioral prompt");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[0].content, "My behavioral prompt");
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[1].content, "Hello!");
        assert_eq!(messages[2].role, "assistant");
        assert_eq!(messages[2].content, "Hi there!");
    }

    #[test]
    fn test_get_available_tools_openai_format() {
        let tools = get_available_tools_openai();
        assert!(!tools.is_empty());
        // Each tool should have "type": "function" wrapper
        for tool in &tools {
            assert_eq!(tool["type"], "function");
            assert!(tool["function"]["name"].is_string());
            assert!(tool["function"]["description"].is_string());
        }
    }

    #[test]
    fn test_preprocess_template_strips_ensure_ascii() {
        let input = r#"{{ tool | tojson(ensure_ascii=False) }}"#;
        let output = preprocess_template(input);
        assert_eq!(output, "{{ tool | tojson }}");
    }

    #[test]
    fn test_preprocess_template_converts_endswith() {
        let input = r#"not visible_text(m.content).endswith("/nothink")"#;
        let output = preprocess_template(input);
        assert_eq!(output, r#"not visible_text(m.content) is endingwith("/nothink")"#);
    }

    #[test]
    fn test_preprocess_template_converts_startswith() {
        let input = r#"message.content.startswith('<tool_response>')"#;
        let output = preprocess_template(input);
        assert_eq!(output, r#"message.content is startingwith('<tool_response>')"#);
    }

    #[test]
    fn test_preprocess_template_converts_strip() {
        let input = r#"{{ content.strip() }}"#;
        let output = preprocess_template(input);
        assert_eq!(output, r#"{{ content | trim }}"#);
    }

    #[test]
    fn test_simple_chatml_jinja_render() {
        // Minimal ChatML template for testing
        let template = r#"{%- for message in messages %}
<|im_start|>{{ message.role }}
{{ message.content }}<|im_end|>
{%- endfor %}
{%- if add_generation_prompt %}
<|im_start|>assistant
{%- endif %}"#;

        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "You are helpful.".to_string(),
                tool_calls: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: "Hello!".to_string(),
                tool_calls: None,
            },
        ];

        let result = apply_native_chat_template(
            template,
            messages,
            None,
            None,
            true,
            "<s>",
            "</s>",
        );
        assert!(result.is_ok());
        let prompt = result.unwrap();
        assert!(prompt.contains("<|im_start|>system"));
        assert!(prompt.contains("You are helpful."));
        assert!(prompt.contains("<|im_start|>user"));
        assert!(prompt.contains("Hello!"));
        assert!(prompt.contains("<|im_start|>assistant"));
    }

    #[test]
    fn test_raise_exception_works() {
        let template = r#"{% if true %}{{ raise_exception("test error") }}{% endif %}"#;
        let messages = vec![];
        let result = apply_native_chat_template(template, messages, None, None, false, "", "");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("test error"));
    }

    #[test]
    fn test_strftime_now_works() {
        let template = r#"{{ strftime_now("%Y-%m-%d") }}"#;
        let messages = vec![];
        let result = apply_native_chat_template(template, messages, None, None, false, "", "");
        assert!(result.is_ok());
        let date = result.unwrap();
        // Should be a valid YYYY-MM-DD format
        assert_eq!(date.len(), 10);
        assert_eq!(&date[4..5], "-");
        assert_eq!(&date[7..8], "-");
    }

    #[test]
    fn test_epoch_days_to_ymd() {
        // 2026-03-01 = day 20513 since epoch
        let (y, m, d) = epoch_days_to_ymd(20513);
        assert_eq!((y, m, d), (2026, 3, 1));
        // 1970-01-01 = day 0
        let (y, m, d) = epoch_days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }
}
