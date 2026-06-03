//! HTTP request building for the OpenAI-compatible provider.
//!
//! Covers: provider default parameters, user parameter application,
//! tool output truncation/summarization, context budget trimming,
//! tool definitions, and local tool execution.

use serde_json::{json, Value};

/// Maximum agentic loop iterations to prevent runaway tool-call chains.
// Safety limit disabled — let the model run as many tool calls as needed.
// The context budget check (MAX_INPUT_TOKENS) handles runaway conversations.
pub(super) const MAX_AGENTIC_ITERATIONS: usize = 2000;

/// Conservative input token limit for context management.
pub(super) const MAX_INPUT_TOKENS: u64 = 100_000;

/// Threshold for tool output summarization (chars).
pub(super) const TOOL_SUMMARIZE_THRESHOLD: usize = 1500;

/// System prompt for cloud provider agentic loops.
pub(super) fn get_cloud_system_prompt() -> &'static str {
    r#"You are an AI assistant with access to tools for file operations, command execution, web search, and more. Follow these guidelines:

1. INVESTIGATE before acting: Check existing files, directory structure, and installed tools before creating or modifying anything.
2. Use web_search when you need current information, documentation, or to find solutions to errors.
3. For execute_command: ALWAYS set the "background" field. Use background=true ONLY for servers/daemons, false for everything else.
4. When creating projects: use the standard tooling (django-admin, npm init, cargo init, etc.) rather than writing every file manually.
5. After making changes, verify they work (run tests, check syntax, start the server briefly).
6. If a command fails, read the error carefully and fix the issue rather than retrying the same command.
7. Keep responses concise. Show what you did and the result, not lengthy explanations."#
}

/// Approximate cost per 1M tokens for known providers.
/// Returns (input_cost_per_million, output_cost_per_million, cache_discount).
/// cache_discount is the fraction of input cost for cached tokens (e.g. 0.1 = 90% off).
pub(super) fn provider_cost_per_million(provider_id: &str, model: &str) -> Option<(f64, f64, f64)> {
    match provider_id {
        "groq" => Some((0.05, 0.10, 1.0)),
        "gemini" => None,
        "sambanova" => None,
        "cerebras" => None,
        "mistral" => match model {
            m if m.contains("large") => Some((2.0, 6.0, 1.0)),
            m if m.contains("small") => Some((0.2, 0.6, 1.0)),
            _ => Some((0.2, 0.6, 1.0)),
        },
        "openrouter" => None,
        "together" => Some((0.20, 0.20, 1.0)),
        "deepseek" => match model {
            "deepseek-v4-pro" => Some((1.74, 3.48, 0.1)),   // 75% launch discount applied
            _ => Some((0.14, 0.28, 0.1)),                    // v4-flash (cache hits 90% off)
        },
        "fireworks" => Some((0.20, 0.20, 1.0)),
        "xai" => Some((2.0, 10.0, 0.25)),  // Grok cache 75% off
        "nvidia" => None,
        "huggingface" => None,
        "cloudflare" => None,
        _ => None,
    }
}

/// Truncate tool output to stay within token budget.
/// Keeps the beginning (most useful) and end (error messages often at end).
pub(super) fn truncate_tool_output(output: &str, max_chars: usize) -> String {
    if output.len() <= max_chars {
        return output.to_string();
    }
    let head = max_chars * 3 / 4; // 75% from start
    let tail = max_chars / 4;     // 25% from end
    let head_end = output.char_indices().nth(head).map(|(i, _)| i).unwrap_or(head.min(output.len()));
    let tail_start = output.char_indices().rev().nth(tail).map(|(i, _)| i).unwrap_or(output.len().saturating_sub(tail));
    format!(
        "{}\n\n[...{} chars truncated...]\n\n{}",
        &output[..head_end],
        output.len() - head_end - (output.len() - tail_start),
        &output[tail_start..]
    )
}

/// Create a budget-trimmed COPY of the messages array for the API request.
/// The original array is never modified — this preserves prompt cache continuity.
/// Keeps the first message (system prompt) and the last 6 messages; replaces
/// older tool results and long assistant messages with summaries.
pub(super) fn budget_trimmed_messages(messages: &[Value]) -> Vec<Value> {
    if messages.len() <= 10 {
        return messages.to_vec();
    }
    let keep_end = 6;
    let trim_end = messages.len().saturating_sub(keep_end);

    messages.iter().enumerate().map(|(i, msg)| {
        if i == 0 || i >= trim_end {
            return msg.clone(); // system prompt + recent messages untouched
        }
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if role == "tool" {
            if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
                if content.len() > 100 {
                    let mut m = msg.clone();
                    m["content"] = json!(format!("[Output: {} chars]", content.len()));
                    return m;
                }
            }
        }
        if role == "assistant" && msg.get("tool_calls").is_none() {
            if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
                if content.len() > 300 {
                    let short: String = content.chars().take(200).collect();
                    let mut m = msg.clone();
                    m["content"] = json!(format!("{}...", short));
                    return m;
                }
            }
        }
        msg.clone()
    }).collect()
}

/// Summarize large tool output using the same provider API.
/// Makes a quick non-streaming API call to condense the output.
pub(super) fn summarize_tool_output(output: &str, tool_name: &str, url: &str, api_key: &str, model: &str) -> String {
    if output.len() <= TOOL_SUMMARIZE_THRESHOLD {
        return output.to_string();
    }
    // Take first 3000 chars + last 1000 chars for the summarization input
    let input = if output.len() > 4000 {
        let head: String = output.chars().take(3000).collect();
        let tail: String = output.chars().rev().take(1000).collect::<Vec<_>>().into_iter().rev().collect();
        format!("{}\n[...{} chars omitted...]\n{}", head, output.len() - 4000, tail)
    } else {
        output.to_string()
    };

    let body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": "Summarize the following tool output concisely. Keep all important information: errors, file paths, key results, numbers. Remove verbose/repetitive content. Output ONLY the summary, no preamble."},
            {"role": "user", "content": format!("Tool: {}\nOutput:\n{}", tool_name, input)}
        ],
        "max_tokens": 300,
        "temperature": 0.0,
        "stream": false
    });

    match ureq::post(url)
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send_string(&body.to_string())
    {
        Ok(resp) => {
            let text = resp.into_string().unwrap_or_default();
            if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                if let Some(content) = parsed["choices"][0]["message"]["content"].as_str() {
                    eprintln!("[OPENAI_COMPAT] Summarized tool output: {} chars → {} chars", output.len(), content.len());
                    return content.to_string();
                }
            }
            // Fallback to truncation if summarization fails
            truncate_tool_output(output, 2000)
        }
        Err(e) => {
            eprintln!("[OPENAI_COMPAT] Summarization failed: {e}, falling back to truncation");
            truncate_tool_output(output, 2000)
        }
    }
}

/// Get the subset of tool definitions suitable for cloud provider agentic loops.
/// Returns tools in OpenAI function-calling format, including MCP tools when available.
pub(super) fn get_agentic_tools(mcp: Option<&dyn llama_chat_tools::McpManagerOps>) -> Vec<Value> {
    use llama_chat_engine::jinja_templates::get_available_tools_openai;
    let mut tools = get_available_tools_openai();
    if let Some(mgr) = mcp {
        for td in mgr.get_tool_definitions() {
            tools.push(td.to_openai_function());
        }
    }
    tools
}

/// Execute a tool call using the full native tool dispatch system.
pub(super) fn execute_openai_tool(
    name: &str,
    arguments_json: &str,
    db: Option<&llama_chat_db::SharedDatabase>,
    mcp: Option<&dyn llama_chat_tools::McpManagerOps>,
) -> String {
    // Build the JSON format that dispatch_native_tool expects
    let args: Value = match serde_json::from_str(arguments_json) {
        Ok(v) => v,
        Err(e) => return format!("Error parsing tool arguments: {e}"),
    };

    // Map web_search → browser_search and web_fetch → browser_navigate + browser_get_text.
    // These tools are handled by dispatch_native_tool under browser_* names,
    // not as standalone tools (the engine layer handles them differently).
    // web_fetch: navigate browser then read content
    if name == "web_fetch" {
        let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("");
        if url.is_empty() {
            return "Error: 'url' is required for web_fetch".to_string();
        }
        let nav_json = json!({"name": "browser_navigate", "arguments": {"url": url}}).to_string();
        let ctx = crate::native_tools_bridge::make_dispatch_context();
        let _ = llama_chat_tools::dispatch_native_tool(&nav_json, true, mcp, db, &ctx);
        std::thread::sleep(std::time::Duration::from_millis(2000));
        let read_json = json!({"name": "browser_get_text", "arguments": {}}).to_string();
        return match llama_chat_tools::dispatch_native_tool(&read_json, true, mcp, db, &ctx) {
            Some(r) => r.text,
            None => "Failed to read page content".to_string(),
        };
    }

    let effective_name = match name {
        "web_search" => "browser_search",
        _ => name,
    };

    let tool_json = json!({
        "name": effective_name,
        "arguments": args
    }).to_string();

    // Use dispatch_native_tool for full tool support
    eprintln!("[OPENAI_TOOL] dispatch: name={name} effective={effective_name} json={}", &tool_json[..tool_json.len().min(200)]);
    let ctx = crate::native_tools_bridge::make_dispatch_context();
    match llama_chat_tools::dispatch_native_tool(
        &tool_json,
        true,
        mcp,
        db,
        &ctx,
    ) {
        Some(result) => result.text,
        None => {
            // dispatch_native_tool returns None for unknown tools
            format!("Unknown tool: {name}. Available tools: read_file, write_file, edit_file, execute_command, execute_python, list_directory, search_files, find_files, browser_search, browser_navigate, send_telegram")
        }
    }
}

/// Apply a single user-configured parameter to the request body.
///
/// Handles provider-specific translation: e.g. "thinking" → DeepSeek/Anthropic
/// thinking format, "reasoning_effort" → top-level field, etc.
pub(super) fn apply_user_param(provider_id: &str, body: &mut Value, key: &str, val: &Value) {
    match key {
        "thinking" => {
            let mode = val.as_str().unwrap_or("disabled");
            match provider_id {
                "deepseek" => {
                    // DeepSeek: remove temperature when thinking is enabled
                    if mode == "enabled" {
                        body.as_object_mut().map(|o| o.remove("temperature"));
                    }
                }
                "anthropic" => {
                    // Anthropic: set thinking.type based on mode
                    match mode {
                        "enabled" => {
                            let budget = body.get("budget_tokens")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(10000);
                            body["thinking"] = json!({"type": "enabled", "budget_tokens": budget});
                        }
                        "adaptive" => {
                            let budget = body.get("budget_tokens")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(10000);
                            body["thinking"] = json!({"type": "enabled", "budget_tokens": budget});
                        }
                        _ => {} // disabled — don't send thinking field
                    }
                }
                _ => {}
            }
        }
        "budget_tokens" => {
            // Handled by "thinking" param above — store for reference
            body["budget_tokens"] = val.clone();
        }
        "reasoning_effort" => {
            body["reasoning_effort"] = val.clone();
        }
        "response_format" => {
            let fmt = val.as_str().unwrap_or("text");
            if fmt != "text" {
                body["response_format"] = json!({"type": fmt});
            }
        }
        "max_completion_tokens" => {
            // OpenAI reasoning models use this instead of max_tokens
            body["max_completion_tokens"] = val.clone();
            body.as_object_mut().map(|o| o.remove("max_tokens"));
        }
        // Standard params: temperature, top_p, max_tokens, frequency_penalty, presence_penalty
        _ => {
            body[key] = val.clone();
        }
    }
}

/// Get provider-specific default parameters.
pub(super) fn provider_default_params(provider_id: &str) -> Value {
    match provider_id {
        "groq" => json!({"temperature": 0.6, "max_tokens": 4096}),
        "gemini" => json!({"temperature": 0.7, "max_tokens": 8192}),
        "sambanova" => json!({"temperature": 0.6, "max_tokens": 4096}),
        "cerebras" => json!({"temperature": 0.6, "max_tokens": 4096}),
        "mistral" => json!({"temperature": 0.7, "max_tokens": 4096}),
        "deepseek" => json!({"temperature": 0.6, "max_tokens": 32768}),  // thinking mode uses CoT tokens from this budget
        "openrouter" => json!({"temperature": 0.7, "max_tokens": 4096}),
        _ => json!({"temperature": 0.7, "max_tokens": 4096}),
    }
}

/// Resolve the model name. Uses the provided model or falls back to first preset default.
pub(super) fn resolve_model(provider_id: &str, model: Option<&str>) -> String {
    if let Some(m) = model.filter(|m| !m.is_empty()) {
        return m.to_string();
    }

    if let Some(preset) = super::openai_compat_types::PROVIDER_PRESETS.iter().find(|p| p.id == provider_id) {
        if let Some(first) = preset.models.first() {
            return (*first).to_string();
        }
    }

    "default".to_string()
}
