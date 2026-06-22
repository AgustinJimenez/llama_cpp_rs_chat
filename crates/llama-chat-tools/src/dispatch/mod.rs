use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use std::sync::OnceLock;

use serde_json::Value;

use crate::browser_session;
use crate::browser_tools;
use crate::mcp_tools;
use crate::parsing;
use crate::screenshot_tool;
use crate::telegram;
use crate::tool_defs;
use crate::{DispatchContext, McpManagerOps, NativeToolResult};

mod desktop_dispatch;
mod text_tools;

pub fn extract_tool_name(cmd: &str) -> Option<String> {
    serde_json::from_str::<Value>(cmd).ok().and_then(|v| {
        v.get("name")
            .and_then(|n| n.as_str())
            .map(|s| s.to_string())
    })
}

pub fn extract_tool_args_summary(cmd: &str) -> String {
    let v: Value = match serde_json::from_str(cmd) {
        Ok(v) => v,
        Err(_) => return cmd.chars().take(80).collect(),
    };
    let args = match v.get("arguments") {
        Some(a) => a,
        None => return "(no args)".to_string(),
    };
    if let Some(obj) = args.as_object() {
        for (key, val) in obj.iter().take(2) {
            if let Some(s) = val.as_str() {
                let truncated: String = s.chars().take(80).collect();
                return format!("{key}={truncated}");
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

static TODO_STORE: OnceLock<StdMutex<HashMap<String, String>>> = OnceLock::new();

pub(super) fn todo_store() -> &'static StdMutex<HashMap<String, String>> {
    TODO_STORE.get_or_init(|| StdMutex::new(HashMap::new()))
}

/// Parsed options from an execute_command tool call.
pub struct ExecuteCommandOpts {
    pub command: String,
    pub background: bool,
    pub timeout: Option<u64>,
    pub working_directory: Option<String>,
}

pub fn extract_execute_command_with_opts(text: &str) -> Option<ExecuteCommandOpts> {
    if let Some((name, args)) = parsing::try_parse_tool_call(text) {
        if name == "execute_command" {
            let command = args.get("command").and_then(|v| v.as_str())?;
            if !command.is_empty() {
                let background = args
                    .get("background")
                    .and_then(parsing::value_as_bool_flexible)
                    .unwrap_or(false);
                let timeout = args.get("timeout").and_then(|v| {
                    v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
                });
                let working_directory = args
                    .get("working_directory")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                return Some(ExecuteCommandOpts { command: command.to_string(), background, timeout, working_directory });
            }
        }
        return None;
    }

    let trimmed = text.trim();
    if let Some(parsed) = parsing::try_parse_with_fixups(trimmed) {
        if let Some(command) = parsed.get("command").and_then(|v| v.as_str()) {
            if !command.is_empty() {
                let background = parsed
                    .get("background")
                    .and_then(parsing::value_as_bool_flexible)
                    .unwrap_or(false);
                let timeout = parsed.get("timeout").and_then(|v| {
                    v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
                });
                let working_directory = parsed
                    .get("working_directory")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                return Some(ExecuteCommandOpts { command: command.to_string(), background, timeout, working_directory });
            }
        }
    }
    None
}

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
    "browser_go_back",
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
    "find_executable",
    "check_environment",
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
    "display_images",
];

fn validate_tool_args(tool_name: &str, args: &serde_json::Value) -> Result<(), String> {
    if !VALIDATED_TOOLS.contains(&tool_name) {
        return Ok(());
    }

    let all_tools = tool_defs::all_tool_definitions();
    let tool_def = match all_tools
        .iter()
        .find(|t| t.get("name").and_then(|n| n.as_str()) == Some(tool_name))
    {
        Some(t) => t,
        None => return Ok(()),
    };
    let params = match tool_def.get("parameters") {
        Some(p) => p,
        None => return Ok(()),
    };

    if let Some(required) = params.get("required").and_then(|r| r.as_array()) {
        for req in required {
            if let Some(field_name) = req.as_str() {
                let value = args.get(field_name);
                match value {
                    None | Some(&serde_json::Value::Null) => {
                        return Err(format!(
                            "Missing required parameter '{field_name}' for tool '{tool_name}'. Required parameters: {required:?}"
                        ));
                    }
                    Some(serde_json::Value::String(s)) if s.is_empty() => {
                        // write_file's content param is legitimately empty (e.g. creating __init__.py)
                        if tool_name == "write_file" && field_name == "content" {
                            // allowed
                        } else {
                            return Err(format!(
                                "Parameter '{field_name}' for tool '{tool_name}' cannot be empty"
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if let Some(properties) = params.get("properties").and_then(|p| p.as_object()) {
        let empty = serde_json::Map::new();
        for (field_name, value) in args.as_object().unwrap_or(&empty) {
            if let Some(field_schema) = properties.get(field_name) {
                if let Some(expected_type) = field_schema.get("type").and_then(|t| t.as_str()) {
                    match expected_type {
                        "string" if !value.is_string() => {
                            return Err(type_error(field_name, tool_name, "string", value));
                        }
                        "number" | "integer"
                            if !(value.is_number()
                                || (value.is_string()
                                    && value.as_str().unwrap_or("").parse::<f64>().is_ok())) =>
                        {
                            return Err(type_error(field_name, tool_name, "number", value));
                        }
                        "boolean"
                            if !value.is_boolean()
                                && parsing::value_as_bool_flexible(value).is_none() =>
                        {
                            return Err(type_error(field_name, tool_name, "boolean", value));
                        }
                        "array" if !value.is_array() => {
                            return Err(type_error(field_name, tool_name, "array", value));
                        }
                        "object" if !value.is_object() => {
                            return Err(type_error(field_name, tool_name, "object", value));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(())
}

fn type_error(field_name: &str, tool_name: &str, expected: &str, value: &Value) -> String {
    let got = value_type_name(value);
    format!(
        "Parameter '{field_name}' for tool '{tool_name}' must be a {expected}, got {got}"
    )
}

fn value_type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

pub fn dispatch_native_tool(
    text: &str,
    _use_htmd: bool,
    mcp_manager: Option<&dyn McpManagerOps>,
    db: Option<&llama_chat_db::SharedDatabase>,
    ctx: &DispatchContext<'_>,
) -> Option<NativeToolResult> {
    let trimmed = text.trim();
    let mut calls = parsing::try_parse_all_from_raw(trimmed);
    let (name, args) = calls.drain(..).next()?;

    if let Err(validation_error) = validate_tool_args(&name, &args) {
        return Some(NativeToolResult::text_only(validation_error));
    }

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

    // Delegate all desktop tool calls to the desktop_dispatch module.
    if let Some(result) = desktop_dispatch::dispatch_desktop_tool(&name, &args) {
        return Some(result);
    }

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
                    "Tool '{tool_name}' not found. Use list_tools to see available tools."
                )
            });
        return Some(NativeToolResult::text_only(result));
    }

    if name == "display_images" {
        let urls: Vec<&str> = args
            .get("urls")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).take(20).collect())
            .unwrap_or_default();
        if urls.is_empty() {
            return Some(NativeToolResult::text_only(
                "Error: 'urls' must be a non-empty array of image URLs".to_string(),
            ));
        }
        let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let n = urls.len();
        let output = serde_json::json!({
            "ok": true,
            "urls": urls,
            "title": title,
            "count": n
        });
        return Some(NativeToolResult::text_only(format!(
            "[DISPLAY_IMAGES]{}",
            output
        )));
    }
    if name == "send_telegram" {
        return Some(NativeToolResult::text_only(telegram::tool_send_telegram(
            &args, db,
        )));
    }
    if name == "spawn_agent" {
        return Some(NativeToolResult::text_only(
            "Error: spawn_agent must be handled by the generation pipeline".to_string(),
        ));
    }
    if name == "browser_search" {
        let query = match args.get("query").and_then(|v| v.as_str()) {
            Some(q) if !q.trim().is_empty() => q.trim(),
            _ => return Some(NativeToolResult::text_only("Error: 'query' is required".into())),
        };
        let encoded = urlencoding::encode(query);
        // gl=us&hl=en: force English/US results regardless of user's IP geo-location
        let search_url = format!("https://www.google.com/search?q={encoded}&gl=us&hl=en&num=8");
        if let Err(e) = browser_session::notify_tauri_browser_navigate(&search_url) {
            return Some(NativeToolResult::text_only(format!(
                "Failed to open browser: {e}"
            )));
        }
        std::thread::sleep(std::time::Duration::from_millis(3000));
        match browser_session::eval_in_browser_panel("document.body.innerText") {
            Ok(text) => {
                let trimmed = if text.len() > 8000 {
                    let mut end = 8000;
                    while end > 0 && !text.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!("{}...\n[Truncated]", &text[..end])
                } else {
                    text
                };
                return Some(NativeToolResult::text_only(format!(
                    "Search results for '{query}':\n\n{trimmed}"
                )));
            }
            Err(e) => {
                return Some(NativeToolResult::text_only(format!(
                    "Failed to read search results from browser: {e}"
                )));
            }
        }
    }
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
        return Some(NativeToolResult::text_only("Browser view closed.".to_string()));
    }
    if let Some(browser_name) = name.strip_prefix("browser_") {
        return Some(browser_tools::handle_browser_tool(browser_name, &args));
    }

    if let Some(text) = text_tools::dispatch_text_tool(&name, &args, mcp_manager, db, ctx) {
        return Some(NativeToolResult::text_only(text));
    }

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
    None
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::command_tools;
    use crate::parsing::{
        auto_close_json, escape_invalid_backslashes_in_strings, escape_newlines_in_json_strings,
    };
    use super::*;

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
        let result = dispatch(r#"{"name": "list_directory", "arguments": {"path": "."}}"#);
        assert!(result.is_some());
        assert!(result.unwrap().text.contains("Directory listing"));
    }

    #[test]
    fn test_dispatch_unknown_tool_returns_none() {
        assert!(dispatch(r#"{"name": "unknown_tool", "arguments": {}}"#).is_none());
    }

    #[test]
    fn test_dispatch_non_json_returns_none() {
        assert!(dispatch("ls -la").is_none());
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
        assert!(result.is_some());
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
        assert!(result.is_some());
        assert!(result.unwrap().text.contains("xml format test"));
        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_name_json_format() {
        let result = dispatch(r#"list_directory{"path": "."}"#);
        assert!(result.is_some());
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
        let input =
            r#"{"name": "write_file", "arguments": {"path": "/tmp/test.txt", "content": "hello"}}"#;
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
        assert!(serde_json::from_str::<Value>(&fixed).is_ok());
    }

    #[test]
    fn test_execute_python_with_quotes_and_regex() {
        let code = r#"import re
text = "Invoice INV-2024-0847 total $1,234.56"
match = re.search(r'\$[\d,]+\.\d+', text)
print(f"Found: {match.group()}" if match else "No match")"#;
        let args = json!({ "code": code });
        let result = command_tools::tool_execute_python(&args);
        if !result.contains("Error running Python") {
            assert!(result.contains("Found: $1,234.56"));
        }
    }
}
