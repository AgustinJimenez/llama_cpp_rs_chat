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
    pub model_path: Option<String>,
    pub system_prompt: Option<String>,
    pub system_prompt_type: SystemPromptType,
    pub context_size: Option<u32>,
    pub stop_tokens: Option<Vec<String>>,
    pub model_history: Vec<String>,
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
            model_path: None,
            system_prompt: None,
            system_prompt_type: SystemPromptType::Default,
            context_size: Some(32768),
            stop_tokens: None,
            model_history: Vec::new(),
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
                        context_size, stop_tokens
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
              mirostat_eta, model_path, system_prompt, system_prompt_type,
              context_size, stop_tokens, updated_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                config.sampler_type,
                config.temperature,
                config.top_p,
                config.top_k,
                config.mirostat_tau,
                config.mirostat_eta,
                config.model_path,
                config.system_prompt,
                system_prompt_type_to_str(&config.system_prompt_type),
                config.context_size,
                stop_tokens_json,
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
             mirostat_tau = ?5, mirostat_eta = ?6, model_path = ?7,
             system_prompt = ?8, system_prompt_type = ?9, context_size = ?10,
             stop_tokens = ?11, updated_at = ?12
             WHERE id = 1",
            params![
                config.sampler_type,
                config.temperature,
                config.top_p,
                config.top_k,
                config.mirostat_tau,
                config.mirostat_eta,
                config.model_path,
                config.system_prompt,
                system_prompt_type_to_str(&config.system_prompt_type),
                config.context_size,
                stop_tokens_json,
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

#[derive(Debug, Clone)]
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
            model_path: Some("/path/to/model.gguf".to_string()),
            system_prompt: Some("You are helpful".to_string()),
            system_prompt_type: SystemPromptType::Custom,
            context_size: Some(4096),
            stop_tokens: Some(vec!["</s>".to_string()]),
            model_history: Vec::new(),
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
