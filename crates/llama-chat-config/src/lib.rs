// Configuration loading and conversion between DB and API config types.

#[macro_use]
extern crate llama_chat_types;

use llama_chat_db::config::DbSamplerConfig;
use llama_chat_db::Database;
use llama_chat_types::SamplerConfig;
use llama_chat_types::TagPair;

/// Convert DbSamplerConfig to the JSON-serializable SamplerConfig
pub fn db_config_to_sampler_config(db_config: &DbSamplerConfig) -> SamplerConfig {
    let tag_pairs: Option<Vec<TagPair>> = db_config.tag_pairs.as_ref().and_then(|json_str| {
        serde_json::from_str(json_str).ok()
    });

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
        web_browser_backend: db_config.web_browser_backend.clone(),
        models_directory: db_config.models_directory.clone(),
        seed: db_config.seed,
        n_ubatch: db_config.n_ubatch,
        n_threads: db_config.n_threads,
        n_threads_batch: db_config.n_threads_batch,
        rope_freq_base: db_config.rope_freq_base,
        rope_freq_scale: db_config.rope_freq_scale,
        use_mlock: db_config.use_mlock,
        use_mmap: db_config.use_mmap,
        main_gpu: db_config.main_gpu,
        split_mode: db_config.split_mode.clone(),
        use_rtk: db_config.use_rtk,
        use_htmd: db_config.use_htmd,
        tag_pairs,
        proactive_compaction: db_config.proactive_compaction,
        telegram_bot_token: db_config.telegram_bot_token.clone(),
        telegram_chat_id: db_config.telegram_chat_id.clone(),
        provider_api_keys: db_config.provider_api_keys.clone(),
    }
}

/// Convert SamplerConfig to DbSamplerConfig
pub fn sampler_config_to_db(config: &SamplerConfig) -> DbSamplerConfig {
    let tag_pairs_json: Option<String> = config.tag_pairs.as_ref().and_then(|pairs| {
        serde_json::to_string(pairs).ok()
    });

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
        web_browser_backend: config.web_browser_backend.clone(),
        models_directory: config.models_directory.clone(),
        seed: config.seed,
        n_ubatch: config.n_ubatch,
        n_threads: config.n_threads,
        n_threads_batch: config.n_threads_batch,
        rope_freq_base: config.rope_freq_base,
        rope_freq_scale: config.rope_freq_scale,
        use_mlock: config.use_mlock,
        use_mmap: config.use_mmap,
        main_gpu: config.main_gpu,
        split_mode: config.split_mode.clone(),
        use_rtk: config.use_rtk,
        use_htmd: config.use_htmd,
        tag_pairs: tag_pairs_json,
        proactive_compaction: config.proactive_compaction,
        telegram_bot_token: config.telegram_bot_token.clone(),
        telegram_chat_id: config.telegram_chat_id.clone(),
        provider_api_keys: config.provider_api_keys.clone(),
    }
}

/// Load configuration from database
pub fn load_config(db: &Database) -> SamplerConfig {
    let db_config = db.load_config();
    db_config_to_sampler_config(&db_config)
}

/// Load configuration for a specific conversation.
/// Falls back to global config if no per-conversation config exists.
pub fn load_config_for_conversation(db: &Database, _conversation_id: &str) -> SamplerConfig {
    // Always use the global config from Load Model modal.
    // Per-conversation overrides were removed — one config for all conversations.
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
