use super::database::config::DbSamplerConfig;
use super::database::Database;
use super::models::SamplerConfig;

#[cfg(not(feature = "mock"))]
use super::models::SharedLlamaState;

#[cfg(feature = "mock")]
use super::models::SharedLlamaState;

use super::chat_handler::{get_tool_tags_for_model, get_universal_system_prompt_with_tags};

// Import logging macro
use crate::sys_warn;

/// Convert DbSamplerConfig to the JSON-serializable SamplerConfig
pub fn db_config_to_sampler_config(db_config: &DbSamplerConfig) -> SamplerConfig {
    SamplerConfig {
        sampler_type: db_config.sampler_type.clone(),
        temperature: db_config.temperature,
        top_p: db_config.top_p,
        top_k: db_config.top_k,
        mirostat_tau: db_config.mirostat_tau,
        mirostat_eta: db_config.mirostat_eta,
        model_path: db_config.model_path.clone(),
        system_prompt: db_config.system_prompt.clone(),
        system_prompt_type: db_config.system_prompt_type.clone(),
        context_size: db_config.context_size,
        stop_tokens: db_config.stop_tokens.clone(),
        model_history: db_config.model_history.clone(),
    }
}

/// Convert SamplerConfig to DbSamplerConfig
pub fn sampler_config_to_db(config: &SamplerConfig) -> DbSamplerConfig {
    DbSamplerConfig {
        sampler_type: config.sampler_type.clone(),
        temperature: config.temperature,
        top_p: config.top_p,
        top_k: config.top_k,
        mirostat_tau: config.mirostat_tau,
        mirostat_eta: config.mirostat_eta,
        model_path: config.model_path.clone(),
        system_prompt: config.system_prompt.clone(),
        system_prompt_type: config.system_prompt_type.clone(),
        context_size: config.context_size,
        stop_tokens: config.stop_tokens.clone(),
        model_history: config.model_history.clone(),
    }
}

/// Load configuration from database
pub fn load_config(db: &Database) -> SamplerConfig {
    let db_config = db.load_config();
    db_config_to_sampler_config(&db_config)
}

// Helper function to add a model path to history
pub fn add_to_model_history(db: &Database, model_path: &str) {
    if let Err(e) = db.add_to_model_history(model_path) {
        sys_warn!("Failed to add to model history: {}", e);
    }

    // Also update model_path in config
    let mut db_config = db.load_config();
    db_config.model_path = Some(model_path.to_string());
    if let Err(e) = db.update_config(&db_config) {
        sys_warn!("Failed to update model_path in config: {}", e);
    }
}

/// Get the resolved system prompt based on config and model state
///
/// This helper resolves the system prompt in the following priority:
/// 1. If config has "__AGENTIC__" marker, use universal agentic prompt
/// 2. If config has custom prompt, use it
/// 3. Otherwise, try to get model's default system prompt from GGUF metadata
#[cfg(not(feature = "mock"))]
pub fn get_resolved_system_prompt(
    db: &Database,
    llama_state: &Option<SharedLlamaState>,
) -> Option<String> {
    let config = load_config(db);
    match config.system_prompt.as_deref() {
        // "__AGENTIC__" marker = use universal agentic prompt with command execution
        // Use model-specific tool tags if a model is loaded
        Some("__AGENTIC__") => {
            let general_name = llama_state.as_ref().and_then(|state| {
                state
                    .lock()
                    .ok()
                    .and_then(|guard| guard.as_ref().and_then(|s| s.general_name.clone()))
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
pub fn get_resolved_system_prompt(
    db: &Database,
    _llama_state: &Option<SharedLlamaState>,
) -> Option<String> {
    let config = load_config(db);
    match config.system_prompt.as_deref() {
        Some("__AGENTIC__") => Some(get_universal_system_prompt_with_tags(
            &super::chat::tool_tags::DEFAULT_TAGS,
        )),
        Some(custom) => Some(custom.to_string()),
        None => None,
    }
}
