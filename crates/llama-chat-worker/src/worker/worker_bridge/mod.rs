//! Server-side abstraction for communicating with the worker process.
//!
//! Replaces `SharedLlamaState + GenerationQueue` in route handlers.
//! Manages stdin/stdout pipes, request/response correlation, and token streaming.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot, Mutex as TokioMutex};
use tokio::time::{timeout, Duration};

use super::io_tasks::{stdin_writer_task, stdout_reader_task, CrashRecoveryCtx};
use super::ipc_types::*;
use super::process_manager::ProcessManager;
use llama_chat_types::models::TokenData;

mod types;
pub use types::{ActiveGeneration, GenerationResult, ModelMeta, PendingRequest};
use types::oneshot_adapter;

/// Shared reference to the WorkerBridge.
pub type SharedWorkerBridge = Arc<WorkerBridge>;

/// Server-side handle to the worker process.
pub struct WorkerBridge {
    /// Sends (serialized JSON + newline) to the stdin writer task.
    /// Wrapped in mutex so it can be replaced after worker restart.
    cmd_tx: Arc<TokioMutex<mpsc::UnboundedSender<String>>>,
    /// Tracks pending requests awaiting a response.
    pending: Arc<TokioMutex<HashMap<u64, PendingRequest>>>,
    /// Active generation token forwarding.
    active_generation: Arc<TokioMutex<Option<ActiveGeneration>>>,
    /// Cached model metadata.
    model_meta: Arc<TokioMutex<Option<ModelMeta>>>,
    /// True while a model load is in progress.
    loading: AtomicBool,
    /// True while the bridge is auto-recovering from a crash (prevents frontend duplicate reload).
    auto_recovering: Arc<AtomicBool>,
    /// Model loading progress (0-100), updated by stdout reader from worker IPC.
    loading_progress: Arc<AtomicU8>,
    /// Model path being loaded (for status reporting during load).
    loading_model_path: Arc<TokioMutex<Option<String>>>,
    /// Last successfully loaded model path — never cleared so it survives crash-recovery cycles.
    last_model_path: Arc<TokioMutex<Option<String>>>,
    /// Status message (e.g. "Compacting conversation (5/43)") visible via API.
    status_message: Arc<TokioMutex<Option<String>>>,
    /// Last generation finish reason (for polling-based auto-continue).
    last_finish_reason: Arc<TokioMutex<Option<String>>>,
    /// Next request ID counter.
    next_id: AtomicU64,
    /// Process manager for kill/restart.
    process_manager: Arc<ProcessManager>,
    /// Crash-recovery context shared with the stdout reader — cleared on intentional unload
    /// so auto-reload doesn't fire when we voluntarily kill the worker.
    recovery_ctx: Arc<TokioMutex<CrashRecoveryCtx>>,
}

impl WorkerBridge {
    /// Create a new WorkerBridge and start IO tasks.
    pub fn new(process_manager: Arc<ProcessManager>) -> Self {
        let stdin_handle = process_manager
            .take_stdin()
            .expect("Worker stdin not available");
        let stdout_handle = process_manager
            .take_stdout()
            .expect("Worker stdout not available");

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<String>();

        let pending: Arc<TokioMutex<HashMap<u64, PendingRequest>>> =
            Arc::new(TokioMutex::new(HashMap::new()));
        let active_generation: Arc<TokioMutex<Option<ActiveGeneration>>> =
            Arc::new(TokioMutex::new(None));
        let model_meta: Arc<TokioMutex<Option<ModelMeta>>> = Arc::new(TokioMutex::new(None));
        let loading_progress: Arc<AtomicU8> = Arc::new(AtomicU8::new(0));

        // Stdin writer task
        tokio::spawn(stdin_writer_task(cmd_rx, stdin_handle));

        // Stdout reader task
        let cmd_tx_arc = Arc::new(TokioMutex::new(cmd_tx));
        let recovery_ctx = Arc::new(TokioMutex::new(CrashRecoveryCtx::default()));
        let auto_recovering = Arc::new(AtomicBool::new(false));
        let last_model_path: Arc<TokioMutex<Option<String>>> = Arc::new(TokioMutex::new(None));
        let status_message: Arc<TokioMutex<Option<String>>> = Arc::new(TokioMutex::new(None));
        let initial_gen = process_manager.generation();
        tokio::spawn(stdout_reader_task(
            stdout_handle,
            pending.clone(),
            active_generation.clone(),
            model_meta.clone(),
            last_model_path.clone(),
            loading_progress.clone(),
            process_manager.clone(),
            cmd_tx_arc.clone(),
            recovery_ctx.clone(),
            auto_recovering.clone(),
            status_message.clone(),
            initial_gen,
        ));

        Self {
            cmd_tx: cmd_tx_arc,
            pending,
            active_generation,
            model_meta,
            loading: AtomicBool::new(false),
            auto_recovering,
            loading_progress,
            loading_model_path: Arc::new(TokioMutex::new(None)),
            last_model_path,
            status_message,
            last_finish_reason: Arc::new(TokioMutex::new(None)),
            next_id: AtomicU64::new(1),
            process_manager,
            recovery_ctx,
        }
    }

    /// Send a command and wait for the response.
    async fn send_and_wait(&self, command: WorkerCommand) -> Result<WorkerPayload, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = WorkerRequest { id, command };
        let json =
            serde_json::to_string(&request).map_err(|e| format!("Serialize error: {e}"))?;

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(id, PendingRequest { tx });
        }

        self.cmd_tx
            .lock()
            .await
            .send(json)
            .map_err(|_| "Worker stdin closed".to_string())?;

        rx.await.map_err(|_| "Worker response channel closed".to_string())
    }

    /// Send a fire-and-forget command (no response expected).
    async fn send_fire_and_forget(&self, command: WorkerCommand) {
        let request = WorkerRequest { id: 0, command };
        if let Ok(json) = serde_json::to_string(&request) {
            let _ = self.cmd_tx.lock().await.send(json);
        }
    }

    /// Load a model in the worker process.
    pub async fn load_model(
        &self,
        model_path: &str,
        gpu_layers: Option<u32>,
        mmproj_path: Option<String>,
    ) -> Result<ModelMeta, String> {
        // If the bridge is auto-recovering from a crash, don't accept external load requests
        // to avoid racing with the recovery thread's own LoadModel command.
        if self.auto_recovering.load(Ordering::SeqCst) {
            return Err(
                "Model is being auto-reloaded after crash recovery. Please wait.".to_string(),
            );
        }
        self.loading.store(true, Ordering::SeqCst);
        self.loading_progress.store(0, Ordering::Relaxed);
        *self.loading_model_path.lock().await = Some(model_path.to_string());

        // Timeout: if the worker doesn't respond within 120s, it's likely stuck
        // due to VRAM overflow (CUDA VMM silently pages to RAM → infinite stall).
        const LOAD_TIMEOUT_SECS: u64 = 120;
        let payload = match timeout(
            Duration::from_secs(LOAD_TIMEOUT_SECS),
            self.send_and_wait(WorkerCommand::LoadModel {
                model_path: model_path.to_string(),
                gpu_layers,
                mmproj_path,
            }),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                eprintln!(
                    "[LOAD] Timeout after {LOAD_TIMEOUT_SECS}s — likely VRAM overflow. Killing worker."
                );
                self.loading.store(false, Ordering::SeqCst);
                self.loading_progress.store(0, Ordering::Relaxed);
                *self.loading_model_path.lock().await = None;
                // Kill and restart the worker so it doesn't stay hung
                let _ = self.force_unload().await;
                return Err(format!(
                    "Model load timed out after {LOAD_TIMEOUT_SECS}s. This usually means the \
                     context size + KV cache exceeds available VRAM. Try reducing context size \
                     or using a smaller KV cache quantization (e.g. q4_0)."
                ));
            }
        };

        self.loading.store(false, Ordering::SeqCst);
        self.loading_progress.store(0, Ordering::Relaxed);
        *self.loading_model_path.lock().await = None;

        match payload? {
            WorkerPayload::ModelLoaded {
                model_path,
                context_length,
                chat_template_type,
                chat_template_string,
                general_name,
                has_vision,
                gpu_layers,
                block_count,
            } => {
                let supports_thinking = chat_template_string
                    .as_deref()
                    .map(llama_chat_engine::jinja_templates::detect_thinking_support)
                    .unwrap_or(false);
                let meta = ModelMeta {
                    loaded: true,
                    model_path,
                    context_length,
                    chat_template_type,
                    general_name,
                    has_vision: has_vision.unwrap_or(false),
                    gpu_layers,
                    block_count,
                    supports_thinking,
                };
                *self.last_model_path.lock().await = Some(meta.model_path.clone());
                *self.model_meta.lock().await = Some(meta.clone());
                Ok(meta)
            }
            WorkerPayload::Error { message } => Err(message),
            _ => Err("Unexpected response to LoadModel".to_string()),
        }
    }

    /// Check if a model is currently being loaded (includes auto-recovery loading).
    pub fn is_loading(&self) -> bool {
        self.loading.load(Ordering::SeqCst) || self.auto_recovering.load(Ordering::SeqCst)
    }

    /// Check if the bridge is auto-recovering from a crash.
    pub fn is_auto_recovering(&self) -> bool {
        self.auto_recovering.load(Ordering::SeqCst)
    }

    /// Get model loading progress (0-100).
    pub fn loading_progress(&self) -> u8 {
        self.loading_progress.load(Ordering::Relaxed)
    }

    /// Get the model path being loaded (if any).
    pub async fn loading_path(&self) -> Option<String> {
        self.loading_model_path.lock().await.clone()
    }

    /// Unload the model (within the worker process).
    #[allow(dead_code)] // Used by Tauri binary (main.rs), not llama_chat_web
    pub async fn unload_model(&self) -> Result<(), String> {
        let payload = self.send_and_wait(WorkerCommand::UnloadModel).await?;
        match payload {
            WorkerPayload::ModelUnloaded => {
                *self.model_meta.lock().await = None;
                Ok(())
            }
            WorkerPayload::Error { message } => Err(message),
            _ => Err("Unexpected response to UnloadModel".to_string()),
        }
    }

    /// Force-kill the worker process. OS reclaims ALL memory (VRAM + RAM).
    /// Automatically restarts a fresh worker.
    pub async fn force_unload(&self) -> Result<(), String> {
        // Clear BOTH recovery_ctx.model_path AND model_meta BEFORE killing.
        // The crash-recovery handler in stdout_reader_task checks model_meta and will
        // overwrite recovery_ctx.model_path from it if model_meta is still set at the
        // time of the kill — causing unintended auto-reload after intentional unload.
        // Clearing both here (before kill) closes that race.
        self.recovery_ctx.lock().await.model_path = None;
        *self.model_meta.lock().await = None;

        // Kill the worker (blocking call — use spawn_blocking to avoid stalling the runtime)
        let pm = self.process_manager.clone();
        tokio::task::spawn_blocking(move || pm.kill())
            .await
            .map_err(|e| format!("Kill task failed: {e}"))?;

        // Clear remaining state (model_meta already cleared above)
        self.loading.store(false, Ordering::SeqCst);
        self.loading_progress.store(0, Ordering::Relaxed);

        // Fail any pending requests
        {
            let mut pending = self.pending.lock().await;
            for (_, req) in pending.drain() {
                let _ = req.tx.send(WorkerPayload::Error {
                    message: "Worker process killed".to_string(),
                });
            }
        }

        // Drop active generation
        {
            *self.active_generation.lock().await = None;
        }

        // Restart the worker
        self.process_manager
            .restart()
            .map_err(|e| format!("Failed to restart worker: {e}"))?;

        // Reconnect IO tasks
        self.reconnect_io().await;

        Ok(())
    }

    /// Reconnect stdin/stdout tasks after worker restart.
    async fn reconnect_io(&self) {
        if let Some(stdin) = self.process_manager.take_stdin() {
            let (new_cmd_tx, cmd_rx) = mpsc::unbounded_channel::<String>();
            tokio::spawn(stdin_writer_task(cmd_rx, stdin));
            // Replace the cmd_tx so new commands go to the new worker
            *self.cmd_tx.lock().await = new_cmd_tx;
        }

        if let Some(stdout) = self.process_manager.take_stdout() {
            let gen = self.process_manager.generation();
            tokio::spawn(stdout_reader_task(
                stdout,
                self.pending.clone(),
                self.active_generation.clone(),
                self.model_meta.clone(),
                self.last_model_path.clone(),
                self.loading_progress.clone(),
                self.process_manager.clone(),
                self.cmd_tx.clone(),
                Arc::new(TokioMutex::new(CrashRecoveryCtx::default())),
                self.auto_recovering.clone(),
                self.status_message.clone(),
                gen,
            ));
        }
    }

    /// Generate a short title for a conversation using the loaded model.
    pub async fn generate_title(
        &self,
        conversation_id: &str,
        prompt: &str,
    ) -> Result<String, String> {
        let payload = self
            .send_and_wait(WorkerCommand::GenerateTitle {
                conversation_id: conversation_id.to_string(),
                prompt: prompt.to_string(),
            })
            .await?;

        match payload {
            WorkerPayload::TitleGenerated { title, .. } => Ok(title),
            WorkerPayload::Error { message } => Err(message),
            _ => Err("Unexpected response to GenerateTitle".to_string()),
        }
    }

    /// Force compact a conversation (manual user action).
    pub async fn compact_conversation(&self, conversation_id: &str) -> Result<(), String> {
        const COMPACT_TIMEOUT_SECS: u64 = 3600; // 1 hour — large contexts can take many minutes
        let payload = match timeout(
            Duration::from_secs(COMPACT_TIMEOUT_SECS),
            self.send_and_wait(WorkerCommand::CompactConversation {
                conversation_id: conversation_id.to_string(),
            }),
        )
        .await
        {
            Ok(result) => result?,
            Err(_) => {
                eprintln!("[COMPACT] Timeout after {COMPACT_TIMEOUT_SECS}s — killing worker.");
                let _ = self.force_unload().await;
                return Err(format!(
                    "Compaction timed out after {COMPACT_TIMEOUT_SECS}s. The worker was restarted."
                ));
            }
        };
        match payload {
            WorkerPayload::CompactionDone { .. } => {
                // Clear progress message so the API doesn't return stale compaction status
                *self.status_message.lock().await = None;
                Ok(())
            }
            WorkerPayload::Error { message } => Err(message),
            _ => Err("Unexpected response to CompactConversation".to_string()),
        }
    }

    /// Get cached model status (no IPC round-trip).
    pub async fn model_status(&self) -> Option<ModelMeta> {
        self.model_meta.lock().await.clone()
    }

    /// Get last successfully loaded model path (persists through crash-recovery cycles).
    pub async fn last_model_path(&self) -> Option<String> {
        self.last_model_path.lock().await.clone()
    }

    /// Check if a generation is currently active (streaming tokens or executing tools).
    pub async fn is_generating(&self) -> bool {
        self.active_generation.lock().await.is_some()
    }

    /// Get the conversation ID of the active generation, if any.
    pub async fn active_conversation_id(&self) -> Option<String> {
        self.active_generation
            .lock()
            .await
            .as_ref()?
            .conversation_id
            .clone()
    }

    /// Set a status message visible via the API (e.g. compaction progress).
    pub async fn set_status_message(&self, msg: Option<String>) {
        *self.status_message.lock().await = msg;
    }

    /// Get global status from the worker (compaction progress, etc.).
    /// Uses a short timeout — if worker is busy (e.g. compacting), returns None immediately.
    pub async fn get_global_status(&self) -> Option<String> {
        match timeout(
            Duration::from_millis(200),
            self.send_and_wait(WorkerCommand::GetGlobalStatus),
        )
        .await
        {
            Ok(Ok(WorkerPayload::GlobalStatus { status })) => status,
            _ => None,
        }
    }

    /// Get conversation event log from the worker.
    pub async fn get_conversation_events(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<llama_chat_db::event_log::ConversationEvent>, String> {
        match self
            .send_and_wait(WorkerCommand::GetConversationEvents {
                conversation_id: conversation_id.to_string(),
            })
            .await?
        {
            WorkerPayload::ConversationEvents { events } => Ok(events),
            _ => Ok(Vec::new()),
        }
    }

    /// Get the current status message.
    pub async fn status_message(&self) -> Option<String> {
        self.status_message.lock().await.clone()
    }

    /// Get the last finish reason (non-consuming — cleared on next generation start).
    pub async fn last_finish_reason(&self) -> Option<String> {
        self.last_finish_reason.lock().await.clone()
    }

    /// Store the finish reason when generation completes.
    pub async fn set_last_finish_reason(&self, reason: Option<String>) {
        *self.last_finish_reason.lock().await = reason;
    }

    /// Clear the finish reason (called at generation start).
    pub async fn clear_last_finish_reason(&self) {
        *self.last_finish_reason.lock().await = None;
    }

    /// Start a generation request. Returns a receiver for streaming tokens.
    /// The caller reads `TokenData` from the receiver until it closes.
    pub async fn generate(
        &self,
        user_message: String,
        conversation_id: Option<String>,
        skip_user_logging: bool,
        image_data: Option<Vec<String>>,
    ) -> Result<(mpsc::UnboundedReceiver<TokenData>, oneshot::Receiver<GenerationResult>), String>
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        // Clear previous finish reason
        self.clear_last_finish_reason().await;

        // Create token channel
        let (token_tx, token_rx) = mpsc::unbounded_channel::<TokenData>();

        // Create completion channel
        let (done_tx, done_rx) = oneshot::channel::<GenerationResult>();

        // Register active generation
        {
            let mut gen = self.active_generation.lock().await;
            *gen = Some(ActiveGeneration {
                request_id: id,
                token_tx,
                conversation_id: conversation_id.clone(),
            });
        }

        // Register completion handler
        {
            let mut pending = self.pending.lock().await;
            let active_gen = self.active_generation.clone();
            // We use the pending map to catch the final response
            pending.insert(
                id,
                PendingRequest {
                    tx: oneshot_adapter(done_tx, active_gen, self.last_finish_reason.clone()),
                },
            );
        }

        // Send generate command
        let request = WorkerRequest {
            id,
            command: WorkerCommand::Generate {
                user_message,
                conversation_id,
                skip_user_logging,
                image_data,
            },
        };
        let json =
            serde_json::to_string(&request).map_err(|e| format!("Serialize error: {e}"))?;
        self.cmd_tx
            .lock()
            .await
            .send(json)
            .map_err(|_| "Worker stdin closed".to_string())?;

        Ok((token_rx, done_rx))
    }

    /// Cancel the in-progress generation.
    pub async fn cancel_generation(self: &Arc<Self>) {
        self.send_fire_and_forget(WorkerCommand::CancelGeneration).await;
    }

    /// Refresh MCP server connections in the worker.
    pub async fn refresh_mcp_servers(&self) -> Result<WorkerPayload, String> {
        self.send_and_wait(WorkerCommand::RefreshMcpServers).await
    }

    /// Get current MCP status from the worker.
    pub async fn get_mcp_status(&self) -> Result<WorkerPayload, String> {
        self.send_and_wait(WorkerCommand::GetMcpStatus).await
    }

    /// Get available compute backends from the worker.
    pub async fn get_available_backends(
        &self,
    ) -> Result<Vec<super::ipc_types::BackendInfo>, String> {
        match self
            .send_and_wait(WorkerCommand::GetAvailableBackends)
            .await?
        {
            WorkerPayload::AvailableBackends { backends } => Ok(backends),
            WorkerPayload::Error { message } => Err(message),
            _ => Err("Unexpected response to GetAvailableBackends".to_string()),
        }
    }
}
