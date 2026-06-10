// Configuration database operations

use super::{current_timestamp_millis, db_error, Database};
use llama_chat_types::SystemPromptType;
use rusqlite::params;

/// Sampler configuration stored in database
#[derive(Debug, Clone)]
pub struct DbSamplerConfig {
    pub sampler_type: String,
    pub temperature: f64,
    pub top_p: f64,
    pub top_k: u32,
    pub mirostat_tau: f64,
    pub mirostat_eta: f64,
    pub repeat_penalty: f64,
    pub min_p: f64,
    pub typical_p: f64,
    pub frequency_penalty: f64,
    pub presence_penalty: f64,
    pub penalty_last_n: i32,
    pub dry_multiplier: f64,
    pub dry_base: f64,
    pub dry_allowed_length: i32,
    pub dry_penalty_last_n: i32,
    pub top_n_sigma: f64,
    pub flash_attention: bool,
    pub cache_type_k: String,
    pub cache_type_v: String,
    pub n_batch: u32,
    pub model_path: Option<String>,
    pub system_prompt: Option<String>,
    pub system_prompt_type: SystemPromptType,
    pub context_size: Option<u32>,
    pub stop_tokens: Option<Vec<String>>,
    pub model_history: Vec<String>,
    pub disable_file_logging: bool,
    // Tool tag overrides (None = use auto-detected)
    pub tool_tag_exec_open: Option<String>,
    pub tool_tag_exec_close: Option<String>,
    pub tool_tag_output_open: Option<String>,
    pub tool_tag_output_close: Option<String>,
    // App settings
    pub web_browser_backend: Option<String>,
    pub models_directory: Option<String>,
    // Hardware / context / sampler params
    pub seed: i32,
    pub n_ubatch: u32,
    pub n_threads: i32,
    pub n_threads_batch: i32,
    pub rope_freq_base: f32,
    pub rope_freq_scale: f32,
    pub use_mlock: bool,
    pub use_mmap: bool,
    pub main_gpu: i32,
    pub split_mode: String,
    // RTK output compression
    pub use_rtk: bool,
    // htmd web fetch (better markdown extraction)
    pub use_htmd: bool,
    // Tag pairs (stored as JSON string in DB)
    pub tag_pairs: Option<String>,
    // Proactive compaction during generation
    pub proactive_compaction: bool,
    // Safe tool injection: restart context after each tool call (slower but avoids MoE deadlock)
    pub safe_tool_injection: bool,
    // Telegram notification settings
    pub telegram_bot_token: Option<String>,
    pub telegram_chat_id: Option<String>,
    // Provider API keys (JSON blob for OpenAI-compatible providers)
    pub provider_api_keys: Option<String>,
    // Max tool calls per remote provider turn (safety limit)
    pub max_tool_calls: i32,
    pub loop_detection_limit: i32,
    // Thinking mode: None = use model default, Some(true/false) = explicit override
    pub thinking_mode: Option<bool>,
}

impl Default for DbSamplerConfig {
    fn default() -> Self {
        Self {
            sampler_type: "Greedy".to_string(),
            temperature: 0.7,
            top_p: 0.95,
            top_k: 20,
            mirostat_tau: 5.0,
            mirostat_eta: 0.1,
            repeat_penalty: 1.0,
            min_p: 0.0,
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
            model_path: None,
            system_prompt: None,
            system_prompt_type: SystemPromptType::Custom,
            context_size: Some(32768),
            stop_tokens: None,
            model_history: Vec::new(),
            disable_file_logging: true,
            tool_tag_exec_open: None,
            tool_tag_exec_close: None,
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
            proactive_compaction: true,
            safe_tool_injection: false,
            telegram_bot_token: None,
            telegram_chat_id: None,
            provider_api_keys: None,
            max_tool_calls: 2000,
            loop_detection_limit: 15,
            thinking_mode: None,
        }
    }
}

impl Database {
    /// Load configuration from database
    pub fn load_config(&self) -> DbSamplerConfig {
        let result = {
            let conn = self.connection();
            conn.query_row(
                "SELECT disable_file_logging,
                        web_browser_backend,
                        models_directory,
                        use_rtk,
                        use_htmd,
                        telegram_bot_token,
                        telegram_chat_id,
                        provider_api_keys,
                        max_tool_calls,
                        loop_detection_limit
                 FROM config WHERE id = 1",
                [],
                |row| {
                    #[allow(clippy::field_reassign_with_default)]
                    let mut config = DbSamplerConfig::default();
                    config.disable_file_logging = row.get::<_, Option<i32>>(0)?.unwrap_or(1) != 0;
                    config.web_browser_backend = row.get(1)?;
                    config.models_directory = row.get(2)?;
                    config.use_rtk = row.get::<_, Option<i32>>(3)?.unwrap_or(1) != 0;
                    config.use_htmd = row.get::<_, Option<i32>>(4)?.unwrap_or(0) != 0;
                    config.telegram_bot_token = row.get(5)?;
                    config.telegram_chat_id = row.get(6)?;
                    config.provider_api_keys = row.get(7)?;
                    config.max_tool_calls = row.get::<_, Option<i32>>(8)?.unwrap_or(2000);
                    config.loop_detection_limit = row.get::<_, Option<i32>>(9)?.unwrap_or(15);
                    Ok(config)
                },
            )
        };

        let mut config = result.unwrap_or_else(|_| DbSamplerConfig::default());

        // Load model history from separate table (now safe - lock was released)
        config.model_history = self.get_model_history().unwrap_or_default();

        config
    }

    /// Save configuration to database
    pub fn save_config(&self, config: &DbSamplerConfig) -> Result<(), String> {
        let conn = self.connection();

        conn.execute(
            "INSERT INTO config
             (id, disable_file_logging, web_browser_backend, models_directory,
              use_rtk, use_htmd, telegram_bot_token, telegram_chat_id,
              provider_api_keys, max_tool_calls, loop_detection_limit, updated_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                config.disable_file_logging as i32,
                config.web_browser_backend,
                config.models_directory,
                config.use_rtk as i32,
                config.use_htmd as i32,
                config.telegram_bot_token,
                config.telegram_chat_id,
                config.provider_api_keys,
                config.max_tool_calls,
                config.loop_detection_limit,
                current_timestamp_millis(),
            ],
        )
        .or_else(|_| {
            conn.execute(
                "UPDATE config SET
                 disable_file_logging = ?1,
                 web_browser_backend = ?2,
                 models_directory = ?3,
                 use_rtk = ?4,
                 use_htmd = ?5,
                 telegram_bot_token = ?6,
                 telegram_chat_id = ?7,
                 provider_api_keys = ?8,
                 max_tool_calls = ?9,
                 loop_detection_limit = ?10,
                 updated_at = ?11
                 WHERE id = 1",
                params![
                    config.disable_file_logging as i32,
                    config.web_browser_backend,
                    config.models_directory,
                    config.use_rtk as i32,
                    config.use_htmd as i32,
                    config.telegram_bot_token,
                    config.telegram_chat_id,
                    config.provider_api_keys,
                    config.max_tool_calls,
                    config.loop_detection_limit,
                    current_timestamp_millis(),
                ],
            )
        })
        .map_err(db_error("save config"))?;

        Ok(())
    }

    /// Update specific config fields (preserves model_history)
    pub fn update_config(&self, config: &DbSamplerConfig) -> Result<(), String> {
        self.save_config(config)
    }

    /// Add model to history (MRU list)
    pub fn add_to_model_history(&self, model_path: &str) -> Result<(), String> {
        let conn = self.connection();
        let now = current_timestamp_millis();

        // Delete if exists (to update position)
        conn.execute(
            "DELETE FROM model_history WHERE model_path = ?1",
            [model_path],
        )
        .map_err(db_error("delete from model history"))?;

        // Shift all display_order values up
        conn.execute(
            "UPDATE model_history SET display_order = display_order + 1",
            [],
        )
        .map_err(db_error("update model history order"))?;

        // Insert at position 0
        conn.execute(
            "INSERT INTO model_history (model_path, last_used, display_order) VALUES (?1, ?2, 0)",
            params![model_path, now],
        )
        .map_err(db_error("insert into model history"))?;

        // Keep only top 10
        conn.execute("DELETE FROM model_history WHERE display_order >= 10", [])
            .map_err(db_error("trim model history"))?;

        Ok(())
    }

    /// Get model history (ordered by display_order)
    pub fn get_model_history(&self) -> Result<Vec<String>, String> {
        let conn = self.connection();
        let mut stmt = conn
            .prepare("SELECT model_path FROM model_history ORDER BY display_order ASC")
            .map_err(db_error("prepare model history query"))?;

        let paths = stmt
            .query_map([], |row| row.get(0))
            .map_err(db_error("query model history"))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(paths)
    }

    /// Insert log entry
    pub fn insert_log(
        &self,
        conversation_id: Option<&str>,
        level: &str,
        message: &str,
    ) -> Result<(), String> {
        let conn = self.connection();
        let now = current_timestamp_millis();

        conn.execute(
            "INSERT INTO logs (conversation_id, level, message, timestamp) VALUES (?1, ?2, ?3, ?4)",
            params![conversation_id, level, message, now],
        )
        .map_err(db_error("insert log"))?;

        Ok(())
    }

    /// Get logs for a conversation
    pub fn get_logs_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<LogEntry>, String> {
        let conn = self.connection();
        let mut stmt = conn
            .prepare(
                "SELECT level, message, timestamp FROM logs
                 WHERE conversation_id = ?1 ORDER BY timestamp ASC",
            )
            .map_err(db_error("prepare logs query"))?;

        let logs = stmt
            .query_map([conversation_id], |row| {
                Ok(LogEntry {
                    level: row.get(0)?,
                    message: row.get(1)?,
                    timestamp: row.get(2)?,
                })
            })
            .map_err(db_error("query logs"))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(logs)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LogEntry {
    pub level: String,
    pub message: String,
    pub timestamp: i64,
}

#[cfg(test)]
mod tests;
