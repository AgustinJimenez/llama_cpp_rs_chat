//! Server-side abstraction for communicating with the worker process.
//!
//! Replaces `SharedLlamaState + GenerationQueue` in route handlers.
//! Manages stdin/stdout pipes, request/response correlation, and token streaming.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot, Mutex as TokioMutex};

use super::ipc_types::*;
use super::process_manager::ProcessManager;
use crate::web::models::TokenData;

/// Cached model metadata from the worker.
#[derive(Debug, Clone)]
pub struct ModelMeta {
    pub loaded: bool,
    pub model_path: String,
    pub context_length: Option<u32>,
    pub chat_template_type: Option<String>,
    pub chat_template_string: Option<String>,
    pub gpu_layers: Option<u32>,
    pub general_name: Option<String>,
    pub default_system_prompt: Option<String>,
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

        // Stdin writer task
        tokio::spawn(stdin_writer_task(cmd_rx, stdin_handle));

        // Stdout reader task
        tokio::spawn(stdout_reader_task(
            stdout_handle,
            pending.clone(),
            active_generation.clone(),
            model_meta.clone(),
        ));

        Self {
            cmd_tx: Arc::new(TokioMutex::new(cmd_tx)),
            pending,
            active_generation,
            model_meta,
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
    pub async fn load_model(&self, model_path: &str) -> Result<ModelMeta, String> {
        let payload = self
            .send_and_wait(WorkerCommand::LoadModel {
                model_path: model_path.to_string(),
            })
            .await?;

        match payload {
            WorkerPayload::ModelLoaded {
                model_path,
                context_length,
                chat_template_type,
                chat_template_string,
                gpu_layers,
                general_name,
                default_system_prompt,
            } => {
                let meta = ModelMeta {
                    loaded: true,
                    model_path,
                    context_length,
                    chat_template_type,
                    chat_template_string,
                    gpu_layers,
                    general_name,
                    default_system_prompt,
                };
                *self.model_meta.lock().await = Some(meta.clone());
                Ok(meta)
            }
            WorkerPayload::Error { message } => Err(message),
            _ => Err("Unexpected response to LoadModel".to_string()),
        }
    }

    /// Unload the model (within the worker process).
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
            ));
        }
    }

    /// Get cached model status (no IPC round-trip).
    pub async fn model_status(&self) -> Option<ModelMeta> {
        self.model_meta.lock().await.clone()
    }

    /// Start a generation request. Returns a receiver for streaming tokens.
    /// The caller reads `TokenData` from the receiver until it closes.
    pub async fn generate(
        &self,
        user_message: String,
        conversation_id: Option<String>,
        skip_user_logging: bool,
    ) -> Result<(mpsc::UnboundedReceiver<TokenData>, oneshot::Receiver<GenerationResult>), String>
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

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
                    tx: oneshot_adapter(done_tx, active_gen),
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
    pub async fn cancel_generation(&self) {
        self.send_fire_and_forget(WorkerCommand::CancelGeneration).await;
    }

    /// Check if worker process is alive.
    pub fn is_alive(&self) -> bool {
        self.process_manager.is_alive()
    }
}

/// Result of a completed generation.
#[derive(Debug)]
pub enum GenerationResult {
    Complete {
        conversation_id: String,
        tokens_used: i32,
        max_tokens: i32,
    },
    Cancelled,
    Error(String),
}

/// Adapt a GenerationResult oneshot into a WorkerPayload oneshot for the pending map.
fn oneshot_adapter(
    done_tx: oneshot::Sender<GenerationResult>,
    active_gen: Arc<TokioMutex<Option<ActiveGeneration>>>,
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
                } => GenerationResult::Complete {
                    conversation_id,
                    tokens_used,
                    max_tokens,
                },
                WorkerPayload::GenerationCancelled => GenerationResult::Cancelled,
                WorkerPayload::Error { message } => GenerationResult::Error(message),
                _ => GenerationResult::Error("Unexpected response".to_string()),
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
        if let WorkerPayload::Token {
            ref token,
            tokens_used,
            max_tokens,
        } = payload
        {
            let gen = active_generation.lock().await;
            if let Some(ref ag) = *gen {
                if ag.request_id == id {
                    let _ = ag.token_tx.send(TokenData {
                        token: token.clone(),
                        tokens_used,
                        max_tokens,
                    });
                    continue;
                }
            }
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

    eprintln!("[BRIDGE] Stdout reader task exiting");
}
