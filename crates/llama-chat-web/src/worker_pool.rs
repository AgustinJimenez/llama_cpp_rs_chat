//! Multi-worker pool for local model worker processes.
//!
//! # Worker lifecycle
//!
//! There are two kinds of workers:
//!
//! **Global agent workers** (`agent_workers`):
//!   One per agent, created when the user clicks "Activate" in the agents modal.
//!   Keeps the model warm in VRAM so the first conversation is instant.
//!   Shared across conversations that use this agent — as long as only one is
//!   active at a time.
//!
//! **Per-conversation overflow workers** (`conversation_workers`):
//!   Spawned automatically when a second conversation tries to use an agent
//!   whose global worker is already generating. Each overflow conversation gets
//!   its own dedicated process, enabling true parallel inference.
//!   Cleaned up when the agent is stopped or the conversation is removed.
//!
//! # Routing (`resolve_bridge_for_conversation`)
//!
//! 1. Conversation already has an overflow worker → use it.
//! 2. Conversation has a local agent with a global worker:
//!    a. Global worker is free → use it.
//!    b. Global worker is busy → spawn overflow worker, bind to conversation.
//! 3. Conversation has a local agent but no global worker → spawn lazily.
//! 4. Legacy `worker_id` on conversation → use that worker.
//! 5. Default worker.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use llama_chat_db::SharedDatabase;
use llama_chat_worker::worker::process_manager::ProcessManager;
use llama_chat_worker::worker::worker_bridge::{SharedWorkerBridge, WorkerBridge};

pub type WorkerId = String;

#[derive(Clone)]
pub struct WorkerEntry {
    pub id: WorkerId,
    pub bridge: SharedWorkerBridge,
    pub created_at: SystemTime,
}

#[derive(Clone)]
pub struct WorkerPool {
    workers: Arc<RwLock<HashMap<WorkerId, WorkerEntry>>>,
    /// agent_id → worker_id: one pre-loaded global worker per agent (Activate button).
    agent_workers: Arc<RwLock<HashMap<String, WorkerId>>>,
    /// conversation_id → worker_id: overflow workers for parallel conversations.
    /// Only populated when a second conversation needs the same agent simultaneously.
    conversation_workers: Arc<RwLock<HashMap<String, WorkerId>>>,
    db_path: String,
    db: SharedDatabase,
}

impl WorkerPool {
    pub fn new(default_bridge: SharedWorkerBridge, db_path: impl Into<String>, db: SharedDatabase) -> Self {
        let mut workers = HashMap::new();
        workers.insert(
            "default".to_string(),
            WorkerEntry {
                id: "default".to_string(),
                bridge: default_bridge,
                created_at: SystemTime::now(),
            },
        );

        Self {
            workers: Arc::new(RwLock::new(workers)),
            agent_workers: Arc::new(RwLock::new(HashMap::new())),
            conversation_workers: Arc::new(RwLock::new(HashMap::new())),
            db_path: db_path.into(),
            db,
        }
    }

    pub fn get(&self, worker_id: &str) -> Option<SharedWorkerBridge> {
        self.workers
            .read()
            .ok()
            .and_then(|workers| workers.get(worker_id).map(|entry| entry.bridge.clone()))
    }

    pub fn get_or_default(&self, worker_id: Option<&str>) -> Option<SharedWorkerBridge> {
        match worker_id {
            Some(id) => self.get(id).or_else(|| self.get("default")),
            None => self.get("default"),
        }
    }

    pub fn list_worker_ids(&self) -> Vec<WorkerId> {
        self.workers
            .read()
            .map(|workers| workers.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn list_entries(&self) -> Vec<WorkerEntry> {
        self.workers
            .read()
            .map(|workers| workers.values().cloned().collect())
            .unwrap_or_default()
    }

    pub async fn spawn_worker(&self, model_path: &str) -> Result<WorkerId, String> {
        self.spawn_worker_with_options(model_path, None, None, None).await
    }

    pub async fn spawn_worker_with_options(
        &self,
        model_path: &str,
        gpu_layers: Option<u32>,
        mmproj_path: Option<String>,
        agent_id: Option<String>,
    ) -> Result<WorkerId, String> {
        // Free unified/GPU memory first if the machine can't hold this model alongside
        // the ones already resident (e.g. a 16GB Mac can't keep two ~6.4GB copies — that
        // OOMs the Metal backend as "Decode Error -3"). On a high-RAM box this is a no-op.
        self.evict_idle_workers_if_memory_tight(model_path).await;

        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let worker_id = format!("w{}", &suffix[..8]);

        let pm = Arc::new(ProcessManager::spawn(&self.db_path)?);
        let bridge = Arc::new(WorkerBridge::new(pm, self.db.clone()));
        if let Err(e) = bridge.load_model(model_path, gpu_layers, mmproj_path, agent_id).await {
            bridge.kill();
            return Err(e);
        }

        let entry = WorkerEntry {
            id: worker_id.clone(),
            bridge,
            created_at: SystemTime::now(),
        };

        self.workers
            .write()
            .map_err(|_| "WorkerPool lock poisoned".to_string())?
            .insert(worker_id.clone(), entry);

        Ok(worker_id)
    }

    /// In-memory half only — removes the worker entry from the pool and kills the process.
    pub async fn remove_worker(&self, worker_id: &str) -> Result<(), String> {
        if worker_id == "default" {
            return Err("Cannot remove default worker from pool".to_string());
        }

        let removed = self
            .workers
            .write()
            .map_err(|_| "WorkerPool lock poisoned".to_string())?
            .remove(worker_id);

        if let Some(entry) = removed {
            // Explicitly kill the process before dropping the Arc so the stdout
            // reader task sees the shutdown flag and doesn't respawn the worker.
            entry.bridge.kill();
            Ok(())
        } else {
            Err(format!("Worker not found: {worker_id}"))
        }
    }

    /// Before loading a new model into a fresh worker, unload idle workers so their
    /// models don't co-reside with the incoming one. Only acts when capacity is tight:
    /// if total RAM can hold every resident model PLUS the new one (minus headroom), it
    /// leaves everything loaded for speed (the 512GB-Mac case). Workers mid-generation
    /// are never touched. Non-default victims are killed (full release) and respawn +
    /// reload on next use; the persistent `default` worker just unloads its model.
    async fn evict_idle_workers_if_memory_tight(&self, new_model_path: &str) {
        self.evict_to_fit(model_file_size(new_model_path), None).await;
    }

    /// Public hook for load paths that reuse an EXISTING worker (e.g. loading a model
    /// into the persistent `default` worker). Frees memory by unloading the *other*
    /// loaded workers if the incoming model won't fit alongside them; `keep_worker_id`
    /// is never evicted and its (about-to-be-replaced) model is excluded from the budget.
    pub async fn free_memory_for_load(&self, new_model_path: &str, keep_worker_id: &str) {
        self.evict_to_fit(model_file_size(new_model_path), Some(keep_worker_id))
            .await;
    }

    /// Core eviction: unload idle workers until a model of `new_size` bytes fits within
    /// the memory budget. Only acts when capacity is tight — on a high-RAM box every
    /// model stays resident for speed. Workers mid-generation are never touched.
    /// `keep_id`, if set, is excluded from both eviction and the resident total (its model
    /// is being swapped out by the caller anyway).
    async fn evict_to_fit(&self, new_size: u64, keep_id: Option<&str>) {
        let total_ram = total_physical_ram_bytes();
        if total_ram == 0 {
            return; // unknown capacity — don't evict anything
        }
        // Reserve headroom for the OS, the app, and per-model KV/compute buffers. On
        // unified-memory Macs the Metal GPU working set is capped well below total RAM
        // (~70%), so reserve more there — otherwise two ~6GB models slip under a naive
        // RAM budget yet still exhaust the GPU working set and OOM.
        let reserve_pct: u64 = if cfg!(target_os = "macos") { 35 } else { 20 };
        let reserve = (total_ram * reserve_pct / 100).max(3 * 1024 * 1024 * 1024);
        let budget = total_ram.saturating_sub(reserve);

        // Snapshot currently-loaded workers, their model size, and whether they're busy.
        let mut loaded: Vec<(WorkerId, SharedWorkerBridge, u64, bool)> = Vec::new();
        let mut resident: u64 = 0;
        for entry in self.list_entries() {
            if keep_id == Some(entry.id.as_str()) {
                continue; // never count or evict the kept worker
            }
            if let Some(meta) = entry.bridge.model_status().await {
                let size = model_file_size(&meta.model_path);
                let generating = entry.bridge.is_generating().await;
                resident = resident.saturating_add(size);
                loaded.push((entry.id, entry.bridge, size, generating));
            }
        }

        // Everything (including the incoming model) fits — keep all models resident.
        if new_size.saturating_add(resident) <= budget {
            return;
        }

        // Tight: unload idle workers until the new model fits.
        for (id, bridge, size, generating) in loaded {
            if new_size.saturating_add(resident) <= budget {
                break; // freed enough
            }
            if generating {
                continue; // never yank a model out from under an active generation
            }
            if id == "default" {
                let _ = bridge.unload_model().await;
            } else {
                self.evict_named_worker(&id).await;
            }
            resident = resident.saturating_sub(size);
        }
    }

    /// Drop any agent/conversation bindings pointing at `worker_id`, then kill it so it
    /// fully releases memory. Routing respawns + reloads it lazily on next use.
    async fn evict_named_worker(&self, worker_id: &str) {
        if let Ok(mut m) = self.agent_workers.write() {
            m.retain(|_, w| w != worker_id);
        }
        if let Ok(mut m) = self.conversation_workers.write() {
            m.retain(|_, w| w != worker_id);
        }
        let _ = self.remove_worker(worker_id).await;
    }

    pub fn mark_dead(&self, worker_id: &str) -> Result<(), String> {
        let mut workers = self
            .workers
            .write()
            .map_err(|_| "WorkerPool lock poisoned".to_string())?;

        if worker_id == "default" {
            return Ok(());
        }

        workers.remove(worker_id);
        Ok(())
    }

    // ─── Global agent workers (Activate / Stop) ────────────────────────────────

    /// Bind an agent to its global pre-loaded worker.
    pub fn bind_agent_worker(&self, agent_id: &str, worker_id: WorkerId) -> Result<(), String> {
        self.agent_workers
            .write()
            .map_err(|_| "AgentWorkers lock poisoned".to_string())?
            .insert(agent_id.to_string(), worker_id);
        Ok(())
    }

    /// Remove the agent → worker binding (does not kill the worker).
    pub fn unbind_agent_worker(&self, agent_id: &str) -> Result<(), String> {
        self.agent_workers
            .write()
            .map_err(|_| "AgentWorkers lock poisoned".to_string())?
            .remove(agent_id);
        Ok(())
    }

    /// Look up the global worker_id for an agent, if activated.
    pub fn get_worker_for_agent(&self, agent_id: &str) -> Option<WorkerId> {
        self.agent_workers
            .read()
            .ok()
            .and_then(|m| m.get(agent_id).cloned())
    }

    /// List all active agent→worker bindings.
    pub fn list_agent_bindings(&self) -> Vec<(String, WorkerId)> {
        self.agent_workers
            .read()
            .map(|m| m.iter().map(|(a, w)| (a.clone(), w.clone())).collect())
            .unwrap_or_default()
    }

    /// Spawn a global worker for an agent and bind it.
    pub async fn spawn_worker_for_agent(
        &self,
        agent_id: &str,
        model_path: &str,
        gpu_layers: Option<u32>,
        mmproj_path: Option<String>,
    ) -> Result<WorkerId, String> {
        let worker_id = self
            .spawn_worker_with_options(model_path, gpu_layers, mmproj_path, Some(agent_id.to_string()))
            .await?;
        self.bind_agent_worker(agent_id, worker_id.clone())?;
        Ok(worker_id)
    }

    /// Stop the global worker for an agent (unbind + kill).
    pub async fn stop_agent_worker(&self, agent_id: &str) -> Result<(), String> {
        let worker_id = self
            .get_worker_for_agent(agent_id)
            .ok_or_else(|| format!("Agent {agent_id} has no active worker"))?;
        self.unbind_agent_worker(agent_id)?;
        self.remove_worker(&worker_id).await
    }

    // ─── Per-conversation overflow workers ─────────────────────────────────────

    /// Bind a conversation to its own overflow worker.
    pub fn bind_conversation_worker(
        &self,
        conversation_id: &str,
        worker_id: WorkerId,
    ) -> Result<(), String> {
        self.conversation_workers
            .write()
            .map_err(|_| "ConversationWorkers lock poisoned".to_string())?
            .insert(conversation_id.to_string(), worker_id);
        Ok(())
    }

    /// Remove the conversation → overflow worker binding (does not kill the worker).
    pub fn unbind_conversation_worker(&self, conversation_id: &str) -> Result<(), String> {
        self.conversation_workers
            .write()
            .map_err(|_| "ConversationWorkers lock poisoned".to_string())?
            .remove(conversation_id);
        Ok(())
    }

    /// Look up the overflow worker_id bound to a conversation, if any.
    pub fn get_worker_for_conversation(&self, conversation_id: &str) -> Option<WorkerId> {
        self.conversation_workers
            .read()
            .ok()
            .and_then(|m| m.get(conversation_id).cloned())
    }

    /// List all active conversation→overflow worker bindings.
    pub fn list_conversation_workers(&self) -> Vec<(String, WorkerId)> {
        self.conversation_workers
            .read()
            .map(|m| m.iter().map(|(c, w)| (c.clone(), w.clone())).collect())
            .unwrap_or_default()
    }

    /// Spawn an overflow worker for a conversation and bind it.
    pub async fn spawn_worker_for_conversation(
        &self,
        conversation_id: &str,
        model_path: &str,
        gpu_layers: Option<u32>,
        mmproj_path: Option<String>,
        agent_id: Option<String>,
    ) -> Result<WorkerId, String> {
        let worker_id = self
            .spawn_worker_with_options(model_path, gpu_layers, mmproj_path, agent_id)
            .await?;
        self.bind_conversation_worker(conversation_id, worker_id.clone())?;
        Ok(worker_id)
    }

    /// Stop all overflow workers for conversations in the given list.
    pub async fn stop_overflow_workers_for_conversations(&self, conversation_ids: &[String]) {
        for conv_id in conversation_ids {
            if let Some(worker_id) = self.get_worker_for_conversation(conv_id) {
                let _ = self.unbind_conversation_worker(conv_id);
                let _ = self.remove_worker(&worker_id).await;
            }
        }
    }
}

pub fn lookup_worker_id_for_conversation(
    db: &SharedDatabase,
    conversation_id: &str,
) -> Option<WorkerId> {
    db.get_conversation_worker_id(conversation_id).ok().flatten()
}

/// Resolve (or auto-spawn) the worker bridge for a conversation.
///
/// See module-level doc for routing order.
pub async fn resolve_bridge_for_conversation(
    pool: &WorkerPool,
    db: &SharedDatabase,
    conversation_id: Option<&str>,
) -> Result<SharedWorkerBridge, String> {
    if let Some(conv_id) = conversation_id {
        // 1. Conversation already has its own overflow worker.
        if let Some(worker_id) = pool.get_worker_for_conversation(conv_id) {
            if let Some(bridge) = pool.get(&worker_id) {
                return Ok(bridge);
            }
            // Overflow worker died — clean up and fall through.
            let _ = pool.unbind_conversation_worker(conv_id);
        }

        // 2 & 3. Conversation has a local agent.
        if let Ok(Some(agent_id)) = db.get_conversation_agent_id(conv_id) {
            if let Ok(Some(agent)) = db.get_agent(&agent_id) {
                if agent.provider_id == "local" {
                    if let Some(model_path) = agent
                        .model_path
                        .as_deref()
                        .map(str::trim)
                        .filter(|p| !p.is_empty())
                    {
                        let gpu_layers =
                            u32::try_from(agent.main_gpu).ok().filter(|&l| l > 0);

                        // 2a. Global agent worker exists and is free → use it.
                        if let Some(global_wid) = pool.get_worker_for_agent(&agent_id) {
                            if let Some(bridge) = pool.get(&global_wid) {
                                if !bridge.is_generating().await {
                                    return Ok(bridge);
                                }
                                // 2b. Global worker is busy → spawn overflow worker.
                                match pool
                                    .spawn_worker_for_conversation(
                                        conv_id,
                                        model_path,
                                        gpu_layers,
                                        None,
                                        Some(agent_id.clone()),
                                    )
                                    .await
                                {
                                    Ok(wid) => {
                                        if let Some(b) = pool.get(&wid) {
                                            return Ok(b);
                                        }
                                    }
                                    Err(e) => {
                                        return Err(format!(
                                            "Failed to spawn overflow worker: {e}"
                                        ))
                                    }
                                }
                            }
                        }

                        // 3. No global worker — spawn lazily for this conversation.
                        match pool
                            .spawn_worker_for_conversation(
                                conv_id,
                                model_path,
                                gpu_layers,
                                None,
                                Some(agent_id.clone()),
                            )
                            .await
                        {
                            Ok(wid) => {
                                if let Some(b) = pool.get(&wid) {
                                    return Ok(b);
                                }
                            }
                            Err(e) => {
                                return Err(format!("Failed to spawn agent worker: {e}"))
                            }
                        }
                    }
                }
            }
        }
    }

    // 4 & 5. Legacy conversation worker_id or default.
    let worker_id = conversation_id.and_then(|id| lookup_worker_id_for_conversation(db, id));
    pool.get_or_default(worker_id.as_deref())
        .ok_or_else(|| "No worker bridge available".to_string())
}

/// Resolve the worker bridge for a new or existing request.
///
/// For new conversations (`conversation_id` is None), routes by `agent_id` first
/// (to the agent's global worker), then falls back to `requested_worker_id`, then default.
/// For existing conversations, delegates to `resolve_bridge_for_conversation`.
pub async fn resolve_bridge_for_request(
    pool: &WorkerPool,
    db: &SharedDatabase,
    conversation_id: Option<&str>,
    requested_worker_id: Option<&str>,
    agent_id: Option<&str>,
) -> Result<SharedWorkerBridge, String> {
    if let Some(conversation_id) = conversation_id {
        return resolve_bridge_for_conversation(pool, db, Some(conversation_id)).await;
    }

    // New conversation: if a local agent is specified, use its global worker.
    if let Some(aid) = agent_id.map(str::trim).filter(|id| !id.is_empty()) {
        if let Some(worker_id) = pool.get_worker_for_agent(aid) {
            if let Some(bridge) = pool.get(&worker_id) {
                return Ok(bridge);
            }
        }
        // No live global worker for this agent yet → spawn one (load the agent's
        // model) instead of falling back to the empty `default` worker, which would
        // error with "No model loaded and no model configured for this conversation".
        if let Ok(Some(agent)) = db.get_agent(aid) {
            if agent.provider_id == "local" {
                if let Some(model_path) = agent
                    .model_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                {
                    let gpu_layers = u32::try_from(agent.main_gpu).ok().filter(|&l| l > 0);
                    match pool
                        .spawn_worker_for_agent(aid, model_path, gpu_layers, None)
                        .await
                    {
                        Ok(wid) => {
                            if let Some(bridge) = pool.get(&wid) {
                                return Ok(bridge);
                            }
                        }
                        Err(e) => return Err(format!("Failed to load agent model: {e}")),
                    }
                }
            }
        }
    }

    let worker_id = requested_worker_id
        .map(str::trim)
        .filter(|id| !id.is_empty() && *id != "default");

    pool.get_or_default(worker_id)
        .ok_or_else(|| "No worker bridge available".to_string())
}

pub async fn remove_worker_and_rebind_conversations(
    pool: &WorkerPool,
    db: &SharedDatabase,
    worker_id: &str,
) -> Result<(), String> {
    if worker_id == "default" {
        return Err("Default worker cannot be removed via pool removal".to_string());
    }

    db.clear_worker_id_for_worker(worker_id)?;
    pool.remove_worker(worker_id).await
}

/// Total physical RAM in bytes (0 if it can't be determined — callers treat 0 as
/// "unknown, don't evict"). On unified-memory Macs this is also the GPU memory ceiling.
fn total_physical_ram_bytes() -> u64 {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0)
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find_map(|l| l.strip_prefix("MemTotal:"))
                    .and_then(|rest| rest.trim().split_whitespace().next().map(str::to_owned))
            })
            .and_then(|kb| kb.parse::<u64>().ok())
            .map(|kb| kb * 1024)
            .unwrap_or(0)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        0
    }
}

/// On-disk size of a GGUF model in bytes — a good proxy for its resident memory
/// footprint when loaded (0 if the file is missing/unreadable).
fn model_file_size(path: &str) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}
