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
        repeat_penalty: db_config.repeat_penalty,
        min_p: db_config.min_p,
        typical_p: db_config.typical_p,
        frequency_penalty: db_config.frequency_penalty,
        presence_penalty: db_config.presence_penalty,
        penalty_last_n: db_config.penalty_last_n,
        dry_multiplier: db_config.dry_multiplier,
        dry_base: db_config.dry_base,
        dry_allowed_length: db_config.dry_allowed_length,
        dry_penalty_last_n: db_config.dry_penalty_last_n,
        top_n_sigma: db_config.top_n_sigma,
        flash_attention: db_config.flash_attention,
        cache_type_k: db_config.cache_type_k.clone(),
        cache_type_v: db_config.cache_type_v.clone(),
        n_batch: db_config.n_batch,
        model_path: db_config.model_path.clone(),
        system_prompt: db_config.system_prompt.clone(),
        system_prompt_type: db_config.system_prompt_type.clone(),
        context_size: db_config.context_size,
        stop_tokens: db_config.stop_tokens.clone(),
        model_history: db_config.model_history.clone(),
        disable_file_logging: db_config.disable_file_logging,
        tool_tag_exec_open: db_config.tool_tag_exec_open.clone(),
        tool_tag_exec_close: db_config.tool_tag_exec_close.clone(),
        tool_tag_output_open: db_config.tool_tag_output_open.clone(),
        tool_tag_output_close: db_config.tool_tag_output_close.clone(),
        web_search_provider: db_config.web_search_provider.clone(),
        web_search_api_key: db_config.web_search_api_key.clone(),
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
        repeat_penalty: config.repeat_penalty,
        min_p: config.min_p,
        typical_p: config.typical_p,
        frequency_penalty: config.frequency_penalty,
        presence_penalty: config.presence_penalty,
        penalty_last_n: config.penalty_last_n,
        dry_multiplier: config.dry_multiplier,
        dry_base: config.dry_base,
        dry_allowed_length: config.dry_allowed_length,
        dry_penalty_last_n: config.dry_penalty_last_n,
        top_n_sigma: config.top_n_sigma,
        flash_attention: config.flash_attention,
        cache_type_k: config.cache_type_k.clone(),
        cache_type_v: config.cache_type_v.clone(),
        n_batch: config.n_batch,
        model_path: config.model_path.clone(),
        system_prompt: config.system_prompt.clone(),
        system_prompt_type: config.system_prompt_type.clone(),
        context_size: config.context_size,
        stop_tokens: config.stop_tokens.clone(),
        model_history: config.model_history.clone(),
        disable_file_logging: config.disable_file_logging,
        tool_tag_exec_open: config.tool_tag_exec_open.clone(),
        tool_tag_exec_close: config.tool_tag_exec_close.clone(),
        tool_tag_output_open: config.tool_tag_output_open.clone(),
        tool_tag_output_close: config.tool_tag_output_close.clone(),
        web_search_provider: config.web_search_provider.clone(),
        web_search_api_key: config.web_search_api_key.clone(),
    }
}

/// Load configuration from database
pub fn load_config(db: &Database) -> SamplerConfig {
    let db_config = db.load_config();
    db_config_to_sampler_config(&db_config)
}

/// Load configuration for a specific conversation.
/// Falls back to global config if no per-conversation config exists.
pub fn load_config_for_conversation(db: &Database, conversation_id: &str) -> SamplerConfig {
    if let Some(conv_config) = db.load_conversation_config(conversation_id) {
        // Per-conversation config doesn't store model_path — fill it from global
        let mut config = db_config_to_sampler_config(&conv_config);
        let global = load_config(db);
        config.model_path = global.model_path;
        return config;
    }
    load_config(db)
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

/// Get the resolved system prompt based on config and model state.
///
/// Uses a cache on LlamaState to avoid re-resolving on every request.
/// Cache key: (config.system_prompt, general_name). Invalidated on config
/// or model change.
///
/// Priority: 1. "__AGENTIC__" → universal agentic prompt
///           2. Custom string → use as-is
///           3. None → model's default from GGUF metadata
#[cfg(not(feature = "mock"))]
pub fn get_resolved_system_prompt(
    db: &Database,
    llama_state: &Option<SharedLlamaState>,
) -> Option<String> {
    let config = load_config(db);
    let current_key = (config.system_prompt.clone(), {
        llama_state.as_ref().and_then(|s| {
            s.lock()
                .ok()
                .and_then(|g| g.as_ref().and_then(|st| st.general_name.clone()))
        })
    });

    // Check cache
    if let Some(ref state_arc) = llama_state {
        if let Ok(mut guard) = state_arc.lock() {
            if let Some(ref mut state) = *guard {
                if state.cached_prompt_key.as_ref() == Some(&current_key) {
                    return state.cached_system_prompt.clone();
                }
            }
        }
    }

    // Cache miss: resolve
    let resolved = match config.system_prompt.as_deref() {
        Some("__AGENTIC__") => {
            let general_name = current_key.1.as_deref();
            let tags = get_tool_tags_for_model(general_name);
            Some(get_universal_system_prompt_with_tags(&tags))
        }
        Some(custom) => Some(custom.to_string()),
        None => {
            if let Some(ref state_arc) = llama_state {
                if let Ok(guard) = state_arc.lock() {
                    guard
                        .as_ref()
                        .and_then(|s| s.model_default_system_prompt.clone())
                } else {
                    None
                }
            } else {
                None
            }
        }
    };

    // Store in cache
    if let Some(ref state_arc) = llama_state {
        if let Ok(mut guard) = state_arc.lock() {
            if let Some(ref mut state) = *guard {
                state.cached_system_prompt = resolved.clone();
                state.cached_prompt_key = Some(current_key);
            }
        }
    }

    resolved
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
            &super::chat::tool_tags::default_tags(),
        )),
        Some(custom) => Some(custom.to_string()),
        None => None,
    }
}
