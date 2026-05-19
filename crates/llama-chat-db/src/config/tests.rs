use super::*;
use std::sync::Arc;

fn create_test_db() -> Arc<Database> {
    Arc::new(Database::new(":memory:").unwrap())
}

#[test]
fn test_load_default_config() {
    let db = create_test_db();
    let config = db.load_config();

    assert_eq!(config.sampler_type, "Greedy");
    assert_eq!(config.temperature, 0.7);
    assert_eq!(config.top_p, 0.95);
}

#[test]
fn test_save_and_load_config() {
    let db = create_test_db();

    let config = DbSamplerConfig {
        sampler_type: "Temperature".to_string(),
        temperature: 0.8,
        top_p: 0.9,
        top_k: 40,
        mirostat_tau: 3.0,
        mirostat_eta: 0.2,
        repeat_penalty: 1.1,
        min_p: 0.05,
        typical_p: 1.0,
        frequency_penalty: 0.0,
        presence_penalty: 0.0,
        penalty_last_n: 64,
        dry_multiplier: 0.0,
        dry_base: 1.75,
        dry_allowed_length: 2,
        dry_penalty_last_n: -1,
        top_n_sigma: -1.0,
        flash_attention: true,
        cache_type_k: "f16".to_string(),
        cache_type_v: "f16".to_string(),
        n_batch: 2048,
        model_path: Some("/path/to/model.gguf".to_string()),
        system_prompt: Some("You are helpful".to_string()),
        system_prompt_type: SystemPromptType::Custom,
        context_size: Some(4096),
        stop_tokens: Some(vec!["</s>".to_string()]),
        model_history: Vec::new(),
        disable_file_logging: true,
        tool_tag_exec_open: Some("<custom_exec>".to_string()),
        tool_tag_exec_close: Some("</custom_exec>".to_string()),
        tool_tag_output_open: None,
        tool_tag_output_close: None,
        web_browser_backend: None,
        models_directory: None,
        seed: -1,
        n_ubatch: 512,
        n_threads: 0,
        n_threads_batch: 0,
        rope_freq_base: 0.0,
        rope_freq_scale: 0.0,
        use_mlock: false,
        use_mmap: true,
        main_gpu: 0,
        split_mode: "layer".to_string(),
        use_rtk: true,
        use_htmd: false,
        tag_pairs: None,
        proactive_compaction: false,
        safe_tool_injection: false,
        telegram_bot_token: None,
        telegram_chat_id: None,
        provider_api_keys: None,
        max_tool_calls: 2000,
        loop_detection_limit: 15,
        thinking_mode: None,
    };

    db.save_config(&config).unwrap();
    let loaded = db.load_config();

    assert_eq!(loaded.sampler_type, "Temperature");
    assert_eq!(loaded.temperature, 0.8);
    assert_eq!(loaded.model_path, Some("/path/to/model.gguf".to_string()));
    assert_eq!(loaded.stop_tokens, Some(vec!["</s>".to_string()]));
}

#[test]
fn test_model_history() {
    let db = create_test_db();

    db.add_to_model_history("/model1.gguf").unwrap();
    db.add_to_model_history("/model2.gguf").unwrap();
    db.add_to_model_history("/model3.gguf").unwrap();

    let history = db.get_model_history().unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(history[0], "/model3.gguf");
    assert_eq!(history[1], "/model2.gguf");
    assert_eq!(history[2], "/model1.gguf");

    db.add_to_model_history("/model1.gguf").unwrap();
    let history = db.get_model_history().unwrap();
    assert_eq!(history[0], "/model1.gguf");
}

#[test]
fn test_model_history_limit() {
    let db = create_test_db();

    for i in 0..15 {
        db.add_to_model_history(&format!("/model{i}.gguf"))
            .unwrap();
    }

    let history = db.get_model_history().unwrap();
    assert_eq!(history.len(), 10);
    assert_eq!(history[0], "/model14.gguf");
}

#[test]
fn test_logs() {
    let db = create_test_db();
    let conv_id = db.create_conversation(None).unwrap();

    db.insert_log(Some(&conv_id), "INFO", "Test message 1")
        .unwrap();
    db.insert_log(Some(&conv_id), "DEBUG", "Test message 2")
        .unwrap();

    let logs = db.get_logs_for_conversation(&conv_id).unwrap();
    assert_eq!(logs.len(), 2);
    assert_eq!(logs[0].level, "INFO");
    assert_eq!(logs[0].message, "Test message 1");
}
