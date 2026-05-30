//! Multi-worker pool for local model worker processes.

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
    /// Maps agent_id → worker_id for agents with dedicated workers.
    agent_workers: Arc<RwLock<HashMap<String, WorkerId>>>,
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
        self.spawn_worker_with_options(model_path, None, None).await
    }

    pub async fn spawn_worker_with_options(
        &self,
        model_path: &str,
        gpu_layers: Option<u32>,
        mmproj_path: Option<String>,
    ) -> Result<WorkerId, String> {
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let worker_id = format!("w{}", &suffix[..8]);

        let pm = Arc::new(ProcessManager::spawn(&self.db_path)?);
        let bridge = Arc::new(WorkerBridge::new(pm));
        bridge
            .load_model(model_path, gpu_layers, mmproj_path)
            .await?;

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

    /// In-memory half only — removes the worker entry from the pool.
    pub async fn remove_worker(&self, worker_id: &str) -> Result<(), String> {
        if worker_id == "default" {
            return Err("Cannot remove default worker from pool".to_string());
        }

        let removed = self
            .workers
            .write()
            .map_err(|_| "WorkerPool lock poisoned".to_string())?
            .remove(worker_id);

        if removed.is_some() {
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

    // ─── Agent binding ─────────────────────────────────────────────────────

    /// Bind an agent to a specific worker (creates dedicated routing for the agent).
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

    /// Look up the worker_id bound to an agent, if any.
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

    /// Spawn a new worker, load the model, and bind it to the agent.
    pub async fn spawn_worker_for_agent(
        &self,
        agent_id: &str,
        model_path: &str,
        gpu_layers: Option<u32>,
        mmproj_path: Option<String>,
    ) -> Result<WorkerId, String> {
        let worker_id = self
            .spawn_worker_with_options(model_path, gpu_layers, mmproj_path)
            .await?;
        self.bind_agent_worker(agent_id, worker_id.clone())?;
        Ok(worker_id)
    }

    /// Stop and remove the worker bound to an agent, and unbind it.
    pub async fn stop_agent_worker(&self, agent_id: &str) -> Result<(), String> {
        let worker_id = self
            .get_worker_for_agent(agent_id)
            .ok_or_else(|| format!("Agent {agent_id} has no active worker"))?;

        self.unbind_agent_worker(agent_id)?;
        remove_worker_and_rebind_no_db(self, &worker_id).await
    }
}

pub fn lookup_worker_id_for_conversation(
    db: &SharedDatabase,
    conversation_id: &str,
) -> Option<WorkerId> {
    db.get_conversation_worker_id(conversation_id).ok().flatten()
}

pub fn resolve_bridge_for_conversation(
    pool: &WorkerPool,
    db: &SharedDatabase,
    conversation_id: Option<&str>,
) -> Result<SharedWorkerBridge, String> {
    // First: if the conversation has an agent with a dedicated worker, use it.
    if let Some(conv_id) = conversation_id {
        if let Ok(Some(agent_id)) = db.get_conversation_agent_id(conv_id) {
            if let Some(worker_id) = pool.get_worker_for_agent(&agent_id) {
                if let Some(bridge) = pool.get(&worker_id) {
                    return Ok(bridge);
                }
            }
        }
    }

    // Fallback: conversation's own worker_id (legacy), then default.
    let worker_id = conversation_id.and_then(|id| lookup_worker_id_for_conversation(db, id));
    pool.get_or_default(worker_id.as_deref())
        .ok_or_else(|| "No worker bridge available".to_string())
}

/// Remove a worker from the pool without touching the DB (for agent stop).
async fn remove_worker_and_rebind_no_db(pool: &WorkerPool, worker_id: &str) -> Result<(), String> {
    if worker_id == "default" {
        return Err("Default worker cannot be removed".to_string());
    }
    pool.remove_worker(worker_id).await
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
