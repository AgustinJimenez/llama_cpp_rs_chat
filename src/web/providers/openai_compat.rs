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
use crate::web::database::SharedDatabase;

/// Maximum agentic loop iterations to prevent runaway tool-call chains.
const MAX_AGENTIC_ITERATIONS: usize = 20;

// ─── Incremental DB persistence ─────────────────────────────────────────

/// Ensure a conversation row exists in the DB.
fn ensure_conversation(db: &SharedDatabase, conv_id: &str, provider_id: &str) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let conn = db.connection();
    let _ = conn.execute(
        "INSERT OR IGNORE INTO conversations (id, created_at, updated_at, provider_id) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![conv_id, now, now, provider_id],
    );
}

/// Get the next sequence_order for a conversation.
fn next_sequence(db: &SharedDatabase, conv_id: &str) -> i32 {
    db.get_messages(conv_id)
        .map(|msgs| msgs.len() as i32 + 1)
        .unwrap_or(1)
}

/// Save a single message to the DB immediately.
fn save_message_now(db: &SharedDatabase, conv_id: &str, role: &str, content: &str) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let seq = next_sequence(db, conv_id);
    let _ = db.insert_message(conv_id, role, content, now, seq);
}

/// System prompt for cloud provider agentic loops.
fn get_cloud_system_prompt() -> &'static str {
    r#"You are an AI assistant with access to tools for file operations, command execution, web search, and more. Follow these guidelines:

1. INVESTIGATE before acting: Check existing files, directory structure, and installed tools before creating or modifying anything.
2. Use web_search when you need current information, documentation, or to find solutions to errors.
3. For execute_command: ALWAYS set the "background" field. Use background=true ONLY for servers/daemons, false for everything else.
4. When creating projects: use the standard tooling (django-admin, npm init, cargo init, etc.) rather than writing every file manually.
5. After making changes, verify they work (run tests, check syntax, start the server briefly).
6. If a command fails, read the error carefully and fix the issue rather than retrying the same command.
7. Keep responses concise. Show what you did and the result, not lengthy explanations."#
}

/// Conservative input token limit for context management.
const MAX_INPUT_TOKENS: u64 = 100_000;

/// Approximate cost per 1M tokens (input, output) for known providers.
/// Returns (input_cost_per_1m, output_cost_per_1m) or None for free/unknown.
fn provider_cost_per_million(provider_id: &str, model: &str) -> Option<(f64, f64)> {
    match provider_id {
        "groq" => Some((0.05, 0.10)),
        "gemini" => None,
        "sambanova" => None,
        "cerebras" => None,
        "mistral" => match model {
            m if m.contains("large") => Some((2.0, 6.0)),
            m if m.contains("small") => Some((0.2, 0.6)),
            _ => Some((0.2, 0.6)),
        },
        "openrouter" => None,
        "together" => Some((0.20, 0.20)),
        "deepseek" => match model {
            "deepseek-v4-pro" => Some((1.74, 3.48)),
            _ => Some((0.14, 0.28)),
        },
        "fireworks" => Some((0.20, 0.20)),
        "xai" => Some((2.0, 10.0)),
        "nvidia" => None,
        "huggingface" => None,
        "cloudflare" => None,
        _ => None,
    }
}

/// Truncate tool output to stay within token budget.
/// Keeps the beginning (most useful) and end (error messages often at end).
fn truncate_tool_output(output: &str, max_chars: usize) -> String {
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

/// Trim older tool result messages to reduce context usage.
/// Keeps the first message (user prompt) and the last 6 messages, replaces
/// older tool results with summaries.
fn trim_old_tool_results(messages: &mut Vec<Value>) {
    if messages.len() <= 8 {
        return;
    }
    let keep_end = 6;
    let trim_end = messages.len() - keep_end;
    for i in 1..trim_end {
        if messages[i].get("role").and_then(|r| r.as_str()) == Some("tool") {
            if let Some(content) = messages[i].get("content").and_then(|c| c.as_str()) {
                if content.len() > 200 {
                    messages[i]["content"] = json!(format!("[Previous tool output truncated — {} chars]", content.len()));
                }
            }
        }
    }
}

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
        models: &["gemini-2.5-flash", "gemini-2.0-flash"],
        env_key: "GEMINI_API_KEY",
    },
    ProviderPreset {
        id: "sambanova",
        name: "SambaNova",
        base_url: "https://api.sambanova.ai/v1",
        description: "SambaNova Cloud inference",
        models: &["DeepSeek-V3.2", "Meta-Llama-3.3-70B-Instruct", "Qwen3-235B", "Llama-4-Maverick-17B-128E-Instruct"],
        env_key: "SAMBANOVA_API_KEY",
    },
    ProviderPreset {
        id: "cerebras",
        name: "Cerebras",
        base_url: "https://api.cerebras.ai/v1",
        description: "Cerebras fast inference",
        models: &["qwen-3-235b-a22b-instruct-2507", "llama3.1-8b"],
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
        base_url: "https://api.deepseek.com",
        description: "DeepSeek AI models",
        models: &["deepseek-v4-flash", "deepseek-v4-pro"],
        env_key: "DEEPSEEK_API_KEY",
    },
    ProviderPreset {
        id: "mistral",
        name: "Mistral AI",
        base_url: "https://api.mistral.ai/v1",
        description: "Mistral AI models with tool calling",
        models: &["mistral-small-latest", "mistral-large-latest", "codestral-latest", "open-mistral-nemo"],
        env_key: "MISTRAL_API_KEY",
    },
    ProviderPreset {
        id: "fireworks",
        name: "Fireworks AI",
        base_url: "https://api.fireworks.ai/inference/v1",
        description: "Fast inference on open-weight models",
        models: &["accounts/fireworks/models/llama-v3p3-70b-instruct", "accounts/fireworks/models/qwen2p5-72b-instruct"],
        env_key: "FIREWORKS_API_KEY",
    },
    ProviderPreset {
        id: "xai",
        name: "xAI (Grok)",
        base_url: "https://api.x.ai/v1",
        description: "xAI Grok models with tool calling",
        models: &["grok-2", "grok-2-mini"],
        env_key: "XAI_API_KEY",
    },
    ProviderPreset {
        id: "nvidia",
        name: "NVIDIA NIM",
        base_url: "https://integrate.api.nvidia.com/v1",
        description: "NVIDIA hosted inference (free daily limit)",
        models: &["meta/llama-3.1-70b-instruct", "mistralai/mistral-large-2-instruct"],
        env_key: "NVIDIA_API_KEY",
    },
    ProviderPreset {
        id: "huggingface",
        name: "Hugging Face",
        base_url: "https://router.huggingface.co/v1",
        description: "Hugging Face Inference API (free tier)",
        models: &["meta-llama/Llama-3.1-70B-Instruct", "mistralai/Mistral-7B-Instruct-v0.3"],
        env_key: "HF_TOKEN",
    },
    ProviderPreset {
        id: "cloudflare",
        name: "Cloudflare Workers AI",
        base_url: "",
        description: "Cloudflare Workers AI (free 10K neurons/day)",
        models: &["@cf/meta/llama-3.1-8b-instruct", "@cf/mistral/mistral-7b-instruct-v0.2"],
        env_key: "CLOUDFLARE_API_TOKEN",
    },
    ProviderPreset {
        id: "glm",
        name: "GLM (Zhipu AI)",
        base_url: "https://api.z.ai/api/paas/v4",
        description: "GLM models by Zhipu AI ($3-15/mo coding plan)",
        models: &["glm-5", "glm-4.7", "glm-4.6", "glm-4.5-air"],
        env_key: "GLM_API_KEY",
    },
    ProviderPreset {
        id: "kimi",
        name: "Kimi (Moonshot)",
        base_url: "https://api.moonshot.cn/v1",
        description: "Kimi K2.5 by Moonshot AI (auto context caching)",
        models: &["kimi-k2.5", "moonshot-v1-auto"],
        env_key: "KIMI_API_KEY",
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

/// Query available models from a provider's /v1/models endpoint.
/// Returns model IDs or falls back to preset defaults on error.
pub fn fetch_models(provider_id: &str, base_url: &str, api_key: &str) -> Vec<String> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let resp = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(5))
        .timeout_read(std::time::Duration::from_secs(10))
        .build()
        .get(&url)
        .set("Authorization", &format!("Bearer {api_key}"))
        .call();

    match resp {
        Ok(r) => {
            if let Ok(body) = r.into_string() {
                if let Ok(json) = serde_json::from_str::<Value>(&body) {
                    if let Some(data) = json.get("data").and_then(|d| d.as_array()) {
                        let mut models: Vec<String> = data
                            .iter()
                            .filter_map(|m| m.get("id").and_then(|id| id.as_str()).map(|s| s.to_string()))
                            .collect();
                        models.sort();
                        if !models.is_empty() {
                            return models;
                        }
                    }
                }
            }
            // Fall back to preset
            get_preset(provider_id)
                .map(|p| p.models.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default()
        }
        Err(_) => {
            get_preset(provider_id)
                .map(|p| p.models.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default()
        }
    }
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

/// Resolve a field from a user-defined custom provider entry in api_keys_json.
pub fn resolve_custom_field(
    provider_id: &str,
    field: &str,
    api_keys_json: Option<&str>,
) -> Option<String> {
    let json_str = api_keys_json?;
    let map: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let val = map.get(provider_id)?.get(field)?.as_str()?;
    if val.is_empty() { None } else { Some(val.to_string()) }
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

/// Execute a tool call using the full native tool dispatch system.
fn execute_openai_tool(name: &str, arguments_json: &str) -> String {
    // Build the JSON format that dispatch_native_tool expects
    let args: Value = match serde_json::from_str(arguments_json) {
        Ok(v) => v,
        Err(e) => return format!("Error parsing tool arguments: {e}"),
    };

    let tool_json = json!({
        "name": name,
        "arguments": args
    }).to_string();

    // Use dispatch_native_tool for full tool support
    let ctx = crate::web::native_tools::make_dispatch_context();
    match crate::web::native_tools::dispatch_native_tool(
        &tool_json,
        true,
        None, // mcp_manager
        None, // db
        &ctx,
    ) {
        Some(result) => result.text,
        None => {
            // dispatch_native_tool returns None for unknown tools
            format!("Unknown tool: {name}. Available tools: read_file, write_file, edit_file, execute_command, execute_python, list_directory, search_files, find_files, web_search, web_fetch, send_telegram")
        }
    }
}

/// Generate a conversation title using the remote provider (non-streaming, cheap).
fn generate_title_via_provider(base_url: &str, api_key: &str, model: &str, user_message: &str, assistant_snippet: &str) -> Option<String> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let snippet = if assistant_snippet.len() > 500 { &assistant_snippet[..500] } else { assistant_snippet };
    let body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": "Generate a concise title (3-6 words) for this conversation. Respond with ONLY the title, no quotes, no punctuation, no explanation."},
            {"role": "user", "content": format!("User: {}\nAssistant: {}", &user_message[..user_message.len().min(300)], snippet)},
        ],
        "max_tokens": 20,
        "temperature": 0.3,
        "stream": false,
    });
    let resp = ureq::post(&url)
        .header("Authorization", &format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .send_json(&body)
        .ok()?;
    let json: Value = resp.into_json().ok()?;
    let title = json["choices"][0]["message"]["content"].as_str()?
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string();
    if title.is_empty() || title.len() > 100 { None } else { Some(title) }
}

/// Get provider-specific default parameters.
fn provider_default_params(provider_id: &str) -> Value {
    match provider_id {
        "groq" => json!({"temperature": 0.6, "max_tokens": 4096}),
        "gemini" => json!({"temperature": 0.7, "max_tokens": 8192}),
        "sambanova" => json!({"temperature": 0.6, "max_tokens": 4096}),
        "cerebras" => json!({"temperature": 0.6, "max_tokens": 4096}),
        "mistral" => json!({"temperature": 0.7, "max_tokens": 4096}),
        "deepseek" => json!({"temperature": 0.6, "max_tokens": 8192}),
        "openrouter" => json!({"temperature": 0.7, "max_tokens": 4096}),
        _ => json!({"temperature": 0.7, "max_tokens": 4096}),
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

    // TLS retry: on transport/connection errors (NOT HTTP 4xx/5xx), retry once after 1s
    let resp = {
        #[allow(unused_assignments)]
        let mut last_err = String::new();
        let mut attempts = 0;
        loop {
            attempts += 1;
            match ureq::AgentBuilder::new()
                .timeout_connect(std::time::Duration::from_secs(30))
                .build()
                .post(url)
                .set("Content-Type", "application/json")
                .set("Authorization", &format!("Bearer {api_key}"))
                .set("Accept", "text/event-stream")
                .send_string(&body_str)
            {
                Ok(r) => break Ok(r),
                Err(ureq::Error::Status(code, resp)) => {
                    // HTTP error — don't retry
                    let body = resp.into_string().unwrap_or_default();
                    break Err(format!("HTTP {code}: {body}"));
                }
                Err(other) => {
                    last_err = format!("{other}");
                    if attempts >= 2 {
                        break Err(format!("Request failed: {last_err}"));
                    }
                    eprintln!("[OPENAI_COMPAT] Connection error (attempt {}/2): {}, retrying in 1s...", attempts, last_err);
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
        }
    };

    let reader = match resp {
        Ok(r) => r.into_reader(),
        Err(error_msg) => return Err(error_msg),
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
    conversation_id: Option<&str>,
    db: Option<&crate::web::database::SharedDatabase>,
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
    let conv_id_owned = conversation_id.map(|s| s.to_string());
    let db_owned = db.cloned();

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

        // Build initial messages array with system prompt
        let mut messages: Vec<Value> = vec![
            json!({"role": "system", "content": get_cloud_system_prompt()}),
        ];

        // Add prior conversation turns from DB
        if let (Some(conv_id), Some(ref db)) = (&conv_id_owned, &db_owned) {
            if let Ok(msgs) = db.get_messages(conv_id) {
                for m in &msgs {
                    if m.compacted || m.role == "system" { continue; }
                    // Reconstruct tool_calls messages from stored format
                    if m.role == "tool" {
                        // Tool results stored as "tool_call_id\n\ncontent"
                        let (tc_id, content) = m.content.split_once("\n\n")
                            .unwrap_or(("unknown", &m.content));
                        messages.push(json!({"role": "tool", "tool_call_id": tc_id, "content": content}));
                    } else if m.role == "assistant" && m.content.contains("\"tool_calls\":") && m.content.starts_with("{") {
                        // Assistant message with tool_calls stored as JSON
                        if let Ok(parsed) = serde_json::from_str::<Value>(&m.content) {
                            let mut msg = json!({"role": "assistant"});
                            if let Some(tc) = parsed.get("tool_calls") {
                                msg["tool_calls"] = tc.clone();
                            }
                            if let Some(c) = parsed.get("content") {
                                msg["content"] = c.clone();
                            } else {
                                msg["content"] = Value::Null;
                            }
                            messages.push(msg);
                        } else {
                            messages.push(json!({"role": m.role, "content": m.content}));
                        }
                    } else {
                        messages.push(json!({"role": m.role, "content": m.content}));
                    }
                }
            }
        }

        // Ensure conversation exists in DB and save user message immediately
        if let (Some(conv_id), Some(ref db)) = (&conv_id_owned, &db_owned) {
            ensure_conversation(db, conv_id, &provider_id_owned);
            save_message_now(db, conv_id, "user", &prompt_owned);
        }

        // Add current user message
        messages.push(json!({"role": "user", "content": prompt_owned}));

        // Track total tokens across all iterations
        let mut total_input_tokens: u64 = 0;
        let mut total_output_tokens: u64 = 0;
        let mut actual_model: Option<String> = None;
        let mut final_stop_reason = "end_turn".to_string();

        // Loop detection: track recent tool calls to detect repetition
        let mut recent_tool_calls: Vec<String> = Vec::new();
        const MAX_REPEAT_COUNT: usize = 3;

        for iteration in 0..MAX_AGENTIC_ITERATIONS {
            eprintln!(
                "[OPENAI_COMPAT] Agentic iteration {}/{} with {} messages",
                iteration + 1,
                MAX_AGENTIC_ITERATIONS,
                messages.len()
            );

            // Check if frontend disconnected (receiver dropped)
            if tx.is_closed() {
                eprintln!("[OPENAI_COMPAT] Frontend disconnected, stopping agentic loop");
                break;
            }

            // Build request body
            let mut body = json!({
                "model": model_name_clone,
                "messages": messages,
                "stream": true,
                "stream_options": {"include_usage": true},
            });

            // Apply provider-specific default parameters
            let defaults = provider_default_params(&provider_id_owned);
            if let Some(temp) = defaults.get("temperature") {
                body["temperature"] = temp.clone();
            }
            if let Some(max) = defaults.get("max_tokens") {
                body["max_tokens"] = max.clone();
            }

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
                    eprintln!("[OPENAI_COMPAT] Error on iteration {}: {error_msg}", iteration + 1);
                    let hint = if error_msg.contains("429") || error_msg.contains("rate_limit") {
                        "\nHint: Rate limit reached. Wait a moment and try again, or use a different provider."
                    } else if error_msg.contains("401") || error_msg.contains("403") {
                        "\nHint: Authentication failed. Check your API key in Settings."
                    } else if error_msg.contains("404") {
                        "\nHint: Model not found. The model ID may have changed — try a different model."
                    } else if error_msg.contains("Connection") || error_msg.contains("tls") {
                        "\nHint: Connection failed. The provider may be down or unreachable."
                    } else {
                        ""
                    };
                    let _ = tx.send(CliTokenData {
                        token: format!("\n**Error:** {error_msg}{hint}"),
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

            // Send cumulative token tracking status after each iteration
            let _ = tx.send(CliTokenData {
                token: String::new(),
                is_done: false,
                session_id: None,
                stop_reason: None,
                cost_usd: None,
                duration_ms: Some(start.elapsed().as_millis() as u64),
                model_id: actual_model.clone(),
                input_tokens: Some(total_input_tokens),
                output_tokens: Some(total_output_tokens),
            });

            // If no tool calls, we're done — save final assistant response and exit
            if result.tool_calls.is_empty() {
                if !result.content.is_empty() {
                    if let (Some(conv_id), Some(ref db)) = (&conv_id_owned, &db_owned) {
                        save_message_now(db, conv_id, "assistant", &result.content);
                    }
                }
                eprintln!(
                    "[OPENAI_COMPAT] No tool calls, finishing after iteration {}",
                    iteration + 1
                );
                break;
            }

            // --- Tool calls detected: execute them and loop ---

            // Loop detection: check if the same tool calls are repeating
            let call_signature: String = result.tool_calls.iter()
                .map(|tc| format!("{}:{}", tc.name, &tc.arguments[..tc.arguments.len().min(200)]))
                .collect::<Vec<_>>()
                .join("|");
            let repeat_count = recent_tool_calls.iter().filter(|s| *s == &call_signature).count();
            recent_tool_calls.push(call_signature);
            if repeat_count >= MAX_REPEAT_COUNT {
                eprintln!("[OPENAI_COMPAT] Loop detected: same tool call repeated {} times, stopping", repeat_count + 1);
                let loop_msg = "\n\n[Generation stopped — detected repeated tool calls (possible infinite loop)]";
                let _ = tx.send(CliTokenData {
                    token: loop_msg.to_string(),
                    is_done: false, session_id: None, stop_reason: None, cost_usd: None,
                    duration_ms: None, model_id: actual_model.clone(), input_tokens: None, output_tokens: None,
                });
                if let (Some(conv_id), Some(ref db)) = (&conv_id_owned, &db_owned) {
                    save_message_now(db, conv_id, "assistant", loop_msg);
                }
                final_stop_reason = "infinite_loop".to_string();
                break;
            }

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
            messages.push(assistant_msg.clone());

            // Save assistant tool_call message to DB (store as JSON so we can reconstruct)
            if let (Some(conv_id), Some(ref db)) = (&conv_id_owned, &db_owned) {
                let stored = json!({
                    "tool_calls": assistant_msg.get("tool_calls"),
                    "content": assistant_msg.get("content"),
                });
                save_message_now(db, conv_id, "assistant", &stored.to_string());
            }

            // Execute tool calls — parallel if multiple, sequential if single
            let tool_results: Vec<(String, String, String)> = if result.tool_calls.len() > 1 {
                result.tool_calls.iter().map(|tc| {
                    let args_display = if tc.arguments.is_empty() { "{}".to_string() } else { tc.arguments.clone() };
                    let _ = tx.send(CliTokenData {
                        token: format!("\n<tool_call>{{\"name\": \"{}\", \"arguments\": {}}}</tool_call>\n", tc.name, args_display),
                        is_done: false, session_id: None, stop_reason: None, cost_usd: None,
                        duration_ms: None, model_id: actual_model.clone(), input_tokens: None, output_tokens: None,
                    });
                    eprintln!(
                        "[OPENAI_COMPAT] Executing tool: {} args={}",
                        tc.name,
                        &tc.arguments[..tc.arguments.len().min(200)]
                    );
                    let result_text = execute_openai_tool(&tc.name, &tc.arguments);
                    // Smart truncation: keep head + tail for large outputs, 50KB safety net
                    let safe = if result_text.len() > 50_000 {
                        format!("{}\n\n[... truncated at 50KB, total {} bytes]", &result_text[..50_000], result_text.len())
                    } else {
                        result_text
                    };
                    let truncated = truncate_tool_output(&safe, 8000);
                    (tc.id.clone(), tc.name.clone(), truncated)
                }).collect()
            } else {
                result.tool_calls.iter().map(|tc| {
                    let args_display = if tc.arguments.is_empty() { "{}".to_string() } else { tc.arguments.clone() };
                    let _ = tx.send(CliTokenData {
                        token: format!("\n<tool_call>{{\"name\": \"{}\", \"arguments\": {}}}</tool_call>\n", tc.name, args_display),
                        is_done: false, session_id: None, stop_reason: None, cost_usd: None,
                        duration_ms: None, model_id: actual_model.clone(), input_tokens: None, output_tokens: None,
                    });
                    eprintln!(
                        "[OPENAI_COMPAT] Executing tool: {} args={}",
                        tc.name,
                        &tc.arguments[..tc.arguments.len().min(200)]
                    );
                    let result_text = execute_openai_tool(&tc.name, &tc.arguments);
                    // Smart truncation: keep head + tail for large outputs, 50KB safety net
                    let safe = if result_text.len() > 50_000 {
                        format!("{}\n\n[... truncated at 50KB, total {} bytes]", &result_text[..50_000], result_text.len())
                    } else {
                        result_text
                    };
                    let truncated = truncate_tool_output(&safe, 8000);
                    (tc.id.clone(), tc.name.clone(), truncated)
                }).collect()
            };

            // Display results, save to DB, and add to messages
            for (id, name, truncated) in &tool_results {
                let response_display = format!(
                    "\n<tool_response>{}</tool_response>\n",
                    &truncated[..truncated.len().min(2000)]
                );
                let _ = tx.send(CliTokenData {
                    token: response_display,
                    is_done: false, session_id: None, stop_reason: None, cost_usd: None,
                    duration_ms: None, model_id: actual_model.clone(), input_tokens: None, output_tokens: None,
                });
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": id,
                    "content": truncated,
                }));
                // Save tool result to DB — store tool_call_id with content for reconstruction
                if let (Some(conv_id), Some(ref db)) = (&conv_id_owned, &db_owned) {
                    save_message_now(db, conv_id, "tool", &format!("{id}\n\n{truncated}"));
                }
                let _ = name; // suppress unused warning
            }

            // Check if frontend disconnected after tool execution
            if tx.is_closed() {
                eprintln!("[OPENAI_COMPAT] Frontend disconnected after tool execution, stopping agentic loop");
                break;
            }

            // Context budget check — trim older tool results if approaching limit
            if total_input_tokens > MAX_INPUT_TOKENS * 80 / 100 {
                eprintln!("[OPENAI_COMPAT] Input tokens ({}) approaching limit, trimming older tool results", total_input_tokens);
                trim_old_tool_results(&mut messages);
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

        // Compute cost estimate based on provider pricing
        let cost_usd = provider_cost_per_million(&provider_id_owned, &model_name_clone)
            .map(|(ic, oc)| {
                (total_input_tokens as f64 * ic / 1_000_000.0) + (total_output_tokens as f64 * oc / 1_000_000.0)
            });

        // Send done event
        let _ = tx.send(CliTokenData {
            token: String::new(),
            is_done: true,
            session_id: None,
            stop_reason: Some(final_stop_reason),
            cost_usd,
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

        // Generate title after response is done (non-blocking, cheap API call)
        if let (Some(conv_id), Some(ref db)) = (&conv_id_owned, &db_owned) {
            if db.get_conversation_title(conv_id).ok().flatten().is_none() {
                let assistant_text: String = messages.iter().rev()
                    .find(|m| m["role"] == "assistant" && m["content"].is_string())
                    .and_then(|m| m["content"].as_str())
                    .unwrap_or("")
                    .to_string();
                let base_url_clean = url.trim_end_matches("/chat/completions").to_string();
                if let Some(title) = generate_title_via_provider(
                    &base_url_clean, &api_key_owned, &model_name_clone,
                    &prompt_owned, &assistant_text,
                ) {
                    eprintln!("[OPENAI_COMPAT] Generated title: {title}");
                    let _ = db.set_conversation_title(conv_id, &title);
                }
            }
        }
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

    #[test]
    fn test_truncate_tool_output_short() {
        let short = "hello world";
        assert_eq!(truncate_tool_output(short, 100), short);
    }

    #[test]
    fn test_truncate_tool_output_long() {
        let long = "a".repeat(10_000);
        let result = truncate_tool_output(&long, 1000);
        assert!(result.len() < 10_000);
        assert!(result.contains("chars truncated"));
    }

    #[test]
    fn test_provider_cost_per_million() {
        assert!(provider_cost_per_million("deepseek", "deepseek-chat").is_some());
        assert!(provider_cost_per_million("gemini", "gemini-2.0-flash").is_none());
        assert!(provider_cost_per_million("unknown_provider", "model").is_none());
    }

    #[test]
    fn test_trim_old_tool_results() {
        let mut messages: Vec<Value> = vec![
            json!({"role": "user", "content": "do something"}),
        ];
        // Add enough messages to trigger trimming (>8)
        for i in 0..10 {
            messages.push(json!({"role": "assistant", "content": format!("step {i}")}));
            messages.push(json!({"role": "tool", "tool_call_id": format!("id_{i}"), "content": "x".repeat(500)}));
        }
        let original_len = messages.len();
        trim_old_tool_results(&mut messages);
        // Length should remain the same (we truncate content, not remove messages)
        assert_eq!(messages.len(), original_len);
        // Early tool messages should be truncated
        let early_tool = messages[2].get("content").unwrap().as_str().unwrap();
        assert!(early_tool.contains("truncated"));
    }
}
