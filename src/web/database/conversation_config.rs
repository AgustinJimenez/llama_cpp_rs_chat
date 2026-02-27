// Per-conversation configuration database operations

use super::{current_timestamp_millis, db_error, Database};
use super::config::DbSamplerConfig;
use crate::web::models::SystemPromptType;
use rusqlite::params;

fn parse_system_prompt_type(s: Option<String>) -> SystemPromptType {
    match s.as_deref() {
        Some("Custom") => SystemPromptType::Custom,
        Some("UserDefined") => SystemPromptType::UserDefined,
        _ => SystemPromptType::Default,
    }
}

fn system_prompt_type_to_str(spt: &SystemPromptType) -> &'static str {
    match spt {
        SystemPromptType::Default => "Default",
        SystemPromptType::Custom => "Custom",
        SystemPromptType::UserDefined => "UserDefined",
    }
}

impl Database {
    /// Save (snapshot) a configuration for a specific conversation.
    /// Used when creating a new conversation to capture the current global config.
    pub fn save_conversation_config(
        &self,
        conversation_id: &str,
        config: &DbSamplerConfig,
    ) -> Result<(), String> {
        let conn = self.connection();
        let stop_tokens_json = config
            .stop_tokens
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or_default());

        conn.execute(
            "INSERT OR REPLACE INTO conversation_config
             (conversation_id, sampler_type, temperature, top_p, top_k,
              mirostat_tau, mirostat_eta, repeat_penalty, min_p,
              typical_p, frequency_penalty, presence_penalty, penalty_last_n,
              dry_multiplier, dry_base, dry_allowed_length, dry_penalty_last_n,
              top_n_sigma, flash_attention, cache_type_k, cache_type_v, n_batch,
              context_size, system_prompt, system_prompt_type, stop_tokens,
              tool_tag_exec_open, tool_tag_exec_close, tool_tag_output_open, tool_tag_output_close,
              updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                     ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26,
                     ?27, ?28, ?29, ?30, ?31)",
            params![
                conversation_id,
                config.sampler_type,
                config.temperature,
                config.top_p,
                config.top_k,
                config.mirostat_tau,
                config.mirostat_eta,
                config.repeat_penalty,
                config.min_p,
                config.typical_p,
                config.frequency_penalty,
                config.presence_penalty,
                config.penalty_last_n,
                config.dry_multiplier,
                config.dry_base,
                config.dry_allowed_length,
                config.dry_penalty_last_n,
                config.top_n_sigma,
                config.flash_attention as i32,
                config.cache_type_k,
                config.cache_type_v,
                config.n_batch,
                config.context_size,
                config.system_prompt,
                system_prompt_type_to_str(&config.system_prompt_type),
                stop_tokens_json,
                config.tool_tag_exec_open,
                config.tool_tag_exec_close,
                config.tool_tag_output_open,
                config.tool_tag_output_close,
                current_timestamp_millis(),
            ],
        )
        .map_err(db_error("save conversation config"))?;

        Ok(())
    }

    /// Load configuration for a specific conversation.
    /// Returns None if no per-conversation config exists (caller should fall back to global).
    pub fn load_conversation_config(&self, conversation_id: &str) -> Option<DbSamplerConfig> {
        let conn = self.connection();
        conn.query_row(
            "SELECT sampler_type, temperature, top_p, top_k, mirostat_tau,
                    mirostat_eta, repeat_penalty, min_p,
                    typical_p, frequency_penalty, presence_penalty, penalty_last_n,
                    dry_multiplier, dry_base, dry_allowed_length, dry_penalty_last_n,
                    top_n_sigma, flash_attention, cache_type_k, cache_type_v, n_batch,
                    context_size, system_prompt, system_prompt_type, stop_tokens,
                    tool_tag_exec_open, tool_tag_exec_close, tool_tag_output_open, tool_tag_output_close
             FROM conversation_config WHERE conversation_id = ?1",
            [conversation_id],
            |row| {
                let stop_tokens_json: Option<String> = row.get(24)?;
                let stop_tokens =
                    stop_tokens_json.and_then(|j| serde_json::from_str(&j).ok());

                Ok(DbSamplerConfig {
                    sampler_type: row
                        .get::<_, Option<String>>(0)?
                        .unwrap_or_else(|| "Greedy".to_string()),
                    temperature: row.get::<_, Option<f64>>(1)?.unwrap_or(0.7),
                    top_p: row.get::<_, Option<f64>>(2)?.unwrap_or(0.95),
                    top_k: row.get::<_, Option<u32>>(3)?.unwrap_or(20),
                    mirostat_tau: row.get::<_, Option<f64>>(4)?.unwrap_or(5.0),
                    mirostat_eta: row.get::<_, Option<f64>>(5)?.unwrap_or(0.1),
                    repeat_penalty: row.get::<_, Option<f64>>(6)?.unwrap_or(1.0),
                    min_p: row.get::<_, Option<f64>>(7)?.unwrap_or(0.0),
                    typical_p: row.get::<_, Option<f64>>(8)?.unwrap_or(1.0),
                    frequency_penalty: row.get::<_, Option<f64>>(9)?.unwrap_or(0.0),
                    presence_penalty: row.get::<_, Option<f64>>(10)?.unwrap_or(0.0),
                    penalty_last_n: row.get::<_, Option<i32>>(11)?.unwrap_or(64),
                    dry_multiplier: row.get::<_, Option<f64>>(12)?.unwrap_or(0.0),
                    dry_base: row.get::<_, Option<f64>>(13)?.unwrap_or(1.75),
                    dry_allowed_length: row.get::<_, Option<i32>>(14)?.unwrap_or(2),
                    dry_penalty_last_n: row.get::<_, Option<i32>>(15)?.unwrap_or(-1),
                    top_n_sigma: row.get::<_, Option<f64>>(16)?.unwrap_or(-1.0),
                    flash_attention: row.get::<_, Option<i32>>(17)?.unwrap_or(0) != 0,
                    cache_type_k: row
                        .get::<_, Option<String>>(18)?
                        .unwrap_or_else(|| "f16".to_string()),
                    cache_type_v: row
                        .get::<_, Option<String>>(19)?
                        .unwrap_or_else(|| "f16".to_string()),
                    n_batch: row.get::<_, Option<u32>>(20)?.unwrap_or(2048),
                    context_size: row.get(21)?,
                    system_prompt: row.get(22)?,
                    system_prompt_type: parse_system_prompt_type(row.get(23)?),
                    stop_tokens,
                    tool_tag_exec_open: row.get(25)?,
                    tool_tag_exec_close: row.get(26)?,
                    tool_tag_output_open: row.get(27)?,
                    tool_tag_output_close: row.get(28)?,
                    // Global-only fields — not stored per-conversation
                    model_path: None,
                    model_history: Vec::new(),
                    disable_file_logging: true,
                    web_search_provider: None,
                    web_search_api_key: None,
                })
            },
        )
        .ok()
    }

    /// Update configuration for a specific conversation.
    #[allow(dead_code)]
    pub fn update_conversation_config(
        &self,
        conversation_id: &str,
        config: &DbSamplerConfig,
    ) -> Result<(), String> {
        let conn = self.connection();
        let stop_tokens_json = config
            .stop_tokens
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or_default());

        let changes = conn
            .execute(
                "UPDATE conversation_config SET
                 sampler_type = ?1, temperature = ?2, top_p = ?3, top_k = ?4,
                 mirostat_tau = ?5, mirostat_eta = ?6, repeat_penalty = ?7, min_p = ?8,
                 typical_p = ?9, frequency_penalty = ?10, presence_penalty = ?11,
                 penalty_last_n = ?12, dry_multiplier = ?13, dry_base = ?14,
                 dry_allowed_length = ?15, dry_penalty_last_n = ?16, top_n_sigma = ?17,
                 flash_attention = ?18, cache_type_k = ?19, cache_type_v = ?20,
                 n_batch = ?21, context_size = ?22, system_prompt = ?23,
                 system_prompt_type = ?24, stop_tokens = ?25,
                 tool_tag_exec_open = ?26, tool_tag_exec_close = ?27,
                 tool_tag_output_open = ?28, tool_tag_output_close = ?29,
                 updated_at = ?30
                 WHERE conversation_id = ?31",
                params![
                    config.sampler_type,
                    config.temperature,
                    config.top_p,
                    config.top_k,
                    config.mirostat_tau,
                    config.mirostat_eta,
                    config.repeat_penalty,
                    config.min_p,
                    config.typical_p,
                    config.frequency_penalty,
                    config.presence_penalty,
                    config.penalty_last_n,
                    config.dry_multiplier,
                    config.dry_base,
                    config.dry_allowed_length,
                    config.dry_penalty_last_n,
                    config.top_n_sigma,
                    config.flash_attention as i32,
                    config.cache_type_k,
                    config.cache_type_v,
                    config.n_batch,
                    config.context_size,
                    config.system_prompt,
                    system_prompt_type_to_str(&config.system_prompt_type),
                    stop_tokens_json,
                    config.tool_tag_exec_open,
                    config.tool_tag_exec_close,
                    config.tool_tag_output_open,
                    config.tool_tag_output_close,
                    current_timestamp_millis(),
                    conversation_id,
                ],
            )
            .map_err(db_error("update conversation config"))?;

        if changes == 0 {
            return Err(format!(
                "No conversation_config found for {conversation_id}"
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn create_test_db() -> Arc<Database> {
        Arc::new(Database::new(":memory:").unwrap())
    }

    #[test]
    fn test_save_and_load_conversation_config() {
        let db = create_test_db();
        let conv_id = db.create_conversation(None).unwrap();

        let config = DbSamplerConfig {
            temperature: 0.9,
            top_p: 0.8,
            sampler_type: "Temperature".to_string(),
            ..DbSamplerConfig::default()
        };

        db.save_conversation_config(&conv_id, &config).unwrap();

        let loaded = db.load_conversation_config(&conv_id);
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.temperature, 0.9);
        assert_eq!(loaded.top_p, 0.8);
        assert_eq!(loaded.sampler_type, "Temperature");
    }

    #[test]
    fn test_load_nonexistent_conversation_config() {
        let db = create_test_db();
        let loaded = db.load_conversation_config("nonexistent");
        assert!(loaded.is_none());
    }

    #[test]
    fn test_update_conversation_config() {
        let db = create_test_db();
        let conv_id = db.create_conversation(None).unwrap();

        let config = DbSamplerConfig::default();
        db.save_conversation_config(&conv_id, &config).unwrap();

        let mut updated = config.clone();
        updated.temperature = 1.5;
        updated.top_k = 50;

        db.update_conversation_config(&conv_id, &updated).unwrap();

        let loaded = db.load_conversation_config(&conv_id).unwrap();
        assert_eq!(loaded.temperature, 1.5);
        assert_eq!(loaded.top_k, 50);
    }

    #[test]
    fn test_conversation_config_cascade_delete() {
        let db = create_test_db();
        let conv_id = db.create_conversation(None).unwrap();

        db.save_conversation_config(&conv_id, &DbSamplerConfig::default())
            .unwrap();
        assert!(db.load_conversation_config(&conv_id).is_some());

        db.delete_conversation(&conv_id).unwrap();
        assert!(db.load_conversation_config(&conv_id).is_none());
    }
}
