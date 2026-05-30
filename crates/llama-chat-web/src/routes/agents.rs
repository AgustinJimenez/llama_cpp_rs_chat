// Agent CRUD route handlers
//
// Agents are named, reusable model+config presets. A conversation references
// one agent by ID; per-conversation tweaks are stored as sparse JSON overrides.

use hyper::{Body, Request, Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;

use llama_chat_db::agents::AgentRecord;
use llama_chat_db::{current_timestamp_millis, SharedDatabase};
use uuid::Uuid;

use crate::request_parsing::parse_json_body;
use crate::response_helpers::{json_error, json_raw, json_success};
#[cfg(not(feature = "mock"))]
use crate::worker_pool::WorkerPool;

// ─── JSON DTOs ───────────────────────────────────────────────────────────────

/// The full agent as sent to/from the frontend.
/// All param fields are optional on create/update — absent fields use defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    // Provider / model
    #[serde(default = "default_local")]
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_model: Option<String>,
    // System prompt
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt_type: Option<String>,
    // Sampler
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampler_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mirostat_tau: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mirostat_eta: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typical_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penalty_last_n: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_multiplier: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_base: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_allowed_length: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_penalty_last_n: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_n_sigma: Option<f64>,
    // Hardware / context
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flash_attention: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_type_k: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_type_v: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n_batch: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_size: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n_ubatch: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n_threads: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n_threads_batch: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rope_freq_base: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rope_freq_scale: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_mlock: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_mmap: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_gpu: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split_mode: Option<String>,
    // Generation settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_tokens: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_pairs: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_tag_exec_open: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_tag_exec_close: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_tag_output_open: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_tag_output_close: Option<String>,
    // Behavior flags
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proactive_compaction: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe_tool_injection: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_mode: Option<bool>,
    // Heartbeat
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heartbeat_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heartbeat_interval_minutes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heartbeat_prompt: Option<String>,
    // Read-only timestamps (set by server, ignored on write)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

fn default_local() -> String {
    "local".to_string()
}

impl AgentJson {
    /// Build an AgentRecord from this DTO. Fills in ID + timestamps.
    /// Use for CREATE — caller must not supply id (it's generated here).
    pub fn into_record(self) -> AgentRecord {
        let now = current_timestamp_millis();
        AgentRecord {
            id: format!("agent_{}", Uuid::new_v4().simple()),
            name: self.name,
            provider_id: self.provider_id,
            model_path: self.model_path,
            provider_model: self.provider_model,
            system_prompt: self.system_prompt,
            system_prompt_type: self
                .system_prompt_type
                .unwrap_or_else(|| "Custom".to_string()),
            sampler_type: self.sampler_type.unwrap_or_else(|| "Greedy".to_string()),
            temperature: self.temperature.unwrap_or(0.7),
            top_p: self.top_p.unwrap_or(0.95),
            top_k: self.top_k.unwrap_or(20),
            mirostat_tau: self.mirostat_tau.unwrap_or(5.0),
            mirostat_eta: self.mirostat_eta.unwrap_or(0.1),
            repeat_penalty: self.repeat_penalty.unwrap_or(1.0),
            min_p: self.min_p.unwrap_or(0.0),
            typical_p: self.typical_p.unwrap_or(1.0),
            frequency_penalty: self.frequency_penalty.unwrap_or(0.0),
            presence_penalty: self.presence_penalty.unwrap_or(0.0),
            penalty_last_n: self.penalty_last_n.unwrap_or(64),
            dry_multiplier: self.dry_multiplier.unwrap_or(0.0),
            dry_base: self.dry_base.unwrap_or(1.75),
            dry_allowed_length: self.dry_allowed_length.unwrap_or(2),
            dry_penalty_last_n: self.dry_penalty_last_n.unwrap_or(-1),
            top_n_sigma: self.top_n_sigma.unwrap_or(-1.0),
            flash_attention: self.flash_attention.unwrap_or(true),
            cache_type_k: self.cache_type_k.unwrap_or_else(|| "f16".to_string()),
            cache_type_v: self.cache_type_v.unwrap_or_else(|| "f16".to_string()),
            n_batch: self.n_batch.unwrap_or(2048),
            context_size: self.context_size,
            seed: self.seed.unwrap_or(-1),
            n_ubatch: self.n_ubatch.unwrap_or(512),
            n_threads: self.n_threads.unwrap_or(0),
            n_threads_batch: self.n_threads_batch.unwrap_or(0),
            rope_freq_base: self.rope_freq_base.unwrap_or(0.0),
            rope_freq_scale: self.rope_freq_scale.unwrap_or(0.0),
            use_mlock: self.use_mlock.unwrap_or(false),
            use_mmap: self.use_mmap.unwrap_or(true),
            main_gpu: self.main_gpu.unwrap_or(0),
            split_mode: self.split_mode.unwrap_or_else(|| "layer".to_string()),
            stop_tokens: self.stop_tokens,
            tag_pairs: self.tag_pairs,
            tool_tag_exec_open: self.tool_tag_exec_open,
            tool_tag_exec_close: self.tool_tag_exec_close,
            tool_tag_output_open: self.tool_tag_output_open,
            tool_tag_output_close: self.tool_tag_output_close,
            proactive_compaction: self.proactive_compaction.unwrap_or(true),
            safe_tool_injection: self.safe_tool_injection.unwrap_or(false),
            thinking_mode: self.thinking_mode,
            heartbeat_enabled: self.heartbeat_enabled.unwrap_or(false),
            heartbeat_interval_minutes: self.heartbeat_interval_minutes.unwrap_or(30),
            heartbeat_prompt: self.heartbeat_prompt,
            created_at: now,
            updated_at: now,
        }
    }

    /// Merge this DTO onto an existing AgentRecord (for PUT updates).
    /// Only Some fields overwrite; None fields keep the existing value.
    pub fn apply_to(self, existing: &mut AgentRecord) {
        existing.name = self.name;
        existing.provider_id = self.provider_id;
        existing.model_path = self.model_path;
        existing.provider_model = self.provider_model;
        existing.system_prompt = self.system_prompt;
        if let Some(v) = self.system_prompt_type {
            existing.system_prompt_type = v;
        }
        if let Some(v) = self.sampler_type {
            existing.sampler_type = v;
        }
        if let Some(v) = self.temperature {
            existing.temperature = v;
        }
        if let Some(v) = self.top_p {
            existing.top_p = v;
        }
        if let Some(v) = self.top_k {
            existing.top_k = v;
        }
        if let Some(v) = self.mirostat_tau {
            existing.mirostat_tau = v;
        }
        if let Some(v) = self.mirostat_eta {
            existing.mirostat_eta = v;
        }
        if let Some(v) = self.repeat_penalty {
            existing.repeat_penalty = v;
        }
        if let Some(v) = self.min_p {
            existing.min_p = v;
        }
        if let Some(v) = self.typical_p {
            existing.typical_p = v;
        }
        if let Some(v) = self.frequency_penalty {
            existing.frequency_penalty = v;
        }
        if let Some(v) = self.presence_penalty {
            existing.presence_penalty = v;
        }
        if let Some(v) = self.penalty_last_n {
            existing.penalty_last_n = v;
        }
        if let Some(v) = self.dry_multiplier {
            existing.dry_multiplier = v;
        }
        if let Some(v) = self.dry_base {
            existing.dry_base = v;
        }
        if let Some(v) = self.dry_allowed_length {
            existing.dry_allowed_length = v;
        }
        if let Some(v) = self.dry_penalty_last_n {
            existing.dry_penalty_last_n = v;
        }
        if let Some(v) = self.top_n_sigma {
            existing.top_n_sigma = v;
        }
        if let Some(v) = self.flash_attention {
            existing.flash_attention = v;
        }
        if let Some(v) = self.cache_type_k {
            existing.cache_type_k = v;
        }
        if let Some(v) = self.cache_type_v {
            existing.cache_type_v = v;
        }
        if let Some(v) = self.n_batch {
            existing.n_batch = v;
        }
        existing.context_size = self.context_size;
        if let Some(v) = self.seed {
            existing.seed = v;
        }
        if let Some(v) = self.n_ubatch {
            existing.n_ubatch = v;
        }
        if let Some(v) = self.n_threads {
            existing.n_threads = v;
        }
        if let Some(v) = self.n_threads_batch {
            existing.n_threads_batch = v;
        }
        if let Some(v) = self.rope_freq_base {
            existing.rope_freq_base = v;
        }
        if let Some(v) = self.rope_freq_scale {
            existing.rope_freq_scale = v;
        }
        if let Some(v) = self.use_mlock {
            existing.use_mlock = v;
        }
        if let Some(v) = self.use_mmap {
            existing.use_mmap = v;
        }
        if let Some(v) = self.main_gpu {
            existing.main_gpu = v;
        }
        if let Some(v) = self.split_mode {
            existing.split_mode = v;
        }
        existing.stop_tokens = self.stop_tokens;
        existing.tag_pairs = self.tag_pairs;
        existing.tool_tag_exec_open = self.tool_tag_exec_open;
        existing.tool_tag_exec_close = self.tool_tag_exec_close;
        existing.tool_tag_output_open = self.tool_tag_output_open;
        existing.tool_tag_output_close = self.tool_tag_output_close;
        if let Some(v) = self.proactive_compaction {
            existing.proactive_compaction = v;
        }
        if let Some(v) = self.safe_tool_injection {
            existing.safe_tool_injection = v;
        }
        existing.thinking_mode = self.thinking_mode;
        if let Some(v) = self.heartbeat_enabled {
            existing.heartbeat_enabled = v;
        }
        if let Some(v) = self.heartbeat_interval_minutes {
            existing.heartbeat_interval_minutes = v;
        }
        existing.heartbeat_prompt = self.heartbeat_prompt;
    }
}

impl From<AgentRecord> for AgentJson {
    fn from(r: AgentRecord) -> Self {
        Self {
            id: Some(r.id),
            name: r.name,
            provider_id: r.provider_id,
            model_path: r.model_path,
            provider_model: r.provider_model,
            system_prompt: r.system_prompt,
            system_prompt_type: Some(r.system_prompt_type),
            sampler_type: Some(r.sampler_type),
            temperature: Some(r.temperature),
            top_p: Some(r.top_p),
            top_k: Some(r.top_k),
            mirostat_tau: Some(r.mirostat_tau),
            mirostat_eta: Some(r.mirostat_eta),
            repeat_penalty: Some(r.repeat_penalty),
            min_p: Some(r.min_p),
            typical_p: Some(r.typical_p),
            frequency_penalty: Some(r.frequency_penalty),
            presence_penalty: Some(r.presence_penalty),
            penalty_last_n: Some(r.penalty_last_n),
            dry_multiplier: Some(r.dry_multiplier),
            dry_base: Some(r.dry_base),
            dry_allowed_length: Some(r.dry_allowed_length),
            dry_penalty_last_n: Some(r.dry_penalty_last_n),
            top_n_sigma: Some(r.top_n_sigma),
            flash_attention: Some(r.flash_attention),
            cache_type_k: Some(r.cache_type_k),
            cache_type_v: Some(r.cache_type_v),
            n_batch: Some(r.n_batch),
            context_size: r.context_size,
            seed: Some(r.seed),
            n_ubatch: Some(r.n_ubatch),
            n_threads: Some(r.n_threads),
            n_threads_batch: Some(r.n_threads_batch),
            rope_freq_base: Some(r.rope_freq_base),
            rope_freq_scale: Some(r.rope_freq_scale),
            use_mlock: Some(r.use_mlock),
            use_mmap: Some(r.use_mmap),
            main_gpu: Some(r.main_gpu),
            split_mode: Some(r.split_mode),
            stop_tokens: r.stop_tokens,
            tag_pairs: r.tag_pairs,
            tool_tag_exec_open: r.tool_tag_exec_open,
            tool_tag_exec_close: r.tool_tag_exec_close,
            tool_tag_output_open: r.tool_tag_output_open,
            tool_tag_output_close: r.tool_tag_output_close,
            proactive_compaction: Some(r.proactive_compaction),
            safe_tool_injection: Some(r.safe_tool_injection),
            thinking_mode: r.thinking_mode,
            heartbeat_enabled: Some(r.heartbeat_enabled),
            heartbeat_interval_minutes: Some(r.heartbeat_interval_minutes),
            heartbeat_prompt: r.heartbeat_prompt,
            created_at: Some(r.created_at),
            updated_at: Some(r.updated_at),
        }
    }
}

// ─── Handlers ────────────────────────────────────────────────────────────────

/// GET /api/agents — list all agents (newest first)
pub async fn handle_list_agents(db: SharedDatabase) -> Result<Response<Body>, Infallible> {
    match db.list_agents() {
        Ok(agents) => {
            let json_list: Vec<AgentJson> = agents.into_iter().map(AgentJson::from).collect();
            match serde_json::to_string(&json_list) {
                Ok(json) => Ok(json_raw(StatusCode::OK, json)),
                Err(e) => Ok(json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("Serialize error: {e}"),
                )),
            }
        }
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

/// GET /api/agents/:id — get a single agent
pub async fn handle_get_agent(id: &str, db: SharedDatabase) -> Result<Response<Body>, Infallible> {
    match db.get_agent(id) {
        Ok(Some(agent)) => {
            let json = AgentJson::from(agent);
            match serde_json::to_string(&json) {
                Ok(s) => Ok(json_raw(StatusCode::OK, s)),
                Err(e) => Ok(json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("Serialize error: {e}"),
                )),
            }
        }
        Ok(None) => Ok(json_error(StatusCode::NOT_FOUND, "Agent not found")),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

/// POST /api/agents — create a new agent
pub async fn handle_create_agent(
    req: Request<Body>,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let dto: AgentJson = match parse_json_body(req.into_body()).await {
        Ok(v) => v,
        Err(err_resp) => return Ok(err_resp),
    };

    if dto.name.trim().is_empty() {
        return Ok(json_error(StatusCode::BAD_REQUEST, "name is required"));
    }

    let record = dto.into_record();
    let id = record.id.clone();

    match db.create_agent(&record) {
        Ok(_) => {
            // Return the created agent (with server-assigned id + timestamps)
            match db.get_agent(&id) {
                Ok(Some(agent)) => {
                    let json = AgentJson::from(agent);
                    match serde_json::to_string(&json) {
                        Ok(s) => Ok(json_raw(StatusCode::CREATED, s)),
                        Err(e) => Ok(json_error(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            &format!("Serialize error: {e}"),
                        )),
                    }
                }
                _ => Ok(json_raw(StatusCode::CREATED, format!(r#"{{"id":"{id}"}}"#))),
            }
        }
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

/// PUT /api/agents/:id — replace an agent
pub async fn handle_update_agent(
    req: Request<Body>,
    id: &str,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let dto: AgentJson = match parse_json_body(req.into_body()).await {
        Ok(v) => v,
        Err(err_resp) => return Ok(err_resp),
    };

    if dto.name.trim().is_empty() {
        return Ok(json_error(StatusCode::BAD_REQUEST, "name is required"));
    }

    let mut existing = match db.get_agent(id) {
        Ok(Some(a)) => a,
        Ok(None) => return Ok(json_error(StatusCode::NOT_FOUND, "Agent not found")),
        Err(e) => return Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    };

    dto.apply_to(&mut existing);

    match db.update_agent(&existing) {
        Ok(()) => Ok(json_success("Agent updated")),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

/// DELETE /api/agents/:id — delete an agent (unlinks conversations)
pub async fn handle_delete_agent(
    id: &str,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    match db.delete_agent(id) {
        Ok(()) => Ok(json_success("Agent deleted")),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

/// POST /api/conversations/:id/agent — assign or clear an agent on a conversation
///
/// Body: `{"agent_id": "agent_xxx"}` to assign, `{"agent_id": null}` to clear
pub async fn handle_set_conversation_agent(
    req: Request<Body>,
    conversation_id: &str,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    #[derive(Deserialize)]
    struct AgentBody {
        agent_id: Option<String>,
    }

    let body: AgentBody = match parse_json_body(req.into_body()).await {
        Ok(v) => v,
        Err(err_resp) => return Ok(err_resp),
    };

    if let Some(ref aid) = body.agent_id {
        match db.get_agent(aid) {
            Ok(None) => return Ok(json_error(StatusCode::NOT_FOUND, "Agent not found")),
            Err(e) => return Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
            Ok(Some(_)) => {}
        }
    }

    match db.set_conversation_agent_id(conversation_id, body.agent_id.as_deref()) {
        Ok(()) => Ok(json_success("Conversation agent updated")),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

/// PATCH /api/conversations/:id/overrides — update per-conversation param overrides
///
/// Body: sparse JSON object of params to override (e.g. `{"temperature": 0.9}`).
/// Send `null` body to clear all overrides.
pub async fn handle_set_conversation_overrides(
    req: Request<Body>,
    conversation_id: &str,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let body_bytes = match hyper::body::to_bytes(req.into_body()).await {
        Ok(b) => b,
        Err(_) => return Ok(json_error(StatusCode::BAD_REQUEST, "Failed to read body")),
    };
    let body_str = std::str::from_utf8(&body_bytes).unwrap_or("null");

    // Validate it's valid JSON (either null or an object)
    let overrides_json: serde_json::Value = match serde_json::from_str(body_str) {
        Ok(v) => v,
        Err(_) => {
            return Ok(json_error(
                StatusCode::BAD_REQUEST,
                "Body must be valid JSON",
            ))
        }
    };

    let overrides = if overrides_json.is_null() {
        None
    } else if overrides_json.is_object() {
        Some(overrides_json.to_string())
    } else {
        return Ok(json_error(
            StatusCode::BAD_REQUEST,
            "Body must be a JSON object or null",
        ));
    };

    match db.set_conversation_overrides(conversation_id, overrides.as_deref()) {
        Ok(()) => Ok(json_success("Overrides saved")),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

/// GET /api/conversations/:id/overrides — get raw sparse overrides for a conversation
pub async fn handle_get_conversation_overrides(
    conversation_id: &str,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let overrides = db
        .get_conversation_overrides(conversation_id)
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .unwrap_or(serde_json::Value::Null);

    match serde_json::to_string(&serde_json::json!({ "overrides": overrides })) {
        Ok(s) => Ok(json_raw(StatusCode::OK, s)),
        Err(e) => Ok(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Serialize error: {e}"),
        )),
    }
}

/// GET /api/conversations/:id/agent — get the active agent for a conversation
pub async fn handle_get_conversation_agent(
    conversation_id: &str,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let agent_id = match db.get_conversation_agent_id(conversation_id) {
        Ok(id) => id,
        Err(e) => return Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    };

    let agent = match agent_id {
        None => {
            return Ok(json_raw(StatusCode::OK, r#"{"agent":null}"#.to_string()));
        }
        Some(ref id) => match db.get_agent(id) {
            Ok(Some(a)) => a,
            Ok(None) => return Ok(json_raw(StatusCode::OK, r#"{"agent":null}"#.to_string())),
            Err(e) => return Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
        },
    };

    let json = AgentJson::from(agent);
    match serde_json::to_string(&serde_json::json!({ "agent": json })) {
        Ok(s) => Ok(json_raw(StatusCode::OK, s)),
        Err(e) => Ok(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Serialize error: {e}"),
        )),
    }
}

// ─── Agent lifecycle: activate / stop / statuses ─────────────────────────────

/// POST /api/agents/:id/activate
///
/// For local agents: spawns a dedicated worker process and loads the model.
/// For remote agents: no-op (remote providers are stateless).
/// Returns `{"status": "active"|"idle", "worker_id": "..."}`.
#[cfg(not(feature = "mock"))]
pub async fn handle_activate_agent(
    id: &str,
    pool: WorkerPool,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let agent = match db.get_agent(id) {
        Ok(Some(a)) => a,
        Ok(None) => return Ok(json_error(StatusCode::NOT_FOUND, "Agent not found")),
        Err(e) => return Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    };

    if agent.provider_id != "local" {
        // Remote/CLI agents don't need a worker process.
        let s = serde_json::json!({ "status": "active" }).to_string();
        return Ok(json_raw(StatusCode::OK, s));
    }

    // Already activated?
    if let Some(existing_worker_id) = pool.get_worker_for_agent(id) {
        let s = serde_json::json!({ "status": "active", "worker_id": existing_worker_id }).to_string();
        return Ok(json_raw(StatusCode::OK, s));
    }

    let Some(model_path) = agent.model_path.as_deref().map(str::trim).filter(|p| !p.is_empty()) else {
        return Ok(json_error(StatusCode::BAD_REQUEST, "Agent has no model_path configured"));
    };

    let gpu_layers = u32::try_from(agent.main_gpu).ok().filter(|&l| l > 0);

    match pool.spawn_worker_for_agent(id, model_path, gpu_layers, None).await {
        Ok(worker_id) => {
            let s = serde_json::json!({ "status": "active", "worker_id": worker_id }).to_string();
            Ok(json_raw(StatusCode::CREATED, s))
        }
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

/// POST /api/agents/:id/stop
///
/// Stops the dedicated worker for a local agent and releases its resources.
#[cfg(not(feature = "mock"))]
pub async fn handle_stop_agent(
    id: &str,
    pool: WorkerPool,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let agent = match db.get_agent(id) {
        Ok(Some(a)) => a,
        Ok(None) => return Ok(json_error(StatusCode::NOT_FOUND, "Agent not found")),
        Err(e) => return Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    };

    if agent.provider_id != "local" {
        return Ok(json_success("Remote agents do not have a worker to stop"));
    }

    match pool.stop_agent_worker(id).await {
        Ok(()) => Ok(json_success("Agent stopped")),
        Err(e) => Ok(json_error(StatusCode::BAD_REQUEST, &e)),
    }
}

/// GET /api/agents/statuses
///
/// Returns a map of agent_id → `{status, worker_id?}` for all agents.
/// Status values: `"idle"` | `"active"` | `"generating"`.
#[cfg(not(feature = "mock"))]
pub async fn handle_get_agent_statuses(
    pool: WorkerPool,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let agents = match db.list_agents() {
        Ok(a) => a,
        Err(e) => return Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    };

    let mut statuses = serde_json::Map::new();

    for agent in &agents {
        let entry = if agent.provider_id != "local" {
            // Remote agents are always "active" (no dedicated process needed).
            serde_json::json!({ "status": "active" })
        } else if let Some(worker_id) = pool.get_worker_for_agent(&agent.id) {
            let is_generating = match pool.get(&worker_id) {
                Some(bridge) => bridge.is_generating().await,
                None => false,
            };

            let status = if is_generating { "generating" } else { "active" };
            serde_json::json!({ "status": status, "worker_id": worker_id })
        } else {
            serde_json::json!({ "status": "idle" })
        };

        statuses.insert(agent.id.clone(), entry);
    }

    match serde_json::to_string(&statuses) {
        Ok(s) => Ok(json_raw(StatusCode::OK, s)),
        Err(e) => Ok(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Serialize error: {e}"),
        )),
    }
}
