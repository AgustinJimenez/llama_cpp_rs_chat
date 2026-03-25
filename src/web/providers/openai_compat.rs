//! OpenAI-compatible provider — works with any API that follows the OpenAI chat completions format.
//!
//! Supports: Groq, Gemini, SambaNova, Cerebras, OpenRouter, Together, Fireworks,
//! DeepSeek, local vLLM, Ollama, and any other OpenAI-compatible endpoint.
//!
//! Protocol: HTTP SSE streaming from `POST {base_url}/chat/completions`

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::CliTokenData;

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

/// Generate a response using an OpenAI-compatible API.
///
/// Streams SSE tokens from `POST {base_url}/chat/completions` and converts them
/// to `CliTokenData` events on the returned channel.
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

    // Build the request body
    let body = serde_json::json!({
        "model": model_name,
        "messages": [{"role": "user", "content": prompt}],
        "stream": true,
        "stream_options": {"include_usage": true},
    });

    let body_str = serde_json::to_string(&body)
        .map_err(|e| format!("Failed to serialize request: {e}"))?;

    let url_clone = url.clone();
    let api_key_owned = api_key.to_string();
    let provider_id_owned = provider_id.to_string();
    let model_name_clone = model_name.clone();

    // Use ureq in a blocking task for the streaming HTTP request
    tokio::task::spawn_blocking(move || {
        let start = std::time::Instant::now();

        let resp = ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(30))
            .build()
            .post(&url_clone)
            .set("Content-Type", "application/json")
            .set("Authorization", &format!("Bearer {api_key_owned}"))
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
                let _ = tx.send(CliTokenData {
                    token: String::new(),
                    is_done: true,
                    session_id: None,
                    stop_reason: Some("error".to_string()),
                    cost_usd: None,
                    duration_ms: Some(start.elapsed().as_millis() as u64),
                    model_id: Some(model_name_clone),
                    input_tokens: None,
                    output_tokens: None,
                });
                return;
            }
        };

        // Read SSE stream line by line
        let buf_reader = std::io::BufReader::new(reader);
        use std::io::BufRead;

        let mut actual_model: Option<String> = None;
        let mut input_tokens: Option<u64> = None;
        let mut output_tokens: Option<u64> = None;
        let mut finish_reason: Option<String> = None;

        // Track streaming tool calls
        let mut tool_name = String::new();
        let mut tool_args = String::new();
        let mut in_tool_call = false;

        for line_result in buf_reader.lines() {
            let line = match line_result {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("[OPENAI_COMPAT] Read error: {e}");
                    break;
                }
            };

            // SSE format: lines starting with "data: "
            if !line.starts_with("data: ") {
                continue;
            }

            let data = &line[6..];

            // End of stream
            if data == "[DONE]" {
                break;
            }

            // Parse the chunk
            let chunk: ChatCompletionChunk = match serde_json::from_str(data) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("[OPENAI_COMPAT] Parse error: {e} | data: {}", &data[..data.len().min(200)]);
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
                // Track finish reason
                if let Some(ref reason) = choice.finish_reason {
                    finish_reason = Some(reason.clone());
                }

                if let Some(ref delta) = choice.delta {
                    // Handle content tokens
                    if let Some(ref content) = delta.content {
                        if !content.is_empty() {
                            let _ = tx.send(CliTokenData {
                                token: content.clone(),
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

                    // Handle tool calls
                    if let Some(ref tool_calls) = delta.tool_calls {
                        for tc in tool_calls {
                            if let Some(ref func) = tc.function {
                                // New tool call starts when name is present
                                if let Some(ref name) = func.name {
                                    // Flush previous tool call if any
                                    if in_tool_call {
                                        let display = format!(
                                            "\n<tool_call>{{\"name\": \"{}\", \"arguments\": {}}}</tool_call>\n",
                                            tool_name,
                                            if tool_args.is_empty() { "{}".to_string() } else { tool_args.clone() }
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
                                    }
                                    tool_name = name.clone();
                                    tool_args.clear();
                                    in_tool_call = true;
                                }

                                // Accumulate arguments
                                if let Some(ref args) = func.arguments {
                                    tool_args.push_str(args);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Flush final tool call if any
        if in_tool_call {
            let display = format!(
                "\n<tool_call>{{\"name\": \"{}\", \"arguments\": {}}}</tool_call>\n",
                tool_name,
                if tool_args.is_empty() { "{}".to_string() } else { tool_args }
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
        }

        let duration_ms = start.elapsed().as_millis() as u64;
        let stop = finish_reason.unwrap_or_else(|| "end_turn".to_string());

        eprintln!(
            "[OPENAI_COMPAT] Done: provider={} model={:?} stop={} duration={}ms tokens={}in/{}out",
            provider_id_owned,
            actual_model,
            stop,
            duration_ms,
            input_tokens.unwrap_or(0),
            output_tokens.unwrap_or(0),
        );

        // Send done event
        let _ = tx.send(CliTokenData {
            token: String::new(),
            is_done: true,
            session_id: None,
            stop_reason: Some(stop),
            cost_usd: None,
            duration_ms: Some(duration_ms),
            model_id: actual_model,
            input_tokens,
            output_tokens,
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
}
