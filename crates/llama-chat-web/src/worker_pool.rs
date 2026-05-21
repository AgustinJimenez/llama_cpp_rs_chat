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
    let worker_id = conversation_id.and_then(|id| lookup_worker_id_for_conversation(db, id));
    pool.get_or_default(worker_id.as_deref())
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
