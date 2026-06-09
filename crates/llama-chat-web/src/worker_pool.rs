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
}

impl WorkerPool {
    pub fn new(default_bridge: SharedWorkerBridge, db_path: impl Into<String>) -> Self {
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
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let worker_id = format!("w{}", &suffix[..8]);

        let pm = Arc::new(ProcessManager::spawn(&self.db_path)?);
        let bridge = Arc::new(WorkerBridge::new(pm));
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
