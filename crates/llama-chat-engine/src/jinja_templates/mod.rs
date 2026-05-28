mod tool_catalog;

#[cfg(test)]
mod tests;

use minijinja::{context, Environment, Error, ErrorKind};
use serde_json::{json, Value};

// Re-export all public items from tool_catalog
pub use tool_catalog::{
    get_all_tools,
    get_available_tools,
    get_desktop_tool_definitions,
    get_tool_catalog,
    get_tool_schema,
};

/// Preprocess a Jinja2 template string for minijinja compatibility.
pub(crate) fn preprocess_template(template: &str) -> String {
    use regex::Regex;

    let mut result = template
        .replace("tojson(ensure_ascii=False)", "tojson")
        .replace("tojson(ensure_ascii=True)", "tojson");

    if let Ok(re) = Regex::new(r"\.endswith\(") {
        result = re.replace_all(&result, " is endingwith(").to_string();
    }

    if let Ok(re) = Regex::new(r"\.startswith\(") {
        result = re.replace_all(&result, " is startingwith(").to_string();
    }

    result = result.replace(".strip()", " | trim");
    result = result.replace(".items()", " | items");
    result = result.replace("[::-1]", " | reverse");

    if let Ok(re) = Regex::new(r#"\.split\((['"][^'"]*['"])\)\[0\]"#) {
        result = re.replace_all(&result, " | split($1) | first").to_string();
    }
    if let Ok(re) = Regex::new(r#"\.split\((['"][^'"]*['"])\)\[-1\]"#) {
        result = re.replace_all(&result, " | split($1) | last").to_string();
    }

    if let Ok(re) = Regex::new(r"\.rstrip\([^)]*\)") {
        result = re.replace_all(&result, " | trim").to_string();
    }
    if let Ok(re) = Regex::new(r"\.lstrip\([^)]*\)") {
        result = re.replace_all(&result, " | trim").to_string();
    }

    result
}

/// Detect whether a Jinja2 chat template supports thinking mode.
pub fn detect_thinking_support(template: &str) -> bool {
    template.contains("enable_thinking") || template.contains("clear_thinking")
}

pub fn apply_native_chat_template(
    template_string: &str,
    messages: Vec<ChatMessage>,
    tools: Option<Vec<Value>>,
    documents: Option<Vec<Value>>,
    add_generation_prompt: bool,
    bos_token: &str,
    eos_token: &str,
    enable_thinking: bool,
) -> Result<String, String> {
    let processed_template = preprocess_template(template_string);

    let mut env = Environment::new();

    env.add_function("raise_exception", |msg: String| -> Result<String, Error> {
        Err(Error::new(ErrorKind::InvalidOperation, msg))
    });

    env.add_function("strftime_now", |fmt: String| -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let secs = now.as_secs() as i64;
        if fmt.contains("%Y") || fmt.contains("%m") || fmt.contains("%d") {
            let days = secs / 86400;
            let (year, month, day) = epoch_days_to_ymd(days);
            fmt.replace("%Y", &format!("{year:04}"))
                .replace("%m", &format!("{month:02}"))
                .replace("%d", &format!("{day:02}"))
        } else {
            let days = secs / 86400;
            let (year, month, day) = epoch_days_to_ymd(days);
            format!("{year:04}-{month:02}-{day:02}")
        }
    });

    env.add_template("chat_template", &processed_template)
        .map_err(|e| format!("Failed to parse chat template: {e}"))?;

    let tools_vec = tools.unwrap_or_default();
    let documents_vec = documents.unwrap_or_default();
    let template_context = context! {
        messages => messages,
        tools => &tools_vec,
        documents => &documents_vec,
        add_generation_prompt => add_generation_prompt,
        available_tools => &tools_vec,
        bos_token => bos_token,
        eos_token => eos_token,
        enable_thinking => enable_thinking,
        clear_thinking => true,
    };

    let template = env.get_template("chat_template")
        .map_err(|e| format!("Failed to get template: {e}"))?;

    template.render(&template_context)
        .map_err(|e| format!("Failed to render template: {e}"))
}

/// Convert epoch days (since 1970-01-01) to (year, month, day).
pub(crate) fn epoch_days_to_ymd(days: i64) -> (i64, u32, u32) {
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
#[allow(dead_code)]
pub fn get_available_tools_openai() -> Vec<Value> {
    get_available_tools_openai_with_mcp(None)
}

/// Get available tools in OpenAI format, optionally including MCP tools.
pub fn get_available_tools_openai_with_mcp(mcp_tools: Option<&[llama_chat_tools::McpToolDefInfo]>) -> Vec<Value> {
    let mut tools: Vec<Value> = get_available_tools()
        .into_iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": tool
            })
        })
        .collect();

    if let Some(mcp) = mcp_tools {
        for t in mcp {
            tools.push(t.to_openai_function());
        }
    }

    tools
}

/// Parse conversation text into ChatMessage format for Jinja rendering.
pub fn parse_conversation_for_jinja(
    conversation: &str,
    system_prompt: &str,
) -> Vec<ChatMessage> {
    let mut messages = Vec::new();

    let mut current_role = "";
    let mut current_content = String::new();
    let mut compaction_summaries: Vec<String> = Vec::new();

    for line in conversation.lines() {
        if line.ends_with(":")
            && (line.starts_with("SYSTEM:")
                || line.starts_with("USER:")
                || line.starts_with("ASSISTANT:"))
        {
            if !current_role.is_empty() && !current_content.trim().is_empty() {
                if current_role == "SYSTEM" {
                    if current_content.trim().starts_with("[Conversation summary") {
                        compaction_summaries.push(current_content.trim().to_string());
                    }
                } else {
                    messages.push(ChatMessage {
                        role: current_role.to_lowercase(),
                        content: current_content.trim().to_string(),
                        tool_calls: None,
                    });
                }
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

    if !current_role.is_empty() && !current_content.trim().is_empty() {
        if current_role == "SYSTEM" {
            if current_content.trim().starts_with("[Conversation summary") {
                compaction_summaries.push(current_content.trim().to_string());
            }
        } else {
            messages.push(ChatMessage {
                role: current_role.to_lowercase(),
                content: current_content.trim().to_string(),
                tool_calls: None,
            });
        }
    }

    let mut system_content = system_prompt.to_string();
    for summary in &compaction_summaries {
        system_content.push_str("\n\n---\n");
        system_content.push_str(summary);
    }

    messages.insert(0, ChatMessage {
        role: "system".to_string(),
        content: system_content,
        tool_calls: None,
    });

    messages
}
