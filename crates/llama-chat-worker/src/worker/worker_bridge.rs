//! Server-side abstraction for communicating with the worker process.
//!
//! Replaces `SharedLlamaState + GenerationQueue` in route handlers.
//! Manages stdin/stdout pipes, request/response correlation, and token streaming.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot, Mutex as TokioMutex};
use tokio::time::{timeout, Duration};

use super::ipc_types::*;
use super::process_manager::ProcessManager;
use llama_chat_types::models::TokenData;

/// Cached model metadata from the worker.
#[derive(Debug, Clone)]
pub struct ModelMeta {
    pub loaded: bool,
    pub model_path: String,
    pub context_length: Option<u32>,
    pub chat_template_type: Option<String>,
    pub general_name: Option<String>,
    pub has_vision: bool,
    pub gpu_layers: Option<u32>,
    pub block_count: Option<u32>,
}

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
    /// Model loading progress (0-100), updated by stdout reader from worker IPC.
    loading_progress: Arc<AtomicU8>,
    /// Model path being loaded (for status reporting during load).
    loading_model_path: Arc<TokioMutex<Option<String>>>,
    /// Status message (e.g. "Compacting conversation (5/43)") visible via API.
    status_message: Arc<TokioMutex<Option<String>>>,
    /// Last generation finish reason (for polling-based auto-continue).
    last_finish_reason: Arc<TokioMutex<Option<String>>>,
    /// Next request ID counter.
    next_id: AtomicU64,
    /// Process manager for kill/restart.
    process_manager: Arc<ProcessManager>,
}

struct PendingRequest {
    tx: oneshot::Sender<WorkerPayload>,
}

struct ActiveGeneration {
    request_id: u64,
    token_tx: mpsc::UnboundedSender<TokenData>,
    conversation_id: Option<String>,
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
        tokio::spawn(stdout_reader_task(
            stdout_handle,
            pending.clone(),
            active_generation.clone(),
            model_meta.clone(),
            loading_progress.clone(),
            process_manager.clone(),
            cmd_tx_arc.clone(),
        ));

        Self {
            cmd_tx: cmd_tx_arc,
            pending,
            active_generation,
            model_meta,
            loading: AtomicBool::new(false),
            loading_progress,
            loading_model_path: Arc::new(TokioMutex::new(None)),
            status_message: Arc::new(TokioMutex::new(None)),
            last_finish_reason: Arc::new(TokioMutex::new(None)),
            next_id: AtomicU64::new(1),
            process_manager,
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
    pub async fn load_model(&self, model_path: &str, gpu_layers: Option<u32>, mmproj_path: Option<String>) -> Result<ModelMeta, String> {
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
                eprintln!("[LOAD] Timeout after {LOAD_TIMEOUT_SECS}s — likely VRAM overflow. Killing worker.");
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
                general_name,
                has_vision,
                gpu_layers,
                block_count,
                ..
            } => {
                let meta = ModelMeta {
                    loaded: true,
                    model_path,
                    context_length,
                    chat_template_type,
                    general_name,
                    has_vision: has_vision.unwrap_or(false),
                    gpu_layers,
                    block_count,
                };
                *self.model_meta.lock().await = Some(meta.clone());
                Ok(meta)
            }
            WorkerPayload::Error { message } => Err(message),
            _ => Err("Unexpected response to LoadModel".to_string()),
        }
    }

    /// Check if a model is currently being loaded.
    pub fn is_loading(&self) -> bool {
        self.loading.load(Ordering::SeqCst)
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
        // Kill the worker (blocking call — use spawn_blocking to avoid stalling the runtime)
        let pm = self.process_manager.clone();
        tokio::task::spawn_blocking(move || pm.kill())
            .await
            .map_err(|e| format!("Kill task failed: {e}"))?;

        // Clear cached state
        *self.model_meta.lock().await = None;
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
            tokio::spawn(stdout_reader_task(
                stdout,
                self.pending.clone(),
                self.active_generation.clone(),
                self.model_meta.clone(),
                self.loading_progress.clone(),
                self.process_manager.clone(),
                self.cmd_tx.clone(),
            ));
        }
    }

    /// Generate a short title for a conversation using the loaded model.
    pub async fn generate_title(&self, conversation_id: &str, prompt: &str) -> Result<String, String> {
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

    /// Get cached model status (no IPC round-trip).
    pub async fn model_status(&self) -> Option<ModelMeta> {
        self.model_meta.lock().await.clone()
    }

    /// Check if a generation is currently active (streaming tokens or executing tools).
    pub async fn is_generating(&self) -> bool {
        self.active_generation.lock().await.is_some()
    }

    /// Get the conversation ID of the active generation, if any.
    pub async fn active_conversation_id(&self) -> Option<String> {
        self.active_generation.lock().await.as_ref()?.conversation_id.clone()
    }

    /// Set a status message visible via the API (e.g. compaction progress).
    pub async fn set_status_message(&self, msg: Option<String>) {
        *self.status_message.lock().await = msg;
    }

    /// Get global status from the worker (compaction progress, etc.).
    pub async fn get_global_status(&self) -> Option<String> {
        match self.send_and_wait(WorkerCommand::GetGlobalStatus).await {
            Ok(WorkerPayload::GlobalStatus { status }) => status,
            _ => None,
        }
    }

    /// Get conversation event log from the worker.
    pub async fn get_conversation_events(&self, conversation_id: &str) -> Result<Vec<llama_chat_db::event_log::ConversationEvent>, String> {
        match self.send_and_wait(WorkerCommand::GetConversationEvents {
            conversation_id: conversation_id.to_string(),
        }).await? {
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

        // NOTE: Cancel watchdog disabled — it was killing new generations
        // that started after a previous cancel. The watchdog checked
        // is_generating() after 5s but couldn't distinguish between
        // "old generation that should stop" and "new generation that just started".
        // TODO: track generation ID to fix this properly.
        /*
        let bridge = Arc::clone(self);
        tokio::spawn(async move {
            // Give the worker 5 seconds to stop gracefully
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;

            if !bridge.is_generating().await {
                return; // Worker stopped in time — all good
            }

            eprintln!("[CANCEL_WATCHDOG] Worker still generating after 5s — force-restarting");

            // Remember the current model path so we can reload after restart
            let model_path = bridge
                .model_status()
                .await
                .map(|m| m.model_path.clone());

            // Kill and restart the worker process
            if let Err(e) = bridge.force_unload().await {
                eprintln!("[CANCEL_WATCHDOG] force_unload failed: {e}");
                return;
            }

            // Auto-reload the same model if one was loaded
            if let Some(path) = model_path {
                eprintln!("[CANCEL_WATCHDOG] Auto-reloading model: {path}");
                bridge.set_status_message(Some("Reloading model after cancel...".to_string())).await;
                let load_cmd = WorkerCommand::LoadModel {
                    model_path: path,
                    gpu_layers: None,
                    mmproj_path: None,
                };
                let _ = bridge.send_and_wait(load_cmd).await;
                bridge.set_status_message(None).await;
            }
        });
        */
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
    pub async fn get_available_backends(&self) -> Result<Vec<super::ipc_types::BackendInfo>, String> {
        match self.send_and_wait(WorkerCommand::GetAvailableBackends).await? {
            WorkerPayload::AvailableBackends { backends } => Ok(backends),
            WorkerPayload::Error { message } => Err(message),
            _ => Err("Unexpected response to GetAvailableBackends".to_string()),
        }
    }
}

/// Result of a completed generation.
#[derive(Debug)]
#[allow(dead_code)]
pub enum GenerationResult {
    Complete {
        conversation_id: String,
        tokens_used: i32,
        max_tokens: i32,
        prompt_tok_per_sec: Option<f64>,
        gen_tok_per_sec: Option<f64>,
        gen_eval_ms: Option<f64>,
        gen_tokens: Option<i32>,
        prompt_eval_ms: Option<f64>,
        prompt_tokens: Option<i32>,
        finish_reason: Option<String>,
        token_breakdown: Option<llama_chat_types::models::TokenBreakdown>,
    },
    Cancelled,
    Error(String),
}

/// Adapt a GenerationResult oneshot into a WorkerPayload oneshot for the pending map.
fn oneshot_adapter(
    done_tx: oneshot::Sender<GenerationResult>,
    active_gen: Arc<TokioMutex<Option<ActiveGeneration>>>,
    finish_reason_store: Arc<TokioMutex<Option<String>>>,
) -> oneshot::Sender<WorkerPayload> {
    let (payload_tx, payload_rx) = oneshot::channel::<WorkerPayload>();

    tokio::spawn(async move {
        if let Ok(payload) = payload_rx.await {
            // Clear active generation
            *active_gen.lock().await = None;

            let result = match payload {
                WorkerPayload::GenerationComplete {
                    conversation_id,
                    tokens_used,
                    max_tokens,
                    prompt_tok_per_sec,
                    gen_tok_per_sec,
                    gen_eval_ms,
                    gen_tokens,
                    prompt_eval_ms,
                    prompt_tokens,
                    finish_reason,
                    token_breakdown,
                } => {
                    // Store finish_reason for polling-based auto-continue
                    *finish_reason_store.lock().await = finish_reason.clone();
                    GenerationResult::Complete {
                        conversation_id,
                        tokens_used,
                        max_tokens,
                        prompt_tok_per_sec,
                        gen_tok_per_sec,
                        gen_eval_ms,
                        gen_tokens,
                        prompt_eval_ms,
                        prompt_tokens,
                        finish_reason,
                        token_breakdown,
                    }
                },
                WorkerPayload::GenerationCancelled => {
                    *finish_reason_store.lock().await = Some("cancelled".to_string());
                    GenerationResult::Cancelled
                },
                WorkerPayload::Error { message } => {
                    *finish_reason_store.lock().await = Some("error".to_string());
                    GenerationResult::Error(message)
                },
                _ => {
                    *finish_reason_store.lock().await = Some("error".to_string());
                    GenerationResult::Error("Unexpected response".to_string())
                },
            };
            let _ = done_tx.send(result);
        }
    });

    payload_tx
}

/// Task that writes commands to the worker's stdin.
async fn stdin_writer_task(
    mut cmd_rx: mpsc::UnboundedReceiver<String>,
    mut stdin: std::process::ChildStdin,
) {
    while let Some(json_line) = cmd_rx.recv().await {
        if writeln!(stdin, "{json_line}").is_err() {
            eprintln!("[BRIDGE] Failed to write to worker stdin");
            break;
        }
        if stdin.flush().is_err() {
            eprintln!("[BRIDGE] Failed to flush worker stdin");
            break;
        }
    }
    eprintln!("[BRIDGE] Stdin writer task exiting");
}

/// Task that reads responses from the worker's stdout and dispatches them.
async fn stdout_reader_task(
    stdout: std::process::ChildStdout,
    pending: Arc<TokioMutex<HashMap<u64, PendingRequest>>>,
    active_generation: Arc<TokioMutex<Option<ActiveGeneration>>>,
    model_meta: Arc<TokioMutex<Option<ModelMeta>>>,
    loading_progress: Arc<AtomicU8>,
    process_manager: Arc<super::process_manager::ProcessManager>,
    cmd_tx: Arc<TokioMutex<mpsc::UnboundedSender<String>>>,
) {
    // Read stdout on a blocking thread (pipe reads are blocking on Windows)
    let (line_tx, mut line_rx) = mpsc::unbounded_channel::<String>();

    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(l) if !l.trim().is_empty() => {
                    if line_tx.send(l).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("[BRIDGE] Worker stdout read error: {e}");
                    break;
                }
            }
        }
        eprintln!("[BRIDGE] Stdout reader thread exiting");
    });

    // Process lines on the async side
    while let Some(line) = line_rx.recv().await {
        let response: WorkerResponse = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[BRIDGE] Failed to parse worker response: {e}");
                continue;
            }
        };

        let id = response.id;
        let payload = response.payload;

        // Handle streaming tokens (sent to active generation channel)
        // Move-destructure to avoid cloning the token String
        if let WorkerPayload::Token {
            token,
            tokens_used,
            max_tokens,
            status,
        } = payload
        {
            let gen = active_generation.lock().await;
            if let Some(ref ag) = *gen {
                if ag.request_id == id {
                    let _ = ag.token_tx.send(TokenData {
                        token,
                        tokens_used,
                        max_tokens,
                        status,
                        ..Default::default()
                    });
                    continue;
                }
            }
            continue;
        }

        // Handle generation started — update active conversation ID
        if let WorkerPayload::GenerationStarted { conversation_id } = &payload {
            let mut gen = active_generation.lock().await;
            if let Some(ref mut ag) = *gen {
                if ag.request_id == id {
                    ag.conversation_id = Some(conversation_id.clone());
                }
            }
            continue;
        }

        // Handle loading progress — update atomic, don't dispatch
        if let WorkerPayload::LoadingProgress { progress } = payload {
            loading_progress.store(progress, Ordering::Relaxed);
            continue;
        }

        // Handle model loaded — update cached metadata
        if let WorkerPayload::ModelLoaded { .. } = &payload {
            // Metadata is updated by the load_model method, not here
        }

        // Handle model unloaded — clear cached metadata
        if let WorkerPayload::ModelUnloaded = &payload {
            *model_meta.lock().await = None;
        }

        // Dispatch to pending request
        let mut pending_guard = pending.lock().await;
        if let Some(req) = pending_guard.remove(&id) {
            let _ = req.tx.send(payload);
        } else if id != 0 {
            eprintln!("[BRIDGE] No pending request for response id={id}");
        }
    }

    // Worker died — clear state and auto-restart.
    {
        // Clear active generation so the UI stops showing the spinner
        let mut gen = active_generation.lock().await;
        if let Some(ag) = gen.take() {
            eprintln!("[BRIDGE] Worker died during generation — clearing active generation");
            let _ = ag.token_tx.send(TokenData {
                token: "\n\n[Worker process crashed — restarting automatically.]".to_string(),
                tokens_used: 0,
                max_tokens: 0,
                status: None,
                ..Default::default()
            });
            let mut pending_guard = pending.lock().await;
            if let Some(req) = pending_guard.remove(&ag.request_id) {
                let _ = req.tx.send(WorkerPayload::Error {
                    message: "Worker process crashed during generation".to_string(),
                });
            }
        }

        // Save model path for auto-reload after restart
        let crashed_model = model_meta.lock().await.as_ref().map(|m| (m.model_path.clone(), m.gpu_layers));
        // Clear model metadata
        *model_meta.lock().await = None;
        loading_progress.store(0, Ordering::Relaxed);

        // Fail any other pending requests
        {
            let mut pending_guard = pending.lock().await;
            for (_, req) in pending_guard.drain() {
                let _ = req.tx.send(WorkerPayload::Error {
                    message: "Worker process crashed".to_string(),
                });
            }
        }

        // Auto-restart the worker process and reconnect IO.
        // Runs on a new thread with its own tokio runtime since the original
        // task context may not be available after worker death.
        eprintln!("[BRIDGE] Auto-restarting worker process...");
        if let Err(e) = process_manager.restart() {
            eprintln!("[BRIDGE] Failed to restart worker: {e}");
        } else {
            eprintln!("[BRIDGE] Worker restarted successfully");
            let stdin_opt = process_manager.take_stdin();
            let stdout_opt = process_manager.take_stdout();
            let p = pending.clone();
            let ag = active_generation.clone();
            let mm = model_meta.clone();
            let lp = loading_progress.clone();
            let pm = process_manager.clone();
            let ct = cmd_tx.clone();
            let reload_model = crashed_model.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to build recovery runtime");
                rt.block_on(async move {
                    // Reconnect stdin writer
                    if let Some(stdin) = stdin_opt {
                        let (new_cmd_tx, new_cmd_rx) = mpsc::unbounded_channel::<String>();
                        tokio::spawn(stdin_writer_task(new_cmd_rx, stdin));
                        *ct.lock().await = new_cmd_tx;
                        eprintln!("[BRIDGE] Stdin writer reconnected");
                    }
                    // Auto-reload the model that was loaded before the crash
                    if let Some((model_path, gpu_layers)) = reload_model {
                        eprintln!("[BRIDGE] Auto-reloading model: {} (gpu_layers={:?})", model_path, gpu_layers);
                        let load_cmd = serde_json::json!({
                            "id": 0,
                            "command": {
                                "LoadModel": {
                                    "model_path": model_path,
                                    "gpu_layers": gpu_layers,
                                    "mmproj_path": null
                                }
                            }
                        });
                        let tx = ct.lock().await;
                        let _ = tx.send(load_cmd.to_string());
                    }
                    // Reconnect stdout reader (blocks until next worker death)
                    if let Some(stdout) = stdout_opt {
                        eprintln!("[BRIDGE] Stdout reader reconnected");
                        stdout_reader_task(stdout, p, ag, mm, lp, pm, ct).await;
                    }
                });
            });
        }
    }
    eprintln!("[BRIDGE] Stdout reader task exiting (old worker)");
}
