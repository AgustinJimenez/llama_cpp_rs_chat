//! OpenAI-compatible provider — works with any API that follows the OpenAI chat completions format.
//!
//! Supports: Groq, Gemini, SambaNova, Cerebras, OpenRouter, Together, Fireworks,
//! DeepSeek, local vLLM, Ollama, and any other OpenAI-compatible endpoint.
//!
//! Protocol: HTTP SSE streaming from `POST {base_url}/chat/completions`
//!
//! **Agentic loop**: When tool definitions are included, the provider will execute
//! tool calls locally and loop back to the API until the model produces a final text
//! response (or hits the 20-iteration safety limit).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::CliTokenData;

/// Maximum agentic loop iterations to prevent runaway tool-call chains.
const MAX_AGENTIC_ITERATIONS: usize = 20;

/// The set of tool names we expose to cloud providers.
/// Kept small to minimize request size — no desktop/screenshot/MCP tools.
const AGENTIC_TOOL_NAMES: &[&str] = &[
    "read_file",
    "write_file",
    "edit_file",
    "execute_command",
    "execute_python",
    "list_directory",
    "search_files",
    "find_files",
    "web_search",
    "web_fetch",
    "send_telegram",
];

// ── Streaming data structures ──────────────────────────────────────────────

/// OpenAI chat completion chunk (streaming response)
#[derive(Debug, Deserialize)]
struct ChatCompletionChunk {
    #[serde(default)]
    choices: Vec<ChunkChoice>,
    model: Option<String>,
    usage: Option<UsageInfo>,
}

#[derive(Debug, Deserialize)]
struct ChunkChoice {
    delta: Option<Delta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Delta {
    content: Option<String>,
    tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct ToolCallDelta {
    #[allow(dead_code)]
    index: Option<u32>,
    id: Option<String>,
    function: Option<FunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct FunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UsageInfo {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
}

/// A fully-accumulated tool call from streaming deltas.
#[derive(Debug, Clone)]
struct AccumulatedToolCall {
    id: String,
    name: String,
    arguments: String,
}

/// Result of streaming one SSE response from the API.
struct StreamResult {
    /// Text content produced by the model (may be empty if only tool calls).
    content: String,
    /// Accumulated tool calls (empty if the model produced only text).
    tool_calls: Vec<AccumulatedToolCall>,
    /// Model ID reported by the API.
    actual_model: Option<String>,
    /// Finish reason from the API.
    finish_reason: Option<String>,
    /// Token usage from this iteration.
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

// ── Provider presets ───────────────────────────────────────────────────────

/// Known provider presets with their base URLs and default models.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub base_url: &'static str,
    pub description: &'static str,
    pub models: &'static [&'static str],
    /// Environment variable name that may contain the API key.
    pub env_key: &'static str,
}

pub const PROVIDER_PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        id: "groq",
        name: "Groq",
        base_url: "https://api.groq.com/openai/v1",
        description: "Ultra-fast inference (Groq LPU)",
        models: &["llama-3.3-70b-versatile", "llama-3.1-8b-instant", "mixtral-8x7b-32768"],
        env_key: "GROQ_API_KEY",
    },
    ProviderPreset {
        id: "gemini",
        name: "Gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        description: "Google Gemini via OpenAI-compatible API",
        models: &["gemini-2.0-flash", "gemini-2.5-flash", "gemini-1.5-flash"],
        env_key: "GEMINI_API_KEY",
    },
    ProviderPreset {
        id: "sambanova",
        name: "SambaNova",
        base_url: "https://api.sambanova.ai/v1",
        description: "SambaNova Cloud inference",
        models: &["Meta-Llama-3.1-405B-Instruct", "Meta-Llama-3.1-70B-Instruct"],
        env_key: "SAMBANOVA_API_KEY",
    },
    ProviderPreset {
        id: "cerebras",
        name: "Cerebras",
        base_url: "https://api.cerebras.ai/v1",
        description: "Cerebras fast inference",
        models: &["llama-3.3-70b", "llama-3.1-8b"],
        env_key: "CEREBRAS_API_KEY",
    },
    ProviderPreset {
        id: "openrouter",
        name: "OpenRouter",
        base_url: "https://openrouter.ai/api/v1",
        description: "Access 100+ models via OpenRouter",
        models: &["auto"],
        env_key: "OPENROUTER_API_KEY",
    },
    ProviderPreset {
        id: "together",
        name: "Together AI",
        base_url: "https://api.together.xyz/v1",
        description: "Together AI inference",
        models: &["meta-llama/Llama-3.3-70B-Instruct-Turbo"],
        env_key: "TOGETHER_API_KEY",
    },
    ProviderPreset {
        id: "deepseek",
        name: "DeepSeek",
        base_url: "https://api.deepseek.com/v1",
        description: "DeepSeek AI models",
        models: &["deepseek-chat", "deepseek-reasoner"],
        env_key: "DEEPSEEK_API_KEY",
    },
    ProviderPreset {
        id: "custom_openai",
        name: "Custom OpenAI-Compatible",
        base_url: "",
        description: "Any OpenAI-compatible endpoint (vLLM, Ollama, etc.)",
        models: &[],
        env_key: "",
    },
];

/// Look up a provider preset by ID.
pub fn get_preset(provider_id: &str) -> Option<&'static ProviderPreset> {
    PROVIDER_PRESETS.iter().find(|p| p.id == provider_id)
}

/// Check if a provider ID is an OpenAI-compatible provider.
pub fn is_openai_compat(provider_id: &str) -> bool {
    get_preset(provider_id).is_some()
}

/// Resolve the API key for a provider. Checks config JSON blob first, then env var.
pub fn resolve_api_key(
    provider_id: &str,
    api_keys_json: Option<&str>,
) -> Option<String> {
    // 1. Check the JSON blob from config
    if let Some(json_str) = api_keys_json {
        if let Ok(map) = serde_json::from_str::<serde_json::Value>(json_str) {
            // Try provider_id directly, e.g. {"groq": {"api_key": "..."}}
            if let Some(provider_obj) = map.get(provider_id) {
                if let Some(key) = provider_obj.get("api_key").and_then(|v| v.as_str()) {
                    if !key.is_empty() {
                        return Some(key.to_string());
                    }
                }
                // Also handle flat format: {"groq": "sk-..."}
                if let Some(key) = provider_obj.as_str() {
                    if !key.is_empty() {
                        return Some(key.to_string());
                    }
                }
            }
        }
    }

    // 2. Check environment variable
    if let Some(preset) = get_preset(provider_id) {
        if !preset.env_key.is_empty() {
            if let Ok(key) = std::env::var(preset.env_key) {
                if !key.is_empty() {
                    return Some(key);
                }
            }
        }
    }

    None
}

/// Resolve the base URL for a provider. Uses preset default, overrideable from config.
pub fn resolve_base_url(
    provider_id: &str,
    api_keys_json: Option<&str>,
) -> Option<String> {
    // Check config for custom base_url override
    if let Some(json_str) = api_keys_json {
        if let Ok(map) = serde_json::from_str::<serde_json::Value>(json_str) {
            if let Some(provider_obj) = map.get(provider_id) {
                if let Some(url) = provider_obj.get("base_url").and_then(|v| v.as_str()) {
                    if !url.is_empty() {
                        return Some(url.to_string());
                    }
                }
            }
        }
    }

    // Fall back to preset default
    if let Some(preset) = get_preset(provider_id) {
        if !preset.base_url.is_empty() {
            return Some(preset.base_url.to_string());
        }
    }

    None
}

/// Resolve the model name. Uses the provided model or falls back to first preset default.
fn resolve_model(provider_id: &str, model: Option<&str>) -> String {
    if let Some(m) = model.filter(|m| !m.is_empty()) {
        return m.to_string();
    }

    if let Some(preset) = get_preset(provider_id) {
        if let Some(first) = preset.models.first() {
            return (*first).to_string();
        }
    }

    "default".to_string()
}

// ── Tool definitions (subset for agentic use) ─────────────────────────────

/// Get the subset of tool definitions suitable for cloud provider agentic loops.
/// Returns tools in OpenAI function-calling format.
fn get_agentic_tools() -> Vec<Value> {
    use crate::web::chat::jinja_templates::get_available_tools_openai;

    let all_tools = get_available_tools_openai();
    all_tools
        .into_iter()
        .filter(|tool| {
            if let Some(name) = tool
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
            {
                AGENTIC_TOOL_NAMES.contains(&name)
            } else {
                false
            }
        })
        .collect()
}

// ── Local tool execution ───────────────────────────────────────────────────

/// Execute a tool call locally and return the result as a string.
///
/// This is a self-contained executor that handles the essential tools without
/// needing the full `dispatch_native_tool` machinery (which requires model/backend refs).
fn execute_openai_tool(name: &str, arguments_json: &str) -> String {
    let args: Value = match serde_json::from_str(arguments_json) {
        Ok(v) => v,
        Err(e) => return format!("Error parsing tool arguments: {e}"),
    };

    match name {
        "read_file" => {
            let path = match args.get("path").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return "Error: 'path' argument is required".to_string(),
            };
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    // Truncate large files
                    if content.len() > 100_000 {
                        format!("{}\n\n[... truncated at 100KB, total {} bytes]", &content[..100_000], content.len())
                    } else {
                        content
                    }
                }
                Err(e) => format!("Error reading file: {e}"),
            }
        }

        "write_file" => {
            let path = match args.get("path").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return "Error: 'path' argument is required".to_string(),
            };
            let content = match args.get("content").and_then(|v| v.as_str()) {
                Some(c) => c,
                None => return "Error: 'content' argument is required".to_string(),
            };
            // Create parent directories
            if let Some(parent) = std::path::Path::new(path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::write(path, content) {
                Ok(_) => format!("Successfully wrote {} bytes to {}", content.len(), path),
                Err(e) => format!("Error writing file: {e}"),
            }
        }

        "edit_file" => {
            let path = match args.get("path").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return "Error: 'path' argument is required".to_string(),
            };
            let old_string = match args.get("old_string").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => return "Error: 'old_string' argument is required".to_string(),
            };
            let new_string = match args.get("new_string").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => return "Error: 'new_string' argument is required".to_string(),
            };
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => return format!("Error reading file: {e}"),
            };
            let count = content.matches(old_string).count();
            if count == 0 {
                return format!("Error: old_string not found in {path}");
            }
            if count > 1 {
                return format!("Error: old_string found {count} times in {path} (must be unique)");
            }
            let new_content = content.replacen(old_string, new_string, 1);
            match std::fs::write(path, &new_content) {
                Ok(_) => format!("Successfully edited {path}"),
                Err(e) => format!("Error writing file: {e}"),
            }
        }

        "execute_command" => {
            let command = match args.get("command").and_then(|v| v.as_str()) {
                Some(c) => c,
                None => return "Error: 'command' argument is required".to_string(),
            };
            if command.is_empty() {
                return "Error: 'command' argument must not be empty".to_string();
            }
            crate::web::command::execute_command(command)
        }

        "execute_python" => {
            let code = match args.get("code").and_then(|v| v.as_str()) {
                Some(c) => c,
                None => return "Error: 'code' argument is required".to_string(),
            };
            let output = crate::web::utils::silent_command("python")
                .args(["-c", code])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output();
            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    let mut result = String::new();
                    if !stdout.is_empty() {
                        result.push_str(&stdout);
                    }
                    if !stderr.is_empty() {
                        if !result.is_empty() {
                            result.push('\n');
                        }
                        result.push_str("[stderr] ");
                        result.push_str(&stderr);
                    }
                    if result.is_empty() {
                        format!("Python exited with code {}", out.status.code().unwrap_or(-1))
                    } else {
                        result
                    }
                }
                Err(e) => format!("Error running python: {e}"),
            }
        }

        "list_directory" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            match std::fs::read_dir(path) {
                Ok(entries) => {
                    let mut items: Vec<String> = Vec::new();
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                        items.push(if is_dir { format!("{name}/") } else { name });
                    }
                    items.sort();
                    if items.is_empty() {
                        format!("Directory {path} is empty")
                    } else {
                        items.join("\n")
                    }
                }
                Err(e) => format!("Error listing directory: {e}"),
            }
        }

        "search_files" => {
            // Delegate to the native tool via a simple JSON dispatch
            let json_str = json!({"name": "search_files", "arguments": args}).to_string();
            match crate::web::native_tools::dispatch_native_tool(
                &json_str, None, None, false,
                &crate::web::browser::BrowserBackend::Chrome, None, None,
            ) {
                Some(result) => result.text,
                None => "Error: search_files dispatch failed".to_string(),
            }
        }

        "find_files" => {
            let json_str = json!({"name": "find_files", "arguments": args}).to_string();
            match crate::web::native_tools::dispatch_native_tool(
                &json_str, None, None, false,
                &crate::web::browser::BrowserBackend::Chrome, None, None,
            ) {
                Some(result) => result.text,
                None => "Error: find_files dispatch failed".to_string(),
            }
        }

        "web_search" => {
            let json_str = json!({"name": "web_search", "arguments": args}).to_string();
            match crate::web::native_tools::dispatch_native_tool(
                &json_str, None, None, false,
                &crate::web::browser::BrowserBackend::Chrome, None, None,
            ) {
                Some(result) => result.text,
                None => "Error: web_search dispatch failed".to_string(),
            }
        }

        "web_fetch" => {
            let json_str = json!({"name": "web_fetch", "arguments": args}).to_string();
            match crate::web::native_tools::dispatch_native_tool(
                &json_str, None, None, true,
                &crate::web::browser::BrowserBackend::Chrome, None, None,
            ) {
                Some(result) => result.text,
                None => "Error: web_fetch dispatch failed".to_string(),
            }
        }

        "send_telegram" => {
            let json_str = json!({"name": "send_telegram", "arguments": args}).to_string();
            match crate::web::native_tools::dispatch_native_tool(
                &json_str, None, None, false,
                &crate::web::browser::BrowserBackend::Chrome, None, None,
            ) {
                Some(result) => result.text,
                None => "Error: send_telegram dispatch failed".to_string(),
            }
        }

        _ => format!("Unknown tool: {name}"),
    }
}

// ── SSE streaming helper ───────────────────────────────────────────────────

/// Stream one SSE response from the API, sending text tokens to `tx` as they arrive.
/// Returns the accumulated result (content, tool calls, usage info).
fn stream_sse_response(
    url: &str,
    api_key: &str,
    body: &Value,
    tx: &mpsc::UnboundedSender<CliTokenData>,
    model_hint: &Option<String>,
) -> Result<StreamResult, String> {
    let body_str = serde_json::to_string(body)
        .map_err(|e| format!("Failed to serialize request: {e}"))?;

    let resp = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(30))
        .build()
        .post(url)
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Accept", "text/event-stream")
        .send_string(&body_str);

    let reader = match resp {
        Ok(r) => r.into_reader(),
        Err(e) => {
            let error_msg = match e {
                ureq::Error::Status(code, resp) => {
                    let body = resp.into_string().unwrap_or_default();
                    format!("HTTP {code}: {body}")
                }
                other => format!("Request failed: {other}"),
            };
            return Err(error_msg);
        }
    };

    let buf_reader = std::io::BufReader::new(reader);
    use std::io::BufRead;

    let mut actual_model: Option<String> = model_hint.clone();
    let mut input_tokens: Option<u64> = None;
    let mut output_tokens: Option<u64> = None;
    let mut finish_reason: Option<String> = None;
    let mut content = String::new();

    // Track streaming tool calls — supports multiple parallel tool calls
    // Each tool call is identified by its index in the delta stream.
    let mut tool_calls_map: std::collections::BTreeMap<u32, AccumulatedToolCall> =
        std::collections::BTreeMap::new();

    for line_result in buf_reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[OPENAI_COMPAT] Read error: {e}");
                break;
            }
        };

        if !line.starts_with("data: ") {
            continue;
        }

        let data = &line[6..];
        if data == "[DONE]" {
            break;
        }

        let chunk: ChatCompletionChunk = match serde_json::from_str(data) {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "[OPENAI_COMPAT] Parse error: {e} | data: {}",
                    &data[..data.len().min(200)]
                );
                continue;
            }
        };

        // Track model ID
        if actual_model.is_none() {
            if let Some(ref m) = chunk.model {
                actual_model = Some(m.clone());
            }
        }

        // Track usage (often in the last chunk)
        if let Some(ref usage) = chunk.usage {
            if let Some(pt) = usage.prompt_tokens {
                input_tokens = Some(pt);
            }
            if let Some(ct) = usage.completion_tokens {
                output_tokens = Some(ct);
            }
        }

        for choice in &chunk.choices {
            if let Some(ref reason) = choice.finish_reason {
                finish_reason = Some(reason.clone());
            }

            if let Some(ref delta) = choice.delta {
                // Stream text content to frontend
                if let Some(ref text) = delta.content {
                    if !text.is_empty() {
                        content.push_str(text);
                        let _ = tx.send(CliTokenData {
                            token: text.clone(),
                            is_done: false,
                            session_id: None,
                            stop_reason: None,
                            cost_usd: None,
                            duration_ms: None,
                            model_id: actual_model.clone(),
                            input_tokens: None,
                            output_tokens: None,
                        });
                    }
                }

                // Accumulate tool calls by index
                if let Some(ref tcs) = delta.tool_calls {
                    for tc in tcs {
                        let idx = tc.index.unwrap_or(0);
                        let entry = tool_calls_map.entry(idx).or_insert_with(|| {
                            AccumulatedToolCall {
                                id: String::new(),
                                name: String::new(),
                                arguments: String::new(),
                            }
                        });
                        if let Some(ref id) = tc.id {
                            entry.id = id.clone();
                        }
                        if let Some(ref func) = tc.function {
                            if let Some(ref name) = func.name {
                                entry.name = name.clone();
                            }
                            if let Some(ref args) = func.arguments {
                                entry.arguments.push_str(args);
                            }
                        }
                    }
                }
            }
        }
    }

    let tool_calls: Vec<AccumulatedToolCall> = tool_calls_map.into_values().collect();

    Ok(StreamResult {
        content,
        tool_calls,
        actual_model,
        finish_reason,
        input_tokens,
        output_tokens,
    })
}

// ── Main generate function with agentic loop ──────────────────────────────

/// Generate a response using an OpenAI-compatible API.
///
/// Streams SSE tokens from `POST {base_url}/chat/completions` and converts them
/// to `CliTokenData` events on the returned channel.
///
/// When the model returns tool calls, they are executed locally and the results
/// are fed back into the conversation for another API round-trip (up to 20 iterations).
pub async fn generate(
    provider_id: &str,
    prompt: &str,
    model: Option<&str>,
    base_url: &str,
    api_key: &str,
) -> Result<mpsc::UnboundedReceiver<CliTokenData>, String> {
    let (tx, rx) = mpsc::unbounded_channel();
    let model_name = resolve_model(provider_id, model);
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    eprintln!(
        "[OPENAI_COMPAT] generate() provider={} model={} url={}",
        provider_id, model_name, url
    );

    let api_key_owned = api_key.to_string();
    let provider_id_owned = provider_id.to_string();
    let model_name_clone = model_name.clone();
    let prompt_owned = prompt.to_string();

    // Use ureq in a blocking task for the streaming HTTP request + agentic loop
    tokio::task::spawn_blocking(move || {
        let start = std::time::Instant::now();

        // Get tool definitions for the agentic loop
        let tools = get_agentic_tools();
        let has_tools = !tools.is_empty();

        eprintln!(
            "[OPENAI_COMPAT] Including {} tool definitions in request",
            tools.len()
        );

        // Build initial messages array
        let mut messages: Vec<Value> = vec![
            json!({"role": "user", "content": prompt_owned}),
        ];

        // Track total tokens across all iterations
        let mut total_input_tokens: u64 = 0;
        let mut total_output_tokens: u64 = 0;
        let mut actual_model: Option<String> = None;
        let mut final_stop_reason = "end_turn".to_string();

        for iteration in 0..MAX_AGENTIC_ITERATIONS {
            eprintln!(
                "[OPENAI_COMPAT] Agentic iteration {}/{} with {} messages",
                iteration + 1,
                MAX_AGENTIC_ITERATIONS,
                messages.len()
            );

            // Build request body
            let mut body = json!({
                "model": model_name_clone,
                "messages": messages,
                "stream": true,
                "stream_options": {"include_usage": true},
            });

            // Include tools only if we have them
            if has_tools {
                body["tools"] = json!(tools);
            }

            // Make the API call
            let result = match stream_sse_response(
                &url,
                &api_key_owned,
                &body,
                &tx,
                &actual_model,
            ) {
                Ok(r) => r,
                Err(error_msg) => {
                    eprintln!("[OPENAI_COMPAT] Error: {error_msg}");
                    let _ = tx.send(CliTokenData {
                        token: format!("\n**Error:** {error_msg}"),
                        is_done: false,
                        session_id: None,
                        stop_reason: None,
                        cost_usd: None,
                        duration_ms: None,
                        model_id: Some(model_name_clone.clone()),
                        input_tokens: None,
                        output_tokens: None,
                    });
                    final_stop_reason = "error".to_string();
                    break;
                }
            };

            // Update tracking
            if result.actual_model.is_some() {
                actual_model = result.actual_model;
            }
            if let Some(it) = result.input_tokens {
                total_input_tokens += it;
            }
            if let Some(ot) = result.output_tokens {
                total_output_tokens += ot;
            }
            if let Some(ref reason) = result.finish_reason {
                final_stop_reason = reason.clone();
            }

            // If no tool calls, we're done — the text was already streamed
            if result.tool_calls.is_empty() {
                eprintln!(
                    "[OPENAI_COMPAT] No tool calls, finishing after iteration {}",
                    iteration + 1
                );
                break;
            }

            // --- Tool calls detected: execute them and loop ---
            eprintln!(
                "[OPENAI_COMPAT] {} tool call(s) to execute",
                result.tool_calls.len()
            );

            // Build the assistant message with tool_calls for the conversation
            let tc_json: Vec<Value> = result
                .tool_calls
                .iter()
                .map(|tc| {
                    json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": tc.arguments,
                        }
                    })
                })
                .collect();

            // Add assistant message (with content if any, plus tool_calls)
            let assistant_msg = if result.content.is_empty() {
                json!({
                    "role": "assistant",
                    "content": null,
                    "tool_calls": tc_json,
                })
            } else {
                json!({
                    "role": "assistant",
                    "content": result.content,
                    "tool_calls": tc_json,
                })
            };
            messages.push(assistant_msg);

            // Send tool call display widgets to frontend, execute each, and add results
            for tc in &result.tool_calls {
                // Display the tool call to the frontend
                let display = format!(
                    "\n<tool_call>{{\"name\": \"{}\", \"arguments\": {}}}</tool_call>\n",
                    tc.name,
                    if tc.arguments.is_empty() {
                        "{}".to_string()
                    } else {
                        tc.arguments.clone()
                    }
                );
                let _ = tx.send(CliTokenData {
                    token: display,
                    is_done: false,
                    session_id: None,
                    stop_reason: None,
                    cost_usd: None,
                    duration_ms: None,
                    model_id: actual_model.clone(),
                    input_tokens: None,
                    output_tokens: None,
                });

                // Execute the tool
                eprintln!(
                    "[OPENAI_COMPAT] Executing tool: {} args={}",
                    tc.name,
                    &tc.arguments[..tc.arguments.len().min(200)]
                );
                let tool_result = execute_openai_tool(&tc.name, &tc.arguments);

                // Truncate very large results to avoid blowing up context
                let tool_result_truncated = if tool_result.len() > 50_000 {
                    format!(
                        "{}\n\n[... truncated at 50KB, total {} bytes]",
                        &tool_result[..50_000],
                        tool_result.len()
                    )
                } else {
                    tool_result
                };

                // Display the tool response to the frontend
                let response_display = format!(
                    "\n<tool_response>{}</tool_response>\n",
                    &tool_result_truncated[..tool_result_truncated.len().min(2000)]
                );
                let _ = tx.send(CliTokenData {
                    token: response_display,
                    is_done: false,
                    session_id: None,
                    stop_reason: None,
                    cost_usd: None,
                    duration_ms: None,
                    model_id: actual_model.clone(),
                    input_tokens: None,
                    output_tokens: None,
                });

                // Add tool result to messages for the next API call
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": tc.id,
                    "content": tool_result_truncated,
                }));
            }
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        eprintln!(
            "[OPENAI_COMPAT] Done: provider={} model={:?} stop={} duration={}ms tokens={}in/{}out",
            provider_id_owned,
            actual_model,
            final_stop_reason,
            duration_ms,
            total_input_tokens,
            total_output_tokens,
        );

        // Send done event
        let _ = tx.send(CliTokenData {
            token: String::new(),
            is_done: true,
            session_id: None,
            stop_reason: Some(final_stop_reason),
            cost_usd: None,
            duration_ms: Some(duration_ms),
            model_id: actual_model,
            input_tokens: if total_input_tokens > 0 {
                Some(total_input_tokens)
            } else {
                None
            },
            output_tokens: if total_output_tokens > 0 {
                Some(total_output_tokens)
            } else {
                None
            },
        });
    });

    Ok(rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_lookup() {
        assert!(get_preset("groq").is_some());
        assert!(get_preset("gemini").is_some());
        assert!(get_preset("unknown_xyz").is_none());
    }

    #[test]
    fn test_is_openai_compat() {
        assert!(is_openai_compat("groq"));
        assert!(is_openai_compat("cerebras"));
        assert!(!is_openai_compat("claude_code"));
        assert!(!is_openai_compat("local"));
    }

    #[test]
    fn test_resolve_api_key_from_json() {
        let json = r#"{"groq": {"api_key": "gsk_test123"}, "gemini": "gem_key456"}"#;
        assert_eq!(resolve_api_key("groq", Some(json)), Some("gsk_test123".to_string()));
        assert_eq!(resolve_api_key("gemini", Some(json)), Some("gem_key456".to_string()));
        assert_eq!(resolve_api_key("cerebras", Some(json)), None);
    }

    #[test]
    fn test_resolve_base_url() {
        assert_eq!(
            resolve_base_url("groq", None),
            Some("https://api.groq.com/openai/v1".to_string())
        );

        // Custom override
        let json = r#"{"groq": {"base_url": "http://localhost:8080/v1"}}"#;
        assert_eq!(
            resolve_base_url("groq", Some(json)),
            Some("http://localhost:8080/v1".to_string())
        );
    }

    #[test]
    fn test_resolve_model() {
        assert_eq!(resolve_model("groq", Some("my-model")), "my-model");
        assert_eq!(resolve_model("groq", None), "llama-3.3-70b-versatile");
        assert_eq!(resolve_model("unknown", None), "default");
    }

    #[test]
    fn test_get_agentic_tools() {
        let tools = get_agentic_tools();
        assert!(!tools.is_empty());
        assert!(tools.len() <= AGENTIC_TOOL_NAMES.len());

        // Verify all returned tools are in our allowlist
        for tool in &tools {
            let name = tool["function"]["name"].as_str().unwrap();
            assert!(
                AGENTIC_TOOL_NAMES.contains(&name),
                "Unexpected tool: {name}"
            );
        }
    }

    #[test]
    fn test_execute_openai_tool_read_file() {
        let dir = std::env::temp_dir();
        let path = dir.join("openai_compat_test_read.txt");
        std::fs::write(&path, "hello from test").unwrap();

        let args = json!({"path": path.to_string_lossy()}).to_string();
        let result = execute_openai_tool("read_file", &args);
        assert!(result.contains("hello from test"));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_execute_openai_tool_write_file() {
        let dir = std::env::temp_dir();
        let path = dir.join("openai_compat_test_write.txt");

        let args = json!({"path": path.to_string_lossy(), "content": "written by test"}).to_string();
        let result = execute_openai_tool("write_file", &args);
        assert!(result.contains("Successfully wrote"));

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "written by test");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_execute_openai_tool_edit_file() {
        let dir = std::env::temp_dir();
        let path = dir.join("openai_compat_test_edit.txt");
        std::fs::write(&path, "foo bar baz").unwrap();

        let args =
            json!({"path": path.to_string_lossy(), "old_string": "bar", "new_string": "qux"})
                .to_string();
        let result = execute_openai_tool("edit_file", &args);
        assert!(result.contains("Successfully edited"));

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "foo qux baz");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_execute_openai_tool_list_directory() {
        let args = json!({"path": "."}).to_string();
        let result = execute_openai_tool("list_directory", &args);
        // Should return something (at least Cargo.toml or src/)
        assert!(!result.is_empty());
    }

    #[test]
    fn test_execute_openai_tool_unknown() {
        let result = execute_openai_tool("nonexistent_tool", "{}");
        assert!(result.contains("Unknown tool"));
    }
}
