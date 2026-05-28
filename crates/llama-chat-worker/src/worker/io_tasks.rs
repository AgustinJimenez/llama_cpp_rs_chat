//! Async IO tasks that manage the worker process's stdin/stdout pipes.
//!
//! These are free functions (not methods on WorkerBridge) — all state is
//! passed as explicit parameters so they can be spawned on independent tasks
//! and survive crash-recovery cycles without holding a reference to the bridge.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot, Mutex as TokioMutex};

use super::ipc_types::*;
use super::worker_bridge::{ActiveGeneration, ModelMeta, PendingRequest};
use llama_chat_types::models::TokenData;

pub const MAX_AUTO_RECOVERY_CRASHES: u32 = 2;

/// Persistent state that survives across crash-recovery cycles.
#[derive(Clone, Default)]
pub struct CrashRecoveryCtx {
    pub model_path: Option<String>,
    pub gpu_layers: Option<u32>,
    pub conversation_id: Option<String>,
    pub crash_count: u32,
}

/// Task that writes serialized commands to the worker's stdin.
pub async fn stdin_writer_task(
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
/// Persists across crash-recovery cycles so auto-continue works on repeated crashes.
///
/// `my_generation` is the ProcessManager generation at spawn time.  If the
/// generation has advanced by the time the worker crashes, this reader is
/// stale — a newer reader already owns the process — so crash recovery is
/// skipped and the task exits silently.
pub async fn stdout_reader_task(
    stdout: std::process::ChildStdout,
    pending: Arc<TokioMutex<HashMap<u64, PendingRequest>>>,
    active_generation: Arc<TokioMutex<Option<ActiveGeneration>>>,
    model_meta: Arc<TokioMutex<Option<ModelMeta>>>,
    last_model_path: Arc<TokioMutex<Option<String>>>,
    loading_progress: Arc<AtomicU8>,
    process_manager: Arc<super::process_manager::ProcessManager>,
    cmd_tx: Arc<TokioMutex<mpsc::UnboundedSender<String>>>,
    recovery_ctx: Arc<TokioMutex<CrashRecoveryCtx>>,
    auto_recovering: Arc<AtomicBool>,
    status_message: Arc<TokioMutex<Option<String>>>,
    my_generation: u64,
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
            tool_timing,
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
                        tool_timing,
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

        // Handle model loaded — always update cached metadata
        // (needed for auto-reload after watchdog kill where there's no pending load_model request)
        if let WorkerPayload::ModelLoaded {
            model_path,
            context_length,
            chat_template_type,
            chat_template_string,
            general_name,
            has_vision,
            gpu_layers,
            block_count,
        } = &payload
        {
            let supports_thinking = chat_template_string
                .as_deref()
                .map(llama_chat_engine::jinja_templates::detect_thinking_support)
                .unwrap_or(false);
            *last_model_path.lock().await = Some(model_path.clone());
            *model_meta.lock().await = Some(ModelMeta {
                loaded: true,
                model_path: model_path.clone(),
                context_length: *context_length,
                chat_template_type: chat_template_type.clone(),
                general_name: general_name.clone(),
                has_vision: has_vision.unwrap_or(false),
                gpu_layers: *gpu_layers,
                block_count: *block_count,
                supports_thinking,
            });
            eprintln!("[BRIDGE] Model metadata cached: {}", model_path);
        }

        // Handle model unloaded — clear cached metadata
        if let WorkerPayload::ModelUnloaded = &payload {
            *model_meta.lock().await = None;
        }

        // Handle unsolicited status updates (id=0) without dispatching to pending
        if let WorkerPayload::StatusUpdate { ref message } = payload {
            *status_message.lock().await = Some(message.clone());
            continue;
        }

        // Dispatch to pending request
        let mut pending_guard = pending.lock().await;
        if let Some(req) = pending_guard.remove(&id) {
            let _ = req.tx.send(payload);
        } else if id != 0 {
            eprintln!("[BRIDGE] No pending request for response id={id}");
        }
    }

    // Worker died — check generation before running crash recovery.
    // If the generation has advanced, a newer reader already owns the process;
    // this reader is stale and should exit without restarting anything.
    if process_manager.generation() != my_generation {
        eprintln!(
            "[BRIDGE] Stale reader (gen={my_generation}, current={}) — skipping crash recovery",
            process_manager.generation()
        );
        return;
    }

    // Worker died — save crash context, clear state, auto-restart + reload + continue.
    {
        // Update recovery context: save model/conversation info on first crash,
        // reuse existing context on subsequent crashes (persists across cycles).
        let (crash_count, has_model) = {
            let mut ctx = recovery_ctx.lock().await;
            // Save model info from meta (only if we have it — first crash)
            if let Some(meta) = model_meta.lock().await.as_ref() {
                ctx.model_path = Some(meta.model_path.clone());
                ctx.gpu_layers = meta.gpu_layers;
            }
            // Save conversation ID from active generation (if any)
            if let Some(conv_id) = active_generation
                .lock()
                .await
                .as_ref()
                .and_then(|ag| ag.conversation_id.clone())
            {
                ctx.conversation_id = Some(conv_id);
            }
            ctx.crash_count += 1;
            eprintln!(
                "[BRIDGE] Crash #{} — model={:?} conv={:?}",
                ctx.crash_count,
                ctx.model_path.as_deref(),
                ctx.conversation_id.as_deref()
            );
            (ctx.crash_count, ctx.model_path.is_some())
        };

        // If within auto-recovery limit, keep the UI spinner and don't show crash message.
        // Otherwise, clear generation and notify the UI.
        let will_auto_recover = crash_count <= MAX_AUTO_RECOVERY_CRASHES && has_model;
        if !will_auto_recover {
            let mut gen = active_generation.lock().await;
            if let Some(ag) = gen.take() {
                let _ = ag.token_tx.send(TokenData {
                    token: "\n\n[Worker process crashed — restarting automatically.]".to_string(),
                    tokens_used: 0,
                    max_tokens: 0,
                    status: None,
                    ..Default::default()
                });
                if let Some(req) = pending.lock().await.remove(&ag.request_id) {
                    let _ = req.tx.send(WorkerPayload::Error {
                        message: "Worker process crashed during generation".to_string(),
                    });
                }
            }
        } else {
            // Drop the old active generation channels (they're connected to the dead worker)
            // but don't send any crash message to the UI
            let mut gen = active_generation.lock().await;
            if let Some(ag) = gen.take() {
                // Resolve the pending request silently so it doesn't hang
                if let Some(req) = pending.lock().await.remove(&ag.request_id) {
                    let _ = req.tx.send(WorkerPayload::Error {
                        message: "auto_recovery".to_string(),
                    });
                }
            }
        }

        // Clear model metadata
        *model_meta.lock().await = None;
        loading_progress.store(0, Ordering::Relaxed);

        // Fail any other pending requests
        for (_, req) in pending.lock().await.drain() {
            let _ = req.tx.send(WorkerPayload::Error {
                message: "Worker process crashed".to_string(),
            });
        }

        // Check crash limit
        let ctx = recovery_ctx.lock().await.clone();
        if ctx.crash_count > MAX_AUTO_RECOVERY_CRASHES {
            eprintln!(
                "[BRIDGE] Max auto-recovery crashes ({MAX_AUTO_RECOVERY_CRASHES}) exceeded — giving up"
            );
            // Still restart worker but don't auto-continue
            let _ = process_manager.restart();
            let stdin_opt = process_manager.take_stdin();
            let stdout_opt = process_manager.take_stdout();
            let ct = cmd_tx.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let local = tokio::task::LocalSet::new();
                local.block_on(&rt, async move {
                    if let Some(stdin) = stdin_opt {
                        let (new_cmd_tx, new_cmd_rx) = mpsc::unbounded_channel::<String>();
                        tokio::task::spawn_local(stdin_writer_task(new_cmd_rx, stdin));
                        *ct.lock().await = new_cmd_tx;
                    }
                    if let Some(stdout) = stdout_opt {
                        let rc = Arc::new(TokioMutex::new(CrashRecoveryCtx::default()));
                        let ar = Arc::new(AtomicBool::new(false));
                        let gen = process_manager.generation();
                        stdout_reader_task(
                            stdout,
                            pending,
                            active_generation,
                            model_meta,
                            last_model_path,
                            loading_progress,
                            process_manager,
                            ct,
                            rc,
                            ar,
                            status_message.clone(),
                            gen,
                        )
                        .await;
                    }
                });
            });
        } else {
            // Auto-restart the worker process, reconnect IO, reload model, continue generation.
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
                let lmp = last_model_path.clone();
                let lp = loading_progress.clone();
                let pm = process_manager.clone();
                let ct = cmd_tx.clone();
                let rc = recovery_ctx.clone();
                let ar = auto_recovering.clone();
                let sm = status_message.clone();
                // Set auto_recovering flag so frontend doesn't race with a duplicate load
                ar.store(true, Ordering::SeqCst);
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("Failed to build recovery runtime");
                    let local = tokio::task::LocalSet::new();
                    local.block_on(&rt, async move {
                        // Reconnect stdin writer
                        if let Some(stdin) = stdin_opt {
                            let (new_cmd_tx, new_cmd_rx) = mpsc::unbounded_channel::<String>();
                            tokio::task::spawn_local(stdin_writer_task(new_cmd_rx, stdin));
                            *ct.lock().await = new_cmd_tx;
                            eprintln!("[BRIDGE] Stdin writer reconnected");
                        }

                        // Auto-reload + auto-continue using persistent recovery context
                        if let Some(ref model_path) = ctx.model_path {
                            eprintln!(
                                "[BRIDGE] Auto-reloading model: {model_path} (crash #{})",
                                ctx.crash_count
                            );
                            let load_id: u64 = 900_000 + ctx.crash_count as u64;
                            let load_req = WorkerRequest {
                                id: load_id,
                                command: WorkerCommand::LoadModel {
                                    model_path: model_path.clone(),
                                    gpu_layers: ctx.gpu_layers,
                                    mmproj_path: None,
                                },
                            };
                            if let Ok(json) = serde_json::to_string(&load_req) {
                                let (load_tx, load_rx) = oneshot::channel::<WorkerPayload>();
                                p.lock().await.insert(load_id, PendingRequest { tx: load_tx });
                                let _ = ct.lock().await.send(json);

                                // Spawn stdout reader (ChildStdout isn't Send — use spawn_local)
                                // Keep a handle so we can await it to keep this thread alive
                                let stdout_handle = if let Some(stdout) = stdout_opt {
                                    let p2 = p.clone();
                                    let ag2 = ag.clone();
                                    let mm2 = mm.clone();
                                    let lmp2 = lmp.clone();
                                    let lp2 = lp.clone();
                                    let pm2 = pm.clone();
                                    let ct2 = ct.clone();
                                    let rc2 = rc.clone();
                                    let ar2 = ar.clone();
                                    let sm2 = sm.clone();
                                    let gen2 = pm2.generation();
                                    Some(tokio::task::spawn_local(async move {
                                        stdout_reader_task(
                                            stdout, p2, ag2, mm2, lmp2, lp2, pm2, ct2, rc2, ar2,
                                            sm2, gen2,
                                        )
                                        .await;
                                    }))
                                } else {
                                    None
                                };

                                // Wait for model load
                                match tokio::time::timeout(
                                    std::time::Duration::from_secs(120),
                                    load_rx,
                                )
                                .await
                                {
                                    Ok(Ok(WorkerPayload::ModelLoaded { .. })) => {
                                        eprintln!("[BRIDGE] Model auto-reloaded successfully");
                                        // Clear auto_recovering — model is loaded, frontend won't race
                                        ar.store(false, Ordering::SeqCst);
                                        if let Some(ref conv_id) = ctx.conversation_id {
                                            eprintln!(
                                                "[BRIDGE] Auto-continuing generation for {conv_id} (crash #{})",
                                                ctx.crash_count
                                            );
                                            let gen_id: u64 = 910_000 + ctx.crash_count as u64;

                                            // Register as active generation so status API reports
                                            // active_conversation_id (sidebar green dot, frontend reconnect)
                                            let (token_tx, _token_rx) =
                                                mpsc::unbounded_channel::<TokenData>();
                                            *ag.lock().await = Some(ActiveGeneration {
                                                request_id: gen_id,
                                                token_tx,
                                                conversation_id: Some(conv_id.clone()),
                                            });

                                            let gen_req = WorkerRequest {
                                                id: gen_id,
                                                command: WorkerCommand::Generate {
                                                    user_message: "Continue from where you left off.".to_string(),
                                                    conversation_id: Some(conv_id.clone()),
                                                    skip_user_logging: true,
                                                    image_data: None,
                                                },
                                            };
                                            if let Ok(json) = serde_json::to_string(&gen_req) {
                                                // Register a pending request so the stdout reader
                                                // can match the response when generation completes
                                                let (gen_tx, _gen_rx) =
                                                    oneshot::channel::<WorkerPayload>();
                                                p.lock()
                                                    .await
                                                    .insert(gen_id, PendingRequest { tx: gen_tx });
                                                let _ = ct.lock().await.send(json);
                                                eprintln!("[BRIDGE] Auto-continue command sent");
                                            }
                                        }
                                    }
                                    Ok(Ok(WorkerPayload::Error { message })) => {
                                        eprintln!("[BRIDGE] Auto-reload failed: {message}");
                                        ar.store(false, Ordering::SeqCst);
                                    }
                                    _ => {
                                        eprintln!(
                                            "[BRIDGE] Auto-reload: timeout or unexpected response"
                                        );
                                        ar.store(false, Ordering::SeqCst);
                                    }
                                }

                                // Keep this thread alive until the stdout reader exits.
                                // Without this, block_on returns → LocalSet drops → stdout reader
                                // is killed → bridge loses connection to the worker.
                                if let Some(handle) = stdout_handle {
                                    eprintln!(
                                        "[BRIDGE] Recovery thread waiting for stdout reader..."
                                    );
                                    let _ = handle.await;
                                }
                            }
                        } else {
                            ar.store(false, Ordering::SeqCst);
                            // No model to reload — just reconnect stdout reader
                            if let Some(stdout) = stdout_opt {
                                eprintln!(
                                    "[BRIDGE] Stdout reader reconnected (no model to reload)"
                                );
                                let gen = pm.generation();
                                stdout_reader_task(stdout, p, ag, mm, lmp, lp, pm, ct, rc, ar, sm.clone(), gen)
                                    .await;
                            }
                        }
                    });
                });
            }
        }
    }
    eprintln!("[BRIDGE] Stdout reader task exiting (old worker)");
}
