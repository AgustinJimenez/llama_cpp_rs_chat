//! Provider abstraction layer for multiple AI backends.
//!
//! Supports:
//! - Local (llama.cpp via worker process)
//! - Claude Code (CLI subprocess using user's subscription)
//! - Codex CLI (CLI subprocess using the user's local Codex auth)

use serde::Serialize;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

pub mod claude_code;
pub mod codex;
pub mod openai_compat;

#[derive(Debug, Clone, Serialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub available: bool,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub models: Vec<String>,
}

/// Token/event data sent from CLI-backed providers to the frontend.
#[allow(dead_code)]
pub struct CliTokenData {
    pub token: String,
    pub is_done: bool,
    pub session_id: Option<String>,
    pub stop_reason: Option<String>,
    pub cost_usd: Option<f64>,
    pub duration_ms: Option<u64>,
    pub model_id: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[allow(dead_code)]
fn remote_provider_tool_delta() -> &'static str {
    "You are running inside LLaMA Chat as a remote CLI-backed provider. Prefer your built-in native tools. Keep responses concise and action-oriented."
}

pub fn resolve_cli_cwd(cwd: Option<&str>) -> Result<PathBuf, String> {
    if let Some(dir) = cwd.map(str::trim).filter(|dir| !dir.is_empty()) {
        let path = Path::new(dir);
        if path.is_dir() {
            return Ok(path.to_path_buf());
        }
        return Err(format!("Provider working directory does not exist: {dir}"));
    }

    let fallback = std::env::temp_dir().join("llama_chat_remote_provider");
    std::fs::create_dir_all(&fallback)
        .map_err(|e| format!("Failed to prepare provider working directory: {e}"))?;
    Ok(fallback)
}

pub fn compose_prompt(
    provider_id: &str,
    user_prompt: &str,
    session_id: Option<&str>,
) -> String {
    // Only inject the remote-provider shim on fresh sessions.
    // Once the CLI session exists, rely on the provider's own persisted context.
    if session_id.is_some() {
        return user_prompt.to_string();
    }

    let _ = provider_id; // used in future for provider-specific prompts
    user_prompt.to_string()
}

pub async fn list_providers_with_keys(api_keys_json: Option<&str>) -> Vec<ProviderInfo> {
    let claude_available = claude_code::is_available().await;
    let codex_available = codex::is_available().await;

    let mut providers = vec![
        ProviderInfo {
            id: "local".into(),
            name: "Local Model (llama.cpp)".into(),
            available: true,
            description: "Run models locally on your GPU".into(),
            version: None,
            models: Vec::new(),
        },
        ProviderInfo {
            id: "claude_code".into(),
            name: "Claude Code".into(),
            available: claude_available,
            description: "Use your Claude Code subscription (Max/Pro)".into(),
            version: if claude_available {
                claude_code::get_version().await
            } else {
                None
            },
            models: vec!["opus".into(), "sonnet".into(), "haiku".into()],
        },
        ProviderInfo {
            id: "codex".into(),
            name: "Codex CLI".into(),
            available: codex_available,
            description: "Use your local Codex CLI session".into(),
            version: if codex_available {
                codex::get_version().await
            } else {
                None
            },
            models: vec!["gpt-5".into()],
        },
    ];

    // Add OpenAI-compatible providers from presets
    for preset in openai_compat::PROVIDER_PRESETS {
        let has_key = openai_compat::resolve_api_key(preset.id, api_keys_json).is_some();
        let available = has_key;
        providers.push(ProviderInfo {
            id: preset.id.into(),
            name: preset.name.into(),
            available,
            description: preset.description.into(),
            version: None,
            models: preset.models.iter().map(|s| s.to_string()).collect(),
        });
    }

    // Add user-defined custom providers from api_keys_json
    if let Some(json) = api_keys_json {
        if let Ok(map) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(json) {
            for (id, val) in &map {
                if val.get("custom").and_then(|v| v.as_bool()) == Some(true) {
                    let name = val.get("name").and_then(|v| v.as_str()).unwrap_or("Custom");
                    let base_url = val.get("base_url").and_then(|v| v.as_str()).unwrap_or("");
                    let api_key = val.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
                    let models_str = val.get("models").and_then(|v| v.as_str()).unwrap_or("");
                    let models: Vec<String> = models_str.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let available = !base_url.is_empty() && (!api_key.is_empty() || base_url.contains("localhost") || base_url.contains("127.0.0.1"));
                    let display_name = if name.is_empty() { id.as_str() } else { name };
                    providers.push(ProviderInfo {
                        id: id.clone(),
                        name: display_name.to_string(),
                        available,
                        description: format!("Custom: {}", base_url),
                        version: None,
                        models,
                    });
                }
            }
        }
    }

    providers
}

pub async fn generate(
    provider_id: &str,
    prompt: &str,
    model: Option<&str>,
    max_turns: Option<u32>,
    cwd: Option<&str>,
    session_id: Option<&str>,
    api_keys_json: Option<&str>,
    conversation_id: Option<&str>,
    db: Option<&crate::web::database::SharedDatabase>,
) -> Result<mpsc::UnboundedReceiver<CliTokenData>, String> {
    match provider_id {
        "claude_code" => claude_code::generate(prompt, model, max_turns, cwd, session_id).await,
        "codex" => codex::generate(prompt, model, max_turns, cwd, session_id).await,
        // TODO (#6): Hybrid Claude/Codex provider — route to Claude Code or Codex
        // with conversation history and tool dispatch. Too complex for this batch.
        id if openai_compat::is_openai_compat(id) => {
            let api_key = openai_compat::resolve_api_key(id, api_keys_json)
                .ok_or_else(|| format!("No API key configured for provider '{id}'. Set it in Settings or via environment variable."))?;
            let base_url = openai_compat::resolve_base_url(id, api_keys_json)
                .ok_or_else(|| format!("No base URL configured for provider '{id}'."))?;
            openai_compat::generate(id, prompt, model, &base_url, &api_key, conversation_id, db).await
        }
        id if id.starts_with("custom_") => {
            // User-defined custom provider — resolve key/url from api_keys_json
            let api_key = openai_compat::resolve_custom_field(id, "api_key", api_keys_json).unwrap_or_default();
            let base_url = openai_compat::resolve_custom_field(id, "base_url", api_keys_json)
                .ok_or_else(|| format!("No base URL configured for custom provider '{id}'."))?;
            openai_compat::generate(id, prompt, model, &base_url, &api_key, conversation_id, db).await
        }
        _ => Err(format!("Unknown provider: {provider_id}")),
    }
}

pub fn default_model(provider_id: &str) -> &'static str {
    match provider_id {
        "claude_code" => "sonnet",
        "codex" => "gpt-5",
        _ => {
            if let Some(preset) = openai_compat::get_preset(provider_id) {
                preset.models.first().copied().unwrap_or("")
            } else {
                ""
            }
        }
    }
}
