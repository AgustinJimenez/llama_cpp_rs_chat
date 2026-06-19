// Agent-based configuration — named, reusable model+sampler presets.
//
// An AgentRecord bundles everything needed to drive a conversation:
// model selection, sampler params, hardware params, system prompt, and tool config.
// Conversations reference an agent by `agent_id`; per-conversation tweaks are stored
// as a sparse JSON blob in `conversations.overrides`.

use super::config::DbSamplerConfig;
use super::{current_timestamp_millis, db_error, Database};
use llama_chat_types::SystemPromptType;
use rusqlite::params;
use uuid::Uuid;

/// Full agent record as stored in the database.
#[derive(Debug, Clone)]
pub struct AgentRecord {
    pub id: String,
    pub name: String,
    // Provider / model selection
    pub provider_id: String,
    pub model_path: Option<String>,
    pub provider_model: Option<String>,
    // System prompt
    pub system_prompt: Option<String>,
    pub system_prompt_type: String,
    // Sampler params
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
    // Hardware / context params
    pub flash_attention: bool,
    pub cache_type_k: String,
    pub cache_type_v: String,
    pub n_batch: u32,
    pub context_size: Option<u32>,
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
    // Generation settings
    pub stop_tokens: Option<Vec<String>>,
    pub tag_pairs: Option<String>,
    pub tool_tag_exec_open: Option<String>,
    pub tool_tag_exec_close: Option<String>,
    pub tool_tag_output_open: Option<String>,
    pub tool_tag_output_close: Option<String>,
    // Behavior flags
    pub proactive_compaction: bool,
    pub safe_tool_injection: bool,
    pub thinking_mode: Option<bool>,
    // Heartbeat defaults
    pub heartbeat_enabled: bool,
    pub heartbeat_interval_minutes: u32,
    pub heartbeat_prompt: Option<String>,
    // Meta
    pub created_at: i64,
    pub updated_at: i64,
}

impl AgentRecord {
    /// Convert to DbSamplerConfig for use in the generation pipeline.
    ///
    /// App-level fields (telegram, browser backend, etc.) are taken from `global`
    /// since they don't belong to any individual agent.
    pub fn to_db_sampler_config(&self, global: &DbSamplerConfig) -> DbSamplerConfig {
        DbSamplerConfig {
            // Agent-owned fields
            model_path: self.model_path.clone(),
            system_prompt: self.system_prompt.clone(),
            system_prompt_type: SystemPromptType::Custom, // always Custom post-migration
            sampler_type: self.sampler_type.clone(),
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            mirostat_tau: self.mirostat_tau,
            mirostat_eta: self.mirostat_eta,
            repeat_penalty: self.repeat_penalty,
            min_p: self.min_p,
            typical_p: self.typical_p,
            frequency_penalty: self.frequency_penalty,
            presence_penalty: self.presence_penalty,
            penalty_last_n: self.penalty_last_n,
            dry_multiplier: self.dry_multiplier,
            dry_base: self.dry_base,
            dry_allowed_length: self.dry_allowed_length,
            dry_penalty_last_n: self.dry_penalty_last_n,
            top_n_sigma: self.top_n_sigma,
            flash_attention: self.flash_attention,
            cache_type_k: self.cache_type_k.clone(),
            cache_type_v: self.cache_type_v.clone(),
            n_batch: self.n_batch,
            context_size: self.context_size,
            seed: self.seed,
            n_ubatch: self.n_ubatch,
            n_threads: self.n_threads,
            n_threads_batch: self.n_threads_batch,
            rope_freq_base: self.rope_freq_base,
            rope_freq_scale: self.rope_freq_scale,
            use_mlock: self.use_mlock,
            use_mmap: self.use_mmap,
            // `self.main_gpu` actually stores the agent's GPU-layers override (see
            // routes/agents.rs spawn_worker_for_agent) — it is not a CUDA device index.
            // The real device index comes from global config.
            main_gpu: global.main_gpu,
            split_mode: self.split_mode.clone(),
            stop_tokens: self.stop_tokens.clone(),
            tag_pairs: self.tag_pairs.clone(),
            tool_tag_exec_open: self.tool_tag_exec_open.clone(),
            tool_tag_exec_close: self.tool_tag_exec_close.clone(),
            tool_tag_output_open: self.tool_tag_output_open.clone(),
            tool_tag_output_close: self.tool_tag_output_close.clone(),
            proactive_compaction: self.proactive_compaction,
            safe_tool_injection: self.safe_tool_injection,
            thinking_mode: self.thinking_mode,
            // App-level fields — taken from global config
            disable_file_logging: global.disable_file_logging,
            web_browser_backend: global.web_browser_backend.clone(),
            models_directory: global.models_directory.clone(),
            use_rtk: global.use_rtk,
            use_htmd: global.use_htmd,
            telegram_bot_token: global.telegram_bot_token.clone(),
            telegram_chat_id: global.telegram_chat_id.clone(),
            provider_api_keys: global.provider_api_keys.clone(),
            max_tool_calls: global.max_tool_calls,
            loop_detection_limit: global.loop_detection_limit,
            model_history: Vec::new(),
        }
    }

    /// Build an AgentRecord from a DbSamplerConfig snapshot (e.g. for migration or "Save as agent").
    pub fn from_db_sampler_config(name: &str, config: &DbSamplerConfig) -> Self {
        Self {
            id: format!("agent_{}", Uuid::new_v4().simple()),
            name: name.to_string(),
            provider_id: "local".to_string(),
            model_path: config.model_path.clone(),
            provider_model: None,
            system_prompt: config.system_prompt.clone(),
            system_prompt_type: "Custom".to_string(),
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
            context_size: config.context_size,
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
            stop_tokens: config.stop_tokens.clone(),
            tag_pairs: config.tag_pairs.clone(),
            tool_tag_exec_open: config.tool_tag_exec_open.clone(),
            tool_tag_exec_close: config.tool_tag_exec_close.clone(),
            tool_tag_output_open: config.tool_tag_output_open.clone(),
            tool_tag_output_close: config.tool_tag_output_close.clone(),
            proactive_compaction: config.proactive_compaction,
            safe_tool_injection: config.safe_tool_injection,
            thinking_mode: config.thinking_mode,
            heartbeat_enabled: false,
            heartbeat_interval_minutes: 30,
            heartbeat_prompt: None,
            created_at: current_timestamp_millis(),
            updated_at: current_timestamp_millis(),
        }
    }
}

// ─── CRUD ───────────────────────────────────────────────────────────────────

const SELECT_AGENT_COLS: &str = "
    id, name, provider_id, model_path, provider_model,
    system_prompt, system_prompt_type,
    sampler_type, temperature, top_p, top_k, mirostat_tau, mirostat_eta,
    repeat_penalty, min_p, typical_p, frequency_penalty, presence_penalty,
    penalty_last_n, dry_multiplier, dry_base, dry_allowed_length, dry_penalty_last_n,
    top_n_sigma, flash_attention, cache_type_k, cache_type_v, n_batch, context_size,
    seed, n_ubatch, n_threads, n_threads_batch, rope_freq_base, rope_freq_scale,
    use_mlock, use_mmap, main_gpu, split_mode,
    stop_tokens, tag_pairs,
    tool_tag_exec_open, tool_tag_exec_close, tool_tag_output_open, tool_tag_output_close,
    proactive_compaction, safe_tool_injection, thinking_mode,
    heartbeat_enabled, heartbeat_interval_minutes, heartbeat_prompt,
    created_at, updated_at
";

fn row_to_agent(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentRecord> {
    let stop_tokens_json: Option<String> = row.get(39)?;
    let stop_tokens = stop_tokens_json.and_then(|j| serde_json::from_str(&j).ok());
    Ok(AgentRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        provider_id: row
            .get::<_, Option<String>>(2)?
            .unwrap_or_else(|| "local".to_string()),
        model_path: row.get(3)?,
        provider_model: row.get(4)?,
        system_prompt: row.get(5)?,
        system_prompt_type: row
            .get::<_, Option<String>>(6)?
            .unwrap_or_else(|| "Custom".to_string()),
        sampler_type: row
            .get::<_, Option<String>>(7)?
            .unwrap_or_else(|| "Greedy".to_string()),
        temperature: row.get::<_, Option<f64>>(8)?.unwrap_or(0.7),
        top_p: row.get::<_, Option<f64>>(9)?.unwrap_or(0.95),
        top_k: row.get::<_, Option<u32>>(10)?.unwrap_or(20),
        mirostat_tau: row.get::<_, Option<f64>>(11)?.unwrap_or(5.0),
        mirostat_eta: row.get::<_, Option<f64>>(12)?.unwrap_or(0.1),
        repeat_penalty: row.get::<_, Option<f64>>(13)?.unwrap_or(1.0),
        min_p: row.get::<_, Option<f64>>(14)?.unwrap_or(0.0),
        typical_p: row.get::<_, Option<f64>>(15)?.unwrap_or(1.0),
        frequency_penalty: row.get::<_, Option<f64>>(16)?.unwrap_or(0.0),
        presence_penalty: row.get::<_, Option<f64>>(17)?.unwrap_or(0.0),
        penalty_last_n: row.get::<_, Option<i32>>(18)?.unwrap_or(64),
        dry_multiplier: row.get::<_, Option<f64>>(19)?.unwrap_or(0.0),
        dry_base: row.get::<_, Option<f64>>(20)?.unwrap_or(1.75),
        dry_allowed_length: row.get::<_, Option<i32>>(21)?.unwrap_or(2),
        dry_penalty_last_n: row.get::<_, Option<i32>>(22)?.unwrap_or(-1),
        top_n_sigma: row.get::<_, Option<f64>>(23)?.unwrap_or(-1.0),
        flash_attention: row.get::<_, Option<i32>>(24)?.unwrap_or(1) != 0,
        cache_type_k: row
            .get::<_, Option<String>>(25)?
            .unwrap_or_else(|| "f16".to_string()),
        cache_type_v: row
            .get::<_, Option<String>>(26)?
            .unwrap_or_else(|| "f16".to_string()),
        n_batch: row.get::<_, Option<u32>>(27)?.unwrap_or(2048),
        context_size: row.get(28)?,
        seed: row.get::<_, Option<i32>>(29)?.unwrap_or(-1),
        n_ubatch: row.get::<_, Option<u32>>(30)?.unwrap_or(512),
        n_threads: row.get::<_, Option<i32>>(31)?.unwrap_or(0),
        n_threads_batch: row.get::<_, Option<i32>>(32)?.unwrap_or(0),
        rope_freq_base: row.get::<_, Option<f64>>(33)?.unwrap_or(0.0) as f32,
        rope_freq_scale: row.get::<_, Option<f64>>(34)?.unwrap_or(0.0) as f32,
        use_mlock: row.get::<_, Option<i32>>(35)?.unwrap_or(0) != 0,
        use_mmap: row.get::<_, Option<i32>>(36)?.unwrap_or(1) != 0,
        main_gpu: row.get::<_, Option<i32>>(37)?.unwrap_or(0),
        split_mode: row
            .get::<_, Option<String>>(38)?
            .unwrap_or_else(|| "layer".to_string()),
        stop_tokens,
        tag_pairs: row.get(40)?,
        tool_tag_exec_open: row.get(41)?,
        tool_tag_exec_close: row.get(42)?,
        tool_tag_output_open: row.get(43)?,
        tool_tag_output_close: row.get(44)?,
        proactive_compaction: row.get::<_, Option<i32>>(45)?.unwrap_or(1) != 0,
        safe_tool_injection: row.get::<_, Option<i32>>(46)?.unwrap_or(0) != 0,
        thinking_mode: row.get::<_, Option<i32>>(47)?.map(|v| v != 0),
        heartbeat_enabled: row.get::<_, Option<i32>>(48)?.unwrap_or(0) != 0,
        heartbeat_interval_minutes: row.get::<_, Option<u32>>(49)?.unwrap_or(30),
        heartbeat_prompt: row.get(50)?,
        created_at: row.get(51)?,
        updated_at: row.get(52)?,
    })
}

impl Database {
    /// Create a new agent. Returns the new agent ID.
    pub fn create_agent(&self, agent: &AgentRecord) -> Result<String, String> {
        let conn = self.connection();
        let stop_tokens_json = agent
            .stop_tokens
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or_default());
        conn.execute(
            "INSERT INTO agents (
                id, name, provider_id, model_path, provider_model,
                system_prompt, system_prompt_type,
                sampler_type, temperature, top_p, top_k, mirostat_tau, mirostat_eta,
                repeat_penalty, min_p, typical_p, frequency_penalty, presence_penalty,
                penalty_last_n, dry_multiplier, dry_base, dry_allowed_length, dry_penalty_last_n,
                top_n_sigma, flash_attention, cache_type_k, cache_type_v, n_batch, context_size,
                seed, n_ubatch, n_threads, n_threads_batch, rope_freq_base, rope_freq_scale,
                use_mlock, use_mmap, main_gpu, split_mode,
                stop_tokens, tag_pairs,
                tool_tag_exec_open, tool_tag_exec_close, tool_tag_output_open, tool_tag_output_close,
                proactive_compaction, safe_tool_injection, thinking_mode,
                heartbeat_enabled, heartbeat_interval_minutes, heartbeat_prompt,
                created_at, updated_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29,
                ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37, ?38, ?39, ?40, ?41, ?42, ?43,
                ?44, ?45, ?46, ?47, ?48, ?49, ?50, ?51, ?52, ?53
            )",
            params![
                agent.id, agent.name, agent.provider_id, agent.model_path, agent.provider_model,
                agent.system_prompt, agent.system_prompt_type,
                agent.sampler_type, agent.temperature, agent.top_p, agent.top_k,
                agent.mirostat_tau, agent.mirostat_eta, agent.repeat_penalty, agent.min_p,
                agent.typical_p, agent.frequency_penalty, agent.presence_penalty,
                agent.penalty_last_n, agent.dry_multiplier, agent.dry_base,
                agent.dry_allowed_length, agent.dry_penalty_last_n, agent.top_n_sigma,
                agent.flash_attention as i32,
                agent.cache_type_k, agent.cache_type_v, agent.n_batch, agent.context_size,
                agent.seed, agent.n_ubatch, agent.n_threads, agent.n_threads_batch,
                agent.rope_freq_base as f64, agent.rope_freq_scale as f64,
                agent.use_mlock as i32, agent.use_mmap as i32, agent.main_gpu, agent.split_mode,
                stop_tokens_json, agent.tag_pairs,
                agent.tool_tag_exec_open, agent.tool_tag_exec_close,
                agent.tool_tag_output_open, agent.tool_tag_output_close,
                agent.proactive_compaction as i32, agent.safe_tool_injection as i32,
                agent.thinking_mode.map(|v| v as i32),
                agent.heartbeat_enabled as i32, agent.heartbeat_interval_minutes,
                agent.heartbeat_prompt,
                agent.created_at, agent.updated_at,
            ],
        )
        .map_err(db_error("create agent"))?;
        Ok(agent.id.clone())
    }

    /// Get an agent by ID. Returns None if not found.
    pub fn get_agent(&self, id: &str) -> Result<Option<AgentRecord>, String> {
        let conn = self.connection();
        let result = conn.query_row(
            &format!("SELECT {SELECT_AGENT_COLS} FROM agents WHERE id = ?1"),
            [id],
            row_to_agent,
        );
        match result {
            Ok(agent) => Ok(Some(agent)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("Failed to get agent: {e}")),
        }
    }

    /// List all agents ordered by creation time (newest first).
    pub fn list_agents(&self) -> Result<Vec<AgentRecord>, String> {
        let conn = self.connection();
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {SELECT_AGENT_COLS} FROM agents ORDER BY created_at DESC"
            ))
            .map_err(db_error("prepare list agents"))?;
        let agents = stmt
            .query_map([], row_to_agent)
            .map_err(db_error("query agents"))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(agents)
    }

    /// Update an existing agent. Returns error if not found.
    pub fn update_agent(&self, agent: &AgentRecord) -> Result<(), String> {
        let conn = self.connection();
        let stop_tokens_json = agent
            .stop_tokens
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or_default());
        let now = current_timestamp_millis();
        let changed = conn
            .execute(
                "UPDATE agents SET
                name = ?1, provider_id = ?2, model_path = ?3, provider_model = ?4,
                system_prompt = ?5, system_prompt_type = ?6,
                sampler_type = ?7, temperature = ?8, top_p = ?9, top_k = ?10,
                mirostat_tau = ?11, mirostat_eta = ?12, repeat_penalty = ?13, min_p = ?14,
                typical_p = ?15, frequency_penalty = ?16, presence_penalty = ?17,
                penalty_last_n = ?18, dry_multiplier = ?19, dry_base = ?20,
                dry_allowed_length = ?21, dry_penalty_last_n = ?22, top_n_sigma = ?23,
                flash_attention = ?24, cache_type_k = ?25, cache_type_v = ?26,
                n_batch = ?27, context_size = ?28, seed = ?29, n_ubatch = ?30,
                n_threads = ?31, n_threads_batch = ?32, rope_freq_base = ?33, rope_freq_scale = ?34,
                use_mlock = ?35, use_mmap = ?36, main_gpu = ?37, split_mode = ?38,
                stop_tokens = ?39, tag_pairs = ?40,
                tool_tag_exec_open = ?41, tool_tag_exec_close = ?42,
                tool_tag_output_open = ?43, tool_tag_output_close = ?44,
                proactive_compaction = ?45, safe_tool_injection = ?46, thinking_mode = ?47,
                heartbeat_enabled = ?48, heartbeat_interval_minutes = ?49, heartbeat_prompt = ?50,
                updated_at = ?51
             WHERE id = ?52",
                params![
                    agent.name,
                    agent.provider_id,
                    agent.model_path,
                    agent.provider_model,
                    agent.system_prompt,
                    agent.system_prompt_type,
                    agent.sampler_type,
                    agent.temperature,
                    agent.top_p,
                    agent.top_k,
                    agent.mirostat_tau,
                    agent.mirostat_eta,
                    agent.repeat_penalty,
                    agent.min_p,
                    agent.typical_p,
                    agent.frequency_penalty,
                    agent.presence_penalty,
                    agent.penalty_last_n,
                    agent.dry_multiplier,
                    agent.dry_base,
                    agent.dry_allowed_length,
                    agent.dry_penalty_last_n,
                    agent.top_n_sigma,
                    agent.flash_attention as i32,
                    agent.cache_type_k,
                    agent.cache_type_v,
                    agent.n_batch,
                    agent.context_size,
                    agent.seed,
                    agent.n_ubatch,
                    agent.n_threads,
                    agent.n_threads_batch,
                    agent.rope_freq_base as f64,
                    agent.rope_freq_scale as f64,
                    agent.use_mlock as i32,
                    agent.use_mmap as i32,
                    agent.main_gpu,
                    agent.split_mode,
                    stop_tokens_json,
                    agent.tag_pairs,
                    agent.tool_tag_exec_open,
                    agent.tool_tag_exec_close,
                    agent.tool_tag_output_open,
                    agent.tool_tag_output_close,
                    agent.proactive_compaction as i32,
                    agent.safe_tool_injection as i32,
                    agent.thinking_mode.map(|v| v as i32),
                    agent.heartbeat_enabled as i32,
                    agent.heartbeat_interval_minutes,
                    agent.heartbeat_prompt,
                    now,
                    agent.id,
                ],
            )
            .map_err(db_error("update agent"))?;
        if changed == 0 {
            return Err(format!("Agent {} not found", agent.id));
        }
        Ok(())
    }

    /// Delete an agent. Conversations referencing it will have agent_id set to NULL.
    pub fn delete_agent(&self, id: &str) -> Result<(), String> {
        let conn = self.connection();
        conn.execute(
            "UPDATE conversations SET agent_id = NULL WHERE agent_id = ?1",
            [id],
        )
        .map_err(db_error("unlink conversations from agent"))?;
        conn.execute("DELETE FROM agents WHERE id = ?1", [id])
            .map_err(db_error("delete agent"))?;
        Ok(())
    }

    // ─── Conversation ↔ Agent binding ───────────────────────────────────────

    /// Get the agent_id assigned to a conversation (None = no agent set).
    pub fn get_conversation_agent_id(
        &self,
        conversation_id: &str,
    ) -> Result<Option<String>, String> {
        let conn = self.connection();
        let result = conn.query_row(
            "SELECT agent_id FROM conversations WHERE id = ?1",
            [conversation_id],
            |row| row.get::<_, Option<String>>(0),
        );
        match result {
            Ok(id) => Ok(id),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("Failed to get conversation agent id: {e}")),
        }
    }

    /// List all conversation IDs that have the given agent assigned.
    pub fn list_conversation_ids_by_agent(
        &self,
        agent_id: &str,
    ) -> Result<Vec<String>, String> {
        let conn = self.connection();
        let mut stmt = conn
            .prepare("SELECT id FROM conversations WHERE agent_id = ?1")
            .map_err(db_error("prepare list conversations by agent"))?;
        let rows = stmt
            .query_map([agent_id], |row| row.get::<_, String>(0))
            .map_err(db_error("query list conversations by agent"))?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row.map_err(db_error("read conversation id"))?);
        }
        Ok(ids)
    }

    /// Assign (or clear) an agent on a conversation.
    pub fn set_conversation_agent_id(
        &self,
        conversation_id: &str,
        agent_id: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.connection();
        conn.execute(
            "UPDATE conversations SET agent_id = ?1 WHERE id = ?2",
            params![agent_id, conversation_id],
        )
        .map_err(db_error("set conversation agent id"))?;
        Ok(())
    }

    // ─── Effective config resolution ────────────────────────────────────────

    /// Load the effective config for a conversation.
    ///
    /// Resolution order:
    /// 1. If conversation has an `agent_id`, use that agent's config.
    /// 2. Final fallback: global config.
    pub fn load_effective_config(&self, conversation_id: &str) -> DbSamplerConfig {
        if let Ok(Some(agent_id)) = self.get_conversation_agent_id(conversation_id) {
            if let Ok(Some(agent)) = self.get_agent(&agent_id) {
                let global = self.load_config();
                return agent.to_db_sampler_config(&global);
            }
        }
        self.load_config()
    }

    /// Load config for a specific agent, merging agent fields with global app-level fields.
    pub fn load_config_for_agent(&self, agent_id: &str) -> DbSamplerConfig {
        if let Ok(Some(agent)) = self.get_agent(agent_id) {
            let global = self.load_config();
            return agent.to_db_sampler_config(&global);
        }
        self.load_config()
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn test_db() -> Arc<Database> {
        Arc::new(Database::new(":memory:").unwrap())
    }

    #[test]
    fn test_create_and_get_agent() {
        let db = test_db();
        let agent = AgentRecord::from_db_sampler_config("Test Agent", &DbSamplerConfig::default());
        let id = db.create_agent(&agent).unwrap();
        let loaded = db.get_agent(&id).unwrap().unwrap();
        assert_eq!(loaded.name, "Test Agent");
        assert_eq!(loaded.temperature, 0.7);
    }

    #[test]
    fn test_list_agents() {
        let db = test_db();
        assert_eq!(db.list_agents().unwrap().len(), 0);
        let a1 = AgentRecord::from_db_sampler_config("A", &DbSamplerConfig::default());
        let a2 = AgentRecord::from_db_sampler_config("B", &DbSamplerConfig::default());
        db.create_agent(&a1).unwrap();
        db.create_agent(&a2).unwrap();
        assert_eq!(db.list_agents().unwrap().len(), 2);
    }

    #[test]
    fn test_update_agent() {
        let db = test_db();
        let mut agent = AgentRecord::from_db_sampler_config("Agent", &DbSamplerConfig::default());
        db.create_agent(&agent).unwrap();
        agent.name = "Updated".to_string();
        agent.temperature = 1.2;
        db.update_agent(&agent).unwrap();
        let loaded = db.get_agent(&agent.id).unwrap().unwrap();
        assert_eq!(loaded.name, "Updated");
        assert_eq!(loaded.temperature, 1.2);
    }

    #[test]
    fn test_delete_agent_unlinks_conversations() {
        let db = test_db();
        let conv_id = db.create_conversation().unwrap();
        let agent = AgentRecord::from_db_sampler_config("A", &DbSamplerConfig::default());
        db.create_agent(&agent).unwrap();
        db.set_conversation_agent_id(&conv_id, Some(&agent.id))
            .unwrap();
        assert_eq!(
            db.get_conversation_agent_id(&conv_id).unwrap(),
            Some(agent.id.clone())
        );
        db.delete_agent(&agent.id).unwrap();
        assert_eq!(db.get_conversation_agent_id(&conv_id).unwrap(), None);
    }

    #[test]
    fn test_load_effective_config_agent_path() {
        let db = test_db();
        let conv_id = db.create_conversation().unwrap();
        let mut config = DbSamplerConfig::default();
        config.temperature = 1.5;
        let agent = AgentRecord::from_db_sampler_config("Hot", &config);
        db.create_agent(&agent).unwrap();
        db.set_conversation_agent_id(&conv_id, Some(&agent.id))
            .unwrap();
        let eff = db.load_effective_config(&conv_id);
        assert_eq!(eff.temperature, 1.5);
    }

    #[test]
    fn test_load_effective_config_fallback() {
        let db = test_db();
        let conv_id = db.create_conversation().unwrap();
        // No agent set — falls back to global
        let eff = db.load_effective_config(&conv_id);
        assert!(eff.temperature >= 0.0);
    }
}
