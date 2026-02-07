use super::models::SamplerConfig;
use std::fs;

#[cfg(not(feature = "mock"))]
use super::models::SharedLlamaState;

#[cfg(feature = "mock")]
use super::models::SharedLlamaState;

use super::chat_handler::{get_universal_system_prompt_with_tags, get_tool_tags_for_model};

// Import logging macro
use crate::sys_warn;

// Helper function to load configuration
pub fn load_config() -> SamplerConfig {
    let config_path = "assets/config.json";
    match fs::read_to_string(config_path) {
        Ok(content) => match serde_json::from_str::<SamplerConfig>(&content) {
            Ok(config) => config,
            Err(e) => {
                sys_warn!("Failed to parse config file: {}, using defaults", e);
                SamplerConfig::default()
            }
        },
        Err(_) => {
            // Config file doesn't exist, use defaults
            SamplerConfig::default()
        }
    }
}

/// Get the resolved system prompt based on config and model state
///
/// This helper resolves the system prompt in the following priority:
/// 1. If config has "__AGENTIC__" marker, use universal agentic prompt
/// 2. If config has custom prompt, use it
/// 3. Otherwise, try to get model's default system prompt from GGUF metadata
#[cfg(not(feature = "mock"))]
pub fn get_resolved_system_prompt(llama_state: &Option<SharedLlamaState>) -> Option<String> {
    let config = load_config();
    match config.system_prompt.as_deref() {
        // "__AGENTIC__" marker = use universal agentic prompt with command execution
        // Use model-specific tool tags if a model is loaded
        Some("__AGENTIC__") => {
            let general_name = llama_state.as_ref().and_then(|state| {
                state.lock().ok().and_then(|guard| {
                    guard.as_ref().and_then(|s| s.general_name.clone())
                })
            });
            let tags = get_tool_tags_for_model(general_name.as_deref());
            Some(get_universal_system_prompt_with_tags(tags))
        }
        // Custom prompt = use as-is
        Some(custom) => Some(custom.to_string()),
        // None = use model's default system prompt from GGUF
        None => {
            if let Some(ref state) = llama_state {
                if let Ok(state_guard) = state.lock() {
                    state_guard
                        .as_ref()
                        .and_then(|s| s.model_default_system_prompt.clone())
                } else {
                    None
                }
            } else {
                None
            }
        }
    }
}

/// Mock version for testing
#[cfg(feature = "mock")]
pub fn get_resolved_system_prompt(_llama_state: &Option<SharedLlamaState>) -> Option<String> {
    let config = load_config();
    match config.system_prompt.as_deref() {
        Some("__AGENTIC__") => Some(get_universal_system_prompt_with_tags(&super::chat::tool_tags::DEFAULT_TAGS)),
        Some(custom) => Some(custom.to_string()),
        None => None,
    }
}

// Helper function to add a model path to history
pub fn add_to_model_history(model_path: &str) {
    let config_path = "assets/config.json";

    // Load current config
    let mut config = load_config();

    // Remove the path if it already exists (to move it to the front)
    config.model_history.retain(|p| p != model_path);

    // Add to the front of the list
    config.model_history.insert(0, model_path.to_string());

    // Keep only the last 10 paths
    if config.model_history.len() > 10 {
        config.model_history.truncate(10);
    }

    // Save the updated config
    let _ = fs::create_dir_all("assets");
    if let Ok(json) = serde_json::to_string_pretty(&config) {
        let _ = fs::write(config_path, json);
    }
}
