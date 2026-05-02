//! Provider abstraction layer for multiple AI backends.
//!
//! Supports:
//! - Local (llama.cpp via worker process)
//! - Claude Code (CLI subprocess using user's subscription)
//! - Codex CLI (CLI subprocess using the user's local Codex auth)

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use tokio::sync::mpsc;

pub mod claude_code;
pub mod codex;
pub mod gemini;
pub mod openai_compat;

/// Apply CREATE_NO_WINDOW on Windows to prevent terminal flashing for CLI providers.
#[cfg(windows)]
pub fn hide_cli_window(cmd: &mut tokio::process::Command) {
    #[allow(unused_imports)]
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x08000000);
}

#[cfg(not(windows))]
pub fn hide_cli_window(_cmd: &mut tokio::process::Command) {}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub available: bool,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub models: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_base_url: Option<String>,
}

/// Token/event data sent from CLI-backed providers to the frontend.
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

/// Resolve the full path to a CLI binary, falling back to a login shell on macOS
/// so that binaries installed via nvm/npm are found when the app is launched from Finder.
pub async fn resolve_bin_path(name: &str) -> Option<String> {
    // Try direct lookup first (works in dev / when PATH is already set correctly)
    let mut cmd = tokio::process::Command::new(name);
    cmd.arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null());
    hide_cli_window(&mut cmd);
    let ok = cmd.status()
        .await
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        return Some(name.to_string());
    }

    // On macOS bundled apps the GUI PATH is minimal (/usr/bin:/bin:/usr/sbin:/sbin).
    // Use a login shell to pick up nvm/npm/homebrew paths from .zshrc/.bash_profile.
    #[cfg(target_os = "macos")]
    {
        let output = tokio::process::Command::new("/bin/zsh")
            .args(["-l", "-c", &format!("which {name}")])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .output()
            .await
            .ok()?;
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }

    None
}

static CLAUDE_PATH: OnceLock<Option<String>> = OnceLock::new();
static CODEX_PATH: OnceLock<Option<String>> = OnceLock::new();
static GEMINI_PATH: OnceLock<Option<String>> = OnceLock::new();

pub async fn claude_bin() -> Option<String> {
    if let Some(cached) = CLAUDE_PATH.get() {
        return cached.clone();
    }
    let name = if cfg!(target_os = "windows") { "claude.cmd" } else { "claude" };
    let resolved = resolve_bin_path(name).await;
    CLAUDE_PATH.get_or_init(|| resolved.clone());
    resolved
}

pub async fn codex_bin() -> Option<String> {
    if let Some(cached) = CODEX_PATH.get() {
        return cached.clone();
    }
    let name = if cfg!(target_os = "windows") { "codex.cmd" } else { "codex" };
    let resolved = resolve_bin_path(name).await;
    CODEX_PATH.get_or_init(|| resolved.clone());
    resolved
}

pub async fn gemini_bin() -> Option<String> {
    if let Some(cached) = GEMINI_PATH.get() {
        return cached.clone();
    }
    let resolved = resolve_bin_path("gemini").await;
    GEMINI_PATH.get_or_init(|| resolved.clone());
    resolved
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
            default_base_url: None,
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
            default_base_url: None,
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
            default_base_url: None,
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
            default_base_url: Some(preset.base_url.into()),
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
                        default_base_url: if base_url.is_empty() { None } else { Some(base_url.to_string()) },
                    });
                }
            }
        }
    }

    providers
}

/// Synchronous variant for Tauri commands — lists providers without async CLI checks.
#[allow(dead_code)]
pub fn list_configured_providers_with_keys(api_keys_json: Option<&str>) -> Vec<ProviderInfo> {
    let mut providers = vec![
        ProviderInfo {
            id: "local".into(),
            name: "Local Model (llama.cpp)".into(),
            available: true,
            description: "Run models locally on your GPU".into(),
            version: None,
            models: Vec::new(),
            default_base_url: None,
        },
    ];
    // Add OpenAI-compatible providers from presets
    for preset in openai_compat::PROVIDER_PRESETS {
        let has_key = openai_compat::resolve_api_key(preset.id, api_keys_json).is_some();
        providers.push(ProviderInfo {
            id: preset.id.into(),
            name: preset.name.into(),
            available: has_key,
            description: preset.description.into(),
            version: None,
            models: preset.models.iter().map(|s| s.to_string()).collect(),
            default_base_url: Some(preset.base_url.into()),
        });
    }
    providers
}

/// List CLI-based providers (Claude Code, Codex, Gemini) always, with availability status.
#[allow(dead_code)]
pub async fn list_cli_providers() -> Vec<ProviderInfo> {
    let claude_available = claude_code::is_available().await;
    let codex_available = codex::is_available().await;
    let gemini_available = gemini::is_available().await;
    vec![
        ProviderInfo {
            id: "claude_code".into(),
            name: "Claude Code".into(),
            available: claude_available,
            description: "Use your Claude Code subscription".into(),
            version: if claude_available { claude_code::get_version().await } else { None },
            models: vec!["opus".into(), "sonnet".into(), "haiku".into()],
            default_base_url: None,
        },
        ProviderInfo {
            id: "codex".into(),
            name: "Codex CLI".into(),
            available: codex_available,
            description: "Use your local Codex CLI session".into(),
            version: if codex_available { codex::get_version().await } else { None },
            models: vec!["gpt-5".into()],
            default_base_url: None,
        },
        ProviderInfo {
            id: "gemini_cli".into(),
            name: "Gemini CLI".into(),
            available: gemini_available,
            description: "Use your Google Gemini CLI session".into(),
            version: if gemini_available { gemini::get_version().await } else { None },
            models: vec!["gemini-2.5-pro".into(), "gemini-2.5-flash".into()],
            default_base_url: None,
        },
    ]
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
    db: Option<&llama_chat_db::SharedDatabase>,
) -> Result<mpsc::UnboundedReceiver<CliTokenData>, String> {
    match provider_id {
        "claude_code" => claude_code::generate(prompt, model, max_turns, cwd, session_id).await,
        "codex" => codex::generate(prompt, model, max_turns, cwd, session_id).await,
        "gemini_cli" => gemini::generate(prompt, model, max_turns, cwd, session_id).await,
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
        "gemini_cli" => "gemini-2.5-flash",
        _ => {
            if let Some(preset) = openai_compat::get_preset(provider_id) {
                preset.models.first().copied().unwrap_or("")
            } else {
                ""
            }
        }
    }
}
