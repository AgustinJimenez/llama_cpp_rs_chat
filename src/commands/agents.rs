use log::error;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::web::database::SharedDatabase;
use crate::web::worker_pool::WorkerPool;

/// Receive a JSON agent payload from the frontend (same shape as AgentJson in routes/agents.rs).
/// We use a minimal DTO here so we don't couple Tauri commands to the web route types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPayload {
    pub name: String,
    pub provider_id: Option<String>,
    pub model_path: Option<String>,
    pub provider_model: Option<String>,
    pub system_prompt: Option<String>,
    pub system_prompt_type: Option<String>,
    pub sampler_type: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<u32>,
    pub mirostat_tau: Option<f64>,
    pub mirostat_eta: Option<f64>,
    pub repeat_penalty: Option<f64>,
    pub min_p: Option<f64>,
    pub typical_p: Option<f64>,
    pub frequency_penalty: Option<f64>,
    pub presence_penalty: Option<f64>,
    pub penalty_last_n: Option<i32>,
    pub dry_multiplier: Option<f64>,
    pub dry_base: Option<f64>,
    pub dry_allowed_length: Option<i32>,
    pub dry_penalty_last_n: Option<i32>,
    pub top_n_sigma: Option<f64>,
    pub flash_attention: Option<bool>,
    pub cache_type_k: Option<String>,
    pub cache_type_v: Option<String>,
    pub n_batch: Option<u32>,
    pub context_size: Option<u32>,
    pub seed: Option<i32>,
    pub n_ubatch: Option<u32>,
    pub n_threads: Option<i32>,
    pub n_threads_batch: Option<i32>,
    pub rope_freq_base: Option<f32>,
    pub rope_freq_scale: Option<f32>,
    pub use_mlock: Option<bool>,
    pub use_mmap: Option<bool>,
    pub main_gpu: Option<i32>,
    pub split_mode: Option<String>,
    pub stop_tokens: Option<Vec<String>>,
    pub tag_pairs: Option<String>,
    pub tool_tag_exec_open: Option<String>,
    pub tool_tag_exec_close: Option<String>,
    pub tool_tag_output_open: Option<String>,
    pub tool_tag_output_close: Option<String>,
    pub proactive_compaction: Option<bool>,
    pub safe_tool_injection: Option<bool>,
    pub thinking_mode: Option<bool>,
    pub heartbeat_enabled: Option<bool>,
    pub heartbeat_interval_minutes: Option<u32>,
    pub heartbeat_prompt: Option<String>,
}

/// Build an AgentRecord from payload + generated ID + timestamps.
fn payload_to_record(p: AgentPayload) -> llama_chat_db::agents::AgentRecord {
    let now = llama_chat_db::current_timestamp_millis();
    use llama_chat_db::agents::AgentRecord;
    AgentRecord {
        id: format!("agent_{}", uuid::Uuid::new_v4().simple()),
        name: p.name,
        provider_id: p.provider_id.unwrap_or_else(|| "local".to_string()),
        model_path: p.model_path,
        provider_model: p.provider_model,
        system_prompt: p.system_prompt,
        system_prompt_type: p
            .system_prompt_type
            .unwrap_or_else(|| "Custom".to_string()),
        sampler_type: p.sampler_type.unwrap_or_else(|| "Greedy".to_string()),
        temperature: p.temperature.unwrap_or(0.7),
        top_p: p.top_p.unwrap_or(0.95),
        top_k: p.top_k.unwrap_or(20),
        mirostat_tau: p.mirostat_tau.unwrap_or(5.0),
        mirostat_eta: p.mirostat_eta.unwrap_or(0.1),
        repeat_penalty: p.repeat_penalty.unwrap_or(1.0),
        min_p: p.min_p.unwrap_or(0.0),
        typical_p: p.typical_p.unwrap_or(1.0),
        frequency_penalty: p.frequency_penalty.unwrap_or(0.0),
        presence_penalty: p.presence_penalty.unwrap_or(0.0),
        penalty_last_n: p.penalty_last_n.unwrap_or(64),
        dry_multiplier: p.dry_multiplier.unwrap_or(0.0),
        dry_base: p.dry_base.unwrap_or(1.75),
        dry_allowed_length: p.dry_allowed_length.unwrap_or(2),
        dry_penalty_last_n: p.dry_penalty_last_n.unwrap_or(-1),
        top_n_sigma: p.top_n_sigma.unwrap_or(-1.0),
        flash_attention: p.flash_attention.unwrap_or(true),
        cache_type_k: p.cache_type_k.unwrap_or_else(|| "f16".to_string()),
        cache_type_v: p.cache_type_v.unwrap_or_else(|| "f16".to_string()),
        n_batch: p.n_batch.unwrap_or(2048),
        context_size: p.context_size,
        seed: p.seed.unwrap_or(-1),
        n_ubatch: p.n_ubatch.unwrap_or(512),
        n_threads: p.n_threads.unwrap_or(0),
        n_threads_batch: p.n_threads_batch.unwrap_or(0),
        rope_freq_base: p.rope_freq_base.unwrap_or(0.0),
        rope_freq_scale: p.rope_freq_scale.unwrap_or(0.0),
        use_mlock: p.use_mlock.unwrap_or(false),
        use_mmap: p.use_mmap.unwrap_or(true),
        main_gpu: p.main_gpu.unwrap_or(0),
        split_mode: p.split_mode.unwrap_or_else(|| "layer".to_string()),
        stop_tokens: p.stop_tokens,
        tag_pairs: p.tag_pairs,
        tool_tag_exec_open: p.tool_tag_exec_open,
        tool_tag_exec_close: p.tool_tag_exec_close,
        tool_tag_output_open: p.tool_tag_output_open,
        tool_tag_output_close: p.tool_tag_output_close,
        proactive_compaction: p.proactive_compaction.unwrap_or(true),
        safe_tool_injection: p.safe_tool_injection.unwrap_or(false),
        thinking_mode: p.thinking_mode,
        heartbeat_enabled: p.heartbeat_enabled.unwrap_or(false),
        heartbeat_interval_minutes: p.heartbeat_interval_minutes.unwrap_or(30),
        heartbeat_prompt: p.heartbeat_prompt,
        created_at: now,
        updated_at: now,
    }
}

/// Apply payload fields onto an existing AgentRecord (for update).
fn apply_payload(record: &mut llama_chat_db::agents::AgentRecord, p: AgentPayload) {
    record.name = p.name;
    record.provider_id = p.provider_id.unwrap_or_else(|| record.provider_id.clone());
    if p.model_path.is_some() {
        record.model_path = p.model_path;
    }
    if p.provider_model.is_some() {
        record.provider_model = p.provider_model;
    }
    if p.system_prompt.is_some() {
        record.system_prompt = p.system_prompt;
    }
    if let Some(v) = p.system_prompt_type {
        record.system_prompt_type = v;
    }
    if let Some(v) = p.sampler_type {
        record.sampler_type = v;
    }
    if let Some(v) = p.temperature {
        record.temperature = v;
    }
    if let Some(v) = p.top_p {
        record.top_p = v;
    }
    if let Some(v) = p.top_k {
        record.top_k = v;
    }
    if let Some(v) = p.mirostat_tau {
        record.mirostat_tau = v;
    }
    if let Some(v) = p.mirostat_eta {
        record.mirostat_eta = v;
    }
    if let Some(v) = p.repeat_penalty {
        record.repeat_penalty = v;
    }
    if let Some(v) = p.min_p {
        record.min_p = v;
    }
    if let Some(v) = p.typical_p {
        record.typical_p = v;
    }
    if let Some(v) = p.frequency_penalty {
        record.frequency_penalty = v;
    }
    if let Some(v) = p.presence_penalty {
        record.presence_penalty = v;
    }
    if let Some(v) = p.penalty_last_n {
        record.penalty_last_n = v;
    }
    if let Some(v) = p.dry_multiplier {
        record.dry_multiplier = v;
    }
    if let Some(v) = p.dry_base {
        record.dry_base = v;
    }
    if let Some(v) = p.dry_allowed_length {
        record.dry_allowed_length = v;
    }
    if let Some(v) = p.dry_penalty_last_n {
        record.dry_penalty_last_n = v;
    }
    if let Some(v) = p.top_n_sigma {
        record.top_n_sigma = v;
    }
    if let Some(v) = p.flash_attention {
        record.flash_attention = v;
    }
    if let Some(v) = p.cache_type_k {
        record.cache_type_k = v;
    }
    if let Some(v) = p.cache_type_v {
        record.cache_type_v = v;
    }
    if let Some(v) = p.n_batch {
        record.n_batch = v;
    }
    if p.context_size.is_some() {
        record.context_size = p.context_size;
    }
    if let Some(v) = p.seed {
        record.seed = v;
    }
    if let Some(v) = p.n_ubatch {
        record.n_ubatch = v;
    }
    if let Some(v) = p.n_threads {
        record.n_threads = v;
    }
    if let Some(v) = p.n_threads_batch {
        record.n_threads_batch = v;
    }
    if let Some(v) = p.rope_freq_base {
        record.rope_freq_base = v;
    }
    if let Some(v) = p.rope_freq_scale {
        record.rope_freq_scale = v;
    }
    if let Some(v) = p.use_mlock {
        record.use_mlock = v;
    }
    if let Some(v) = p.use_mmap {
        record.use_mmap = v;
    }
    if let Some(v) = p.main_gpu {
        record.main_gpu = v;
    }
    if let Some(v) = p.split_mode {
        record.split_mode = v;
    }
    if p.stop_tokens.is_some() {
        record.stop_tokens = p.stop_tokens;
    }
    if p.tag_pairs.is_some() {
        record.tag_pairs = p.tag_pairs;
    }
    if p.tool_tag_exec_open.is_some() {
        record.tool_tag_exec_open = p.tool_tag_exec_open;
    }
    if p.tool_tag_exec_close.is_some() {
        record.tool_tag_exec_close = p.tool_tag_exec_close;
    }
    if p.tool_tag_output_open.is_some() {
        record.tool_tag_output_open = p.tool_tag_output_open;
    }
    if p.tool_tag_output_close.is_some() {
        record.tool_tag_output_close = p.tool_tag_output_close;
    }
    if let Some(v) = p.proactive_compaction {
        record.proactive_compaction = v;
    }
    if let Some(v) = p.safe_tool_injection {
        record.safe_tool_injection = v;
    }
    if p.thinking_mode.is_some() {
        record.thinking_mode = p.thinking_mode;
    }
    if let Some(v) = p.heartbeat_enabled {
        record.heartbeat_enabled = v;
    }
    if let Some(v) = p.heartbeat_interval_minutes {
        record.heartbeat_interval_minutes = v;
    }
    if p.heartbeat_prompt.is_some() {
        record.heartbeat_prompt = p.heartbeat_prompt;
    }
}

fn record_to_json(
    r: &llama_chat_db::agents::AgentRecord,
) -> serde_json::Value {
    use serde_json::Value;
    let mut m = serde_json::Map::new();
    macro_rules! str { ($k:ident) => { m.insert(stringify!($k).into(), Value::String(r.$k.clone())); }; }
    macro_rules! num { ($k:ident) => { m.insert(stringify!($k).into(), serde_json::json!(r.$k)); }; }
    macro_rules! opt_str { ($k:ident) => { if let Some(ref v) = r.$k { m.insert(stringify!($k).into(), Value::String(v.clone())); } }; }
    macro_rules! opt_val { ($k:ident) => { if let Some(ref v) = r.$k { m.insert(stringify!($k).into(), serde_json::json!(v)); } }; }
    str!(id); str!(name); str!(provider_id);
    opt_str!(model_path); opt_str!(provider_model); opt_str!(system_prompt);
    str!(system_prompt_type); str!(sampler_type);
    num!(temperature); num!(top_p); num!(top_k); num!(mirostat_tau); num!(mirostat_eta);
    num!(repeat_penalty); num!(min_p); num!(typical_p); num!(frequency_penalty);
    num!(presence_penalty); num!(penalty_last_n); num!(dry_multiplier); num!(dry_base);
    num!(dry_allowed_length); num!(dry_penalty_last_n); num!(top_n_sigma);
    num!(flash_attention); str!(cache_type_k); str!(cache_type_v);
    num!(n_batch); opt_val!(context_size); num!(seed); num!(n_ubatch);
    num!(n_threads); num!(n_threads_batch); num!(rope_freq_base); num!(rope_freq_scale);
    num!(use_mlock); num!(use_mmap); num!(main_gpu); str!(split_mode);
    opt_val!(stop_tokens); opt_str!(tag_pairs);
    opt_str!(tool_tag_exec_open); opt_str!(tool_tag_exec_close);
    opt_str!(tool_tag_output_open); opt_str!(tool_tag_output_close);
    num!(proactive_compaction); num!(safe_tool_injection); opt_val!(thinking_mode);
    num!(heartbeat_enabled); num!(heartbeat_interval_minutes); opt_str!(heartbeat_prompt);
    num!(created_at); num!(updated_at);
    Value::Object(m)
}

/// List all agents (newest first).
#[tauri::command]
pub fn list_agents(
    db: State<'_, SharedDatabase>,
) -> Result<Vec<serde_json::Value>, String> {
    let records = db.list_agents()?;
    Ok(records.iter().map(record_to_json).collect())
}

/// Get a single agent by ID.
#[tauri::command]
pub fn get_agent(
    id: String,
    db: State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    match db.get_agent(&id)? {
        Some(r) => Ok(record_to_json(&r)),
        None => Err(format!("Agent {id} not found")),
    }
}

/// Create a new agent.
#[tauri::command]
pub fn create_agent(
    agent: AgentPayload,
    db: State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    let record = payload_to_record(agent);
    let id = record.id.clone();
    db.create_agent(&record)?;
    match db.get_agent(&id)? {
        Some(r) => {
            let json = record_to_json(&r);
            error!("[AGENT] Created agent: {} ({})", json["name"], json["id"]);
            Ok(json)
        }
        None => Err("Agent created but not found".into()),
    }
}

/// Update an existing agent.
#[tauri::command]
pub fn update_agent(
    id: String,
    agent: AgentPayload,
    db: State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    let mut record = db.get_agent(&id)?.ok_or_else(|| format!("Agent {id} not found"))?;
    apply_payload(&mut record, agent);
    record.updated_at = llama_chat_db::current_timestamp_millis();
    db.update_agent(&record)?;
    let json = record_to_json(&record);
    error!("[AGENT] Updated agent: {} ({})", json["name"], json["id"]);
    Ok(json)
}

/// Delete an agent.
#[tauri::command]
pub fn delete_agent(
    id: String,
    db: State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    let record = db.get_agent(&id)?;
    db.delete_agent(&id)?;
    if let Some(r) = record {
        error!("[AGENT] Deleted agent: {} ({})", r.name, r.id);
    }
    Ok(serde_json::json!({"success": true}))
}

/// Get the agent assigned to a conversation, or null.
#[tauri::command]
pub fn get_conversation_agent(
    conversation_id: String,
    db: State<'_, SharedDatabase>,
) -> Result<Option<serde_json::Value>, String> {
    let agent_id = db.get_conversation_agent_id(&conversation_id)?;
    match agent_id {
        Some(aid) => match db.get_agent(&aid)? {
            Some(r) => Ok(Some(record_to_json(&r))),
            None => Ok(None),
        },
        None => Ok(None),
    }
}

/// Assign or clear an agent on a conversation.
#[tauri::command]
pub fn set_conversation_agent(
    conversation_id: String,
    agent_id: Option<String>,
    db: State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    db.set_conversation_agent_id(&conversation_id, agent_id.as_deref())?;
    Ok(serde_json::json!({"success": true}))
}

/// List all agent statuses (idle / active / generating).
#[tauri::command]
pub async fn list_agent_statuses(
    db: State<'_, SharedDatabase>,
    pool: State<'_, WorkerPool>,
) -> Result<serde_json::Value, String> {
    let agents = db.list_agents().map_err(|e| e.to_string())?;
    let mut statuses = serde_json::Map::new();

    for agent in &agents {
        let entry = if agent.provider_id != "local" {
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

    Ok(serde_json::Value::Object(statuses))
}

/// Activate (spawn a worker for) a local agent.
#[tauri::command]
pub async fn activate_agent(
    id: String,
    db: State<'_, SharedDatabase>,
    pool: State<'_, WorkerPool>,
) -> Result<serde_json::Value, String> {
    let agent = db
        .get_agent(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Agent {id} not found"))?;

    if agent.provider_id != "local" {
        return Ok(serde_json::json!({ "status": "active" }));
    }

    // Already activated?
    if let Some(existing_worker_id) = pool.get_worker_for_agent(&id) {
        return Ok(serde_json::json!({ "status": "active", "worker_id": existing_worker_id }));
    }

    let model_path = agent
        .model_path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .ok_or_else(|| "Agent has no model_path configured".to_string())?;

    let gpu_layers = u32::try_from(agent.main_gpu).ok().filter(|&l| l > 0);

    match pool
        .spawn_worker_for_agent(&id, model_path, gpu_layers, None)
        .await
    {
        Ok(worker_id) => Ok(serde_json::json!({ "status": "active", "worker_id": worker_id })),
        Err(e) => Err(e),
    }
}

/// Stop (kill the worker for) a local agent.
#[tauri::command]
pub async fn stop_agent(
    id: String,
    db: State<'_, SharedDatabase>,
    pool: State<'_, WorkerPool>,
) -> Result<serde_json::Value, String> {
    let agent = db
        .get_agent(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Agent {id} not found"))?;

    if agent.provider_id != "local" {
        return Ok(serde_json::json!({"success": true, "message": "Remote agents do not have workers to stop"}));
    }

    if pool.get_worker_for_agent(&id).is_some() {
        pool.stop_agent_worker(&id).await.map_err(|e| e.to_string())?;
    }

    let conversation_ids = db
        .list_conversation_ids_by_agent(&id)
        .unwrap_or_default();
    pool.stop_overflow_workers_for_conversations(&conversation_ids)
        .await;

    error!("[AGENT] Stopped agent: {} ({})", agent.name, agent.id);
    Ok(serde_json::json!({"success": true, "message": "Agent stopped"}))
}
