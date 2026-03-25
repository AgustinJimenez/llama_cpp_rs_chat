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
    pub id: &'static str,
    pub name: &'static str,
    pub available: bool,
    pub description: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub models: Vec<&'static str>,
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
            id: "local",
            name: "Local Model (llama.cpp)",
            available: true,
            description: "Run models locally on your GPU",
            version: None,
            models: Vec::new(),
        },
        ProviderInfo {
            id: "claude_code",
            name: "Claude Code",
            available: claude_available,
            description: "Use your Claude Code subscription (Max/Pro)",
            version: if claude_available {
                claude_code::get_version().await
            } else {
                None
            },
            models: vec!["opus", "sonnet", "haiku"],
        },
        ProviderInfo {
            id: "codex",
            name: "Codex CLI",
            available: codex_available,
            description: "Use your local Codex CLI session",
            version: if codex_available {
                codex::get_version().await
            } else {
                None
            },
            models: vec!["gpt-5"],
        },
    ];

    // Add OpenAI-compatible providers
    for preset in openai_compat::PROVIDER_PRESETS {
        let has_key = openai_compat::resolve_api_key(preset.id, api_keys_json).is_some();
        // custom_openai needs both key and base_url to be "available"
        let available = if preset.id == "custom_openai" {
            has_key && openai_compat::resolve_base_url(preset.id, api_keys_json).is_some()
        } else {
            has_key
        };
        providers.push(ProviderInfo {
            id: preset.id,
            name: preset.name,
            available,
            description: preset.description,
            version: None,
            models: preset.models.to_vec(),
        });
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
) -> Result<mpsc::UnboundedReceiver<CliTokenData>, String> {
    match provider_id {
        "claude_code" => claude_code::generate(prompt, model, max_turns, cwd, session_id).await,
        "codex" => codex::generate(prompt, model, max_turns, cwd, session_id).await,
        id if openai_compat::is_openai_compat(id) => {
            let api_key = openai_compat::resolve_api_key(id, api_keys_json)
                .ok_or_else(|| format!("No API key configured for provider '{id}'. Set it in Settings or via environment variable."))?;
            let base_url = openai_compat::resolve_base_url(id, api_keys_json)
                .ok_or_else(|| format!("No base URL configured for provider '{id}'."))?;
            openai_compat::generate(id, prompt, model, &base_url, &api_key).await
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
