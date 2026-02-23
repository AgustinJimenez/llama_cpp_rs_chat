// Configuration database operations

use super::{current_timestamp_millis, db_error, Database};
use crate::web::models::SystemPromptType;
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
            flash_attention: false,
            cache_type_k: "f16".to_string(),
            cache_type_v: "f16".to_string(),
            n_batch: 2048,
            model_path: None,
            system_prompt: None,
            system_prompt_type: SystemPromptType::Default,
            context_size: Some(32768),
            stop_tokens: None,
            model_history: Vec::new(),
            disable_file_logging: true,
            tool_tag_exec_open: None,
            tool_tag_exec_close: None,
            tool_tag_output_open: None,
            tool_tag_output_close: None,
        }
    }
}

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
    /// Load configuration from database
    pub fn load_config(&self) -> DbSamplerConfig {
        // Query config table - use a block to release the lock before calling get_model_history
        let result = {
            let conn = self.connection();
            conn.query_row(
                "SELECT sampler_type, temperature, top_p, top_k, mirostat_tau,
                        mirostat_eta, model_path, system_prompt, system_prompt_type,
                        context_size, stop_tokens, disable_file_logging, repeat_penalty, min_p,
                        flash_attention, cache_type_k, cache_type_v, n_batch,
                        typical_p, frequency_penalty, presence_penalty, penalty_last_n,
                        dry_multiplier, dry_base, dry_allowed_length, dry_penalty_last_n,
                        top_n_sigma,
                        tool_tag_exec_open, tool_tag_exec_close, tool_tag_output_open, tool_tag_output_close
                 FROM config WHERE id = 1",
                [],
                |row| {
                    let stop_tokens_json: Option<String> = row.get(10)?;
                    let stop_tokens = stop_tokens_json.and_then(|j| serde_json::from_str(&j).ok());

                    Ok(DbSamplerConfig {
                        sampler_type: row
                            .get::<_, Option<String>>(0)?
                            .unwrap_or_else(|| "Greedy".to_string()),
                        temperature: row.get::<_, Option<f64>>(1)?.unwrap_or(0.7),
                        top_p: row.get::<_, Option<f64>>(2)?.unwrap_or(0.95),
                        top_k: row.get::<_, Option<u32>>(3)?.unwrap_or(20),
                        mirostat_tau: row.get::<_, Option<f64>>(4)?.unwrap_or(5.0),
                        mirostat_eta: row.get::<_, Option<f64>>(5)?.unwrap_or(0.1),
                        model_path: row.get(6)?,
                        system_prompt: row.get(7)?,
                        system_prompt_type: parse_system_prompt_type(row.get(8)?),
                        context_size: row.get(9)?,
                        stop_tokens,
                        model_history: Vec::new(), // Loaded separately
                        disable_file_logging: row.get::<_, Option<i32>>(11)?.unwrap_or(1) != 0,
                        repeat_penalty: row.get::<_, Option<f64>>(12)?.unwrap_or(1.0),
                        min_p: row.get::<_, Option<f64>>(13)?.unwrap_or(0.0),
                        flash_attention: row.get::<_, Option<i32>>(14)?.unwrap_or(0) != 0,
                        cache_type_k: row.get::<_, Option<String>>(15)?.unwrap_or_else(|| "f16".to_string()),
                        cache_type_v: row.get::<_, Option<String>>(16)?.unwrap_or_else(|| "f16".to_string()),
                        n_batch: row.get::<_, Option<u32>>(17)?.unwrap_or(2048),
                        typical_p: row.get::<_, Option<f64>>(18)?.unwrap_or(1.0),
                        frequency_penalty: row.get::<_, Option<f64>>(19)?.unwrap_or(0.0),
                        presence_penalty: row.get::<_, Option<f64>>(20)?.unwrap_or(0.0),
                        penalty_last_n: row.get::<_, Option<i32>>(21)?.unwrap_or(64),
                        dry_multiplier: row.get::<_, Option<f64>>(22)?.unwrap_or(0.0),
                        dry_base: row.get::<_, Option<f64>>(23)?.unwrap_or(1.75),
                        dry_allowed_length: row.get::<_, Option<i32>>(24)?.unwrap_or(2),
                        dry_penalty_last_n: row.get::<_, Option<i32>>(25)?.unwrap_or(-1),
                        top_n_sigma: row.get::<_, Option<f64>>(26)?.unwrap_or(-1.0),
                        tool_tag_exec_open: row.get(27)?,
                        tool_tag_exec_close: row.get(28)?,
                        tool_tag_output_open: row.get(29)?,
                        tool_tag_output_close: row.get(30)?,
                    })
                },
            )
        }; // Connection lock is released here

        let mut config = result.unwrap_or_else(|_| DbSamplerConfig::default());

        // Load model history from separate table (now safe - lock was released)
        config.model_history = self.get_model_history().unwrap_or_default();

        config
    }

    /// Save configuration to database
    pub fn save_config(&self, config: &DbSamplerConfig) -> Result<(), String> {
        let conn = self.connection();
        let stop_tokens_json = config
            .stop_tokens
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or_default());

        conn.execute(
            "INSERT OR REPLACE INTO config
             (id, sampler_type, temperature, top_p, top_k, mirostat_tau,
              mirostat_eta, repeat_penalty, min_p, model_path, system_prompt, system_prompt_type,
              context_size, stop_tokens, disable_file_logging, flash_attention,
              cache_type_k, cache_type_v, n_batch,
              typical_p, frequency_penalty, presence_penalty, penalty_last_n,
              dry_multiplier, dry_base, dry_allowed_length, dry_penalty_last_n,
              top_n_sigma,
              tool_tag_exec_open, tool_tag_exec_close, tool_tag_output_open, tool_tag_output_close,
              updated_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
                     ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32)",
            params![
                config.sampler_type,
                config.temperature,
                config.top_p,
                config.top_k,
                config.mirostat_tau,
                config.mirostat_eta,
                config.repeat_penalty,
                config.min_p,
                config.model_path,
                config.system_prompt,
                system_prompt_type_to_str(&config.system_prompt_type),
                config.context_size,
                stop_tokens_json,
                config.disable_file_logging as i32,
                config.flash_attention as i32,
                config.cache_type_k,
                config.cache_type_v,
                config.n_batch,
                config.typical_p,
                config.frequency_penalty,
                config.presence_penalty,
                config.penalty_last_n,
                config.dry_multiplier,
                config.dry_base,
                config.dry_allowed_length,
                config.dry_penalty_last_n,
                config.top_n_sigma,
                config.tool_tag_exec_open,
                config.tool_tag_exec_close,
                config.tool_tag_output_open,
                config.tool_tag_output_close,
                current_timestamp_millis(),
            ],
        )
        .map_err(db_error("save config"))?;

        Ok(())
    }

    /// Update specific config fields (preserves model_history)
    pub fn update_config(&self, config: &DbSamplerConfig) -> Result<(), String> {
        let conn = self.connection();
        let stop_tokens_json = config
            .stop_tokens
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or_default());

        conn.execute(
            "UPDATE config SET
             sampler_type = ?1, temperature = ?2, top_p = ?3, top_k = ?4,
             mirostat_tau = ?5, mirostat_eta = ?6, repeat_penalty = ?7, min_p = ?8,
             model_path = ?9, system_prompt = ?10, system_prompt_type = ?11, context_size = ?12,
             stop_tokens = ?13, disable_file_logging = ?14,
             flash_attention = ?15, cache_type_k = ?16, cache_type_v = ?17, n_batch = ?18,
             typical_p = ?19, frequency_penalty = ?20, presence_penalty = ?21, penalty_last_n = ?22,
             dry_multiplier = ?23, dry_base = ?24, dry_allowed_length = ?25, dry_penalty_last_n = ?26,
             top_n_sigma = ?27,
             tool_tag_exec_open = ?28, tool_tag_exec_close = ?29,
             tool_tag_output_open = ?30, tool_tag_output_close = ?31,
             updated_at = ?32
             WHERE id = 1",
            params![
                config.sampler_type,
                config.temperature,
                config.top_p,
                config.top_k,
                config.mirostat_tau,
                config.mirostat_eta,
                config.repeat_penalty,
                config.min_p,
                config.model_path,
                config.system_prompt,
                system_prompt_type_to_str(&config.system_prompt_type),
                config.context_size,
                stop_tokens_json,
                config.disable_file_logging as i32,
                config.flash_attention as i32,
                config.cache_type_k,
                config.cache_type_v,
                config.n_batch,
                config.typical_p,
                config.frequency_penalty,
                config.presence_penalty,
                config.penalty_last_n,
                config.dry_multiplier,
                config.dry_base,
                config.dry_allowed_length,
                config.dry_penalty_last_n,
                config.top_n_sigma,
                config.tool_tag_exec_open,
                config.tool_tag_exec_close,
                config.tool_tag_output_open,
                config.tool_tag_output_close,
                current_timestamp_millis(),
            ],
        )
        .map_err(db_error("update config"))?;

        Ok(())
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct LogEntry {
    pub level: String,
    pub message: String,
    pub timestamp: i64,
}

#[cfg(test)]
mod tests {
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
            flash_attention: false,
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
        assert_eq!(history[0], "/model3.gguf"); // Most recent first
        assert_eq!(history[1], "/model2.gguf");
        assert_eq!(history[2], "/model1.gguf");

        // Adding existing model moves it to front
        db.add_to_model_history("/model1.gguf").unwrap();
        let history = db.get_model_history().unwrap();
        assert_eq!(history[0], "/model1.gguf");
    }

    #[test]
    fn test_model_history_limit() {
        let db = create_test_db();

        // Add 15 models
        for i in 0..15 {
            db.add_to_model_history(&format!("/model{i}.gguf"))
                .unwrap();
        }

        let history = db.get_model_history().unwrap();
        assert_eq!(history.len(), 10); // Should be limited to 10
        assert_eq!(history[0], "/model14.gguf"); // Most recent
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
}
