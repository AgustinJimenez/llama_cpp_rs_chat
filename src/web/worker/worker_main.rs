//! Worker process entry point.
//!
//! Runs as a child process spawned by the web server. Reads JSON commands
//! from stdin, runs model operations, and writes JSON responses to stdout.
//! All log output goes to stderr (inherited by parent).
//!
//! Thread design:
//! - Thread 0 (stdin reader): reads lines → stdin_rx channel
//! - Thread 1 (main loop): selects between stdin_rx and token_rx, writes to stdout
//! - Thread 2 (generation, temporary): runs generate_llama_response, sends tokens

use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use crossbeam_channel::{self, Receiver, Sender, TryRecvError};

use super::ipc_types::*;
use crate::web::chat::generate_llama_response;
use crate::web::database::{Database, SharedDatabase};
use crate::web::model_manager::{get_model_status, load_model};
use crate::web::models::{SharedLlamaState, TokenData};

/// Run the worker process. This function never returns normally.
pub fn run_worker(db_path: &str) {
    eprintln!("[WORKER] Starting model worker process (pid={})", std::process::id());

    // Open database
    let db: SharedDatabase = Arc::new(
        Database::new(db_path).expect("Worker: failed to open database"),
    );
    eprintln!("[WORKER] Database opened: {db_path}");

    // LlamaState — owned directly, wrapped in Arc<Mutex> for generate_llama_response compatibility
    let llama_state: SharedLlamaState = Arc::new(Mutex::new(None));

    // Channels
    let (stdin_tx, stdin_rx): (Sender<String>, Receiver<String>) = crossbeam_channel::unbounded();
    let (token_tx, token_rx): (Sender<WorkerResponse>, Receiver<WorkerResponse>) =
        crossbeam_channel::unbounded();

    // Cancellation flag for generation
    let cancel_flag = Arc::new(AtomicBool::new(false));

    // Thread 0: stdin reader
    thread::spawn(move || {
        let stdin = io::stdin();
        let reader = stdin.lock();
        for line in reader.lines() {
            match line {
                Ok(l) if !l.trim().is_empty() => {
                    if stdin_tx.send(l).is_err() {
                        break; // Main loop exited
                    }
                }
                Ok(_) => {} // Empty line, skip
                Err(_) => break, // stdin closed (parent died)
            }
        }
        eprintln!("[WORKER] Stdin reader thread exiting");
    });

    // Main loop (Thread 1)
    let mut generation_thread: Option<thread::JoinHandle<()>> = None;
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    eprintln!("[WORKER] Ready, waiting for commands...");

    loop {
        // Drain token channel → write to stdout
        loop {
            match token_rx.try_recv() {
                Ok(response) => {
                    write_response(&mut stdout, &response);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }

        // Check if generation thread finished
        if let Some(ref handle) = generation_thread {
            if handle.is_finished() {
                generation_thread = None;
            }
        }

        // Try to read a command (with timeout to keep draining tokens)
        let line = match stdin_rx.recv_timeout(std::time::Duration::from_millis(5)) {
            Ok(l) => l,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                eprintln!("[WORKER] Stdin channel disconnected, shutting down");
                break;
            }
        };

        // Parse command
        let request: WorkerRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[WORKER] Failed to parse command: {e}");
                write_response(
                    &mut stdout,
                    &WorkerResponse::error(0, format!("Parse error: {e}")),
                );
                continue;
            }
        };

        let req_id = request.id;

        match request.command {
            WorkerCommand::Ping => {
                write_response(&mut stdout, &WorkerResponse::ok(req_id, WorkerPayload::Pong));
            }

            WorkerCommand::Shutdown => {
                eprintln!("[WORKER] Shutdown requested");
                // Cancel any in-progress generation
                cancel_flag.store(true, Ordering::SeqCst);
                if let Some(handle) = generation_thread.take() {
                    let _ = handle.join();
                }
                write_response(&mut stdout, &WorkerResponse::ok(req_id, WorkerPayload::Pong));
                break;
            }

            WorkerCommand::LoadModel { model_path, gpu_layers } => {
                if generation_thread.is_some() {
                    write_response(
                        &mut stdout,
                        &WorkerResponse::error(req_id, "Cannot load model while generation is in progress"),
                    );
                    continue;
                }

                eprintln!("[WORKER] Loading model: {model_path} (gpu_layers: {gpu_layers:?})");
                let state = llama_state.clone();

                // Load model synchronously (blocking is fine, no generation running)
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create tokio runtime");

                let result = rt.block_on(load_model(state.clone(), &model_path, gpu_layers));

                match result {
                    Ok(()) => {
                        // Read back metadata from state
                        let guard = state.lock().unwrap();
                        let s = guard.as_ref().unwrap();
                        let payload = WorkerPayload::ModelLoaded {
                            model_path: s.current_model_path.clone().unwrap_or_default(),
                            context_length: s.model_context_length,
                            chat_template_type: s.chat_template_type.clone(),
                            chat_template_string: s.chat_template_string.clone(),
                            gpu_layers: s.gpu_layers,
                            general_name: s.general_name.clone(),
                            default_system_prompt: s.model_default_system_prompt.clone(),
                        };
                        drop(guard);
                        eprintln!("[WORKER] Model loaded successfully");
                        write_response(&mut stdout, &WorkerResponse::ok(req_id, payload));
                    }
                    Err(e) => {
                        eprintln!("[WORKER] Model load failed: {e}");
                        write_response(&mut stdout, &WorkerResponse::error(req_id, e));
                    }
                }
            }

            WorkerCommand::UnloadModel => {
                if generation_thread.is_some() {
                    cancel_flag.store(true, Ordering::SeqCst);
                    if let Some(handle) = generation_thread.take() {
                        let _ = handle.join();
                    }
                }

                eprintln!("[WORKER] Unloading model");
                let mut guard = llama_state.lock().unwrap();
                if let Some(ref mut state) = *guard {
                    state.inference_cache = None;
                    state.model = None;
                    state.current_model_path = None;
                    state.cached_system_prompt = None;
                    state.cached_prompt_key = None;
                }
                drop(guard);

                eprintln!("[WORKER] Model unloaded");
                write_response(
                    &mut stdout,
                    &WorkerResponse::ok(req_id, WorkerPayload::ModelUnloaded),
                );
            }

            WorkerCommand::GetModelStatus => {
                let status = get_model_status(&llama_state);
                let payload = WorkerPayload::ModelStatus {
                    loaded: status.loaded,
                    model_path: status.model_path,
                    general_name: llama_state
                        .lock()
                        .ok()
                        .and_then(|g| g.as_ref().and_then(|s| s.general_name.clone())),
                    context_length: llama_state
                        .lock()
                        .ok()
                        .and_then(|g| g.as_ref().and_then(|s| s.model_context_length)),
                    gpu_layers: llama_state
                        .lock()
                        .ok()
                        .and_then(|g| g.as_ref().and_then(|s| s.gpu_layers)),
                };
                write_response(&mut stdout, &WorkerResponse::ok(req_id, payload));
            }

            WorkerCommand::CancelGeneration => {
                cancel_flag.store(true, Ordering::SeqCst);
                eprintln!("[WORKER] Cancellation flag set");
                // No response needed for cancel (fire-and-forget)
            }

            WorkerCommand::Generate {
                user_message,
                conversation_id,
                skip_user_logging,
            } => {
                if generation_thread.is_some() {
                    write_response(
                        &mut stdout,
                        &WorkerResponse::error(req_id, "Generation already in progress"),
                    );
                    continue;
                }

                // Reset cancel flag
                cancel_flag.store(false, Ordering::SeqCst);

                let state = llama_state.clone();
                let db = db.clone();
                let cancel = cancel_flag.clone();
                let tx = token_tx.clone();

                eprintln!(
                    "[WORKER] Starting generation: conv={}, msg_len={}",
                    conversation_id.as_deref().unwrap_or("new"),
                    user_message.len()
                );

                generation_thread = Some(thread::spawn(move || {
                    let tx_panic = tx.clone();
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run_generation(GenerationParams {
                            req_id,
                            user_message,
                            conversation_id,
                            skip_user_logging,
                            llama_state: state,
                            db,
                            cancel,
                            tx,
                        });
                    }));
                    if let Err(panic_info) = result {
                        let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                            s.to_string()
                        } else if let Some(s) = panic_info.downcast_ref::<String>() {
                            s.clone()
                        } else {
                            "Unknown panic in generation thread".to_string()
                        };
                        eprintln!("[WORKER] Generation thread panicked: {msg}");
                        let _ = tx_panic.send(WorkerResponse::error(req_id, format!("Generation panicked: {msg}")));
                    }
                }));
            }
        }
    }

    eprintln!("[WORKER] Exiting");
    std::process::exit(0);
}

/// Parameters for a generation request.
struct GenerationParams {
    req_id: u64,
    user_message: String,
    conversation_id: Option<String>,
    skip_user_logging: bool,
    llama_state: SharedLlamaState,
    db: SharedDatabase,
    cancel: Arc<AtomicBool>,
    tx: Sender<WorkerResponse>,
}

/// Run a generation request on a background thread.
/// Sends Token and GenerationComplete/Error responses through the channel.
fn run_generation(params: GenerationParams) {
    use crate::web::config::get_resolved_system_prompt;
    use crate::web::database::conversation::ConversationLogger;
    use tokio::sync::mpsc;

    // Create a tokio runtime for the generation (it uses async internally)
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime for generation");

    let GenerationParams {
        req_id,
        user_message,
        conversation_id,
        skip_user_logging,
        llama_state,
        db,
        cancel,
        tx,
    } = params;

    rt.block_on(async {
        // Create or load conversation logger
        let shared_logger = if let Some(ref conv_id) = conversation_id {
            match ConversationLogger::from_existing(db.clone(), conv_id) {
                Ok(logger) => Arc::new(Mutex::new(logger)),
                Err(e) => {
                    let _ = tx.send(WorkerResponse::error(
                        req_id,
                        format!("Failed to load conversation: {e}"),
                    ));
                    return;
                }
            }
        } else {
            let system_prompt =
                get_resolved_system_prompt(&db, &Some(llama_state.clone()));
            match ConversationLogger::new(db.clone(), system_prompt.as_deref()) {
                Ok(logger) => Arc::new(Mutex::new(logger)),
                Err(e) => {
                    let _ = tx.send(WorkerResponse::error(
                        req_id,
                        format!("Failed to create conversation: {e}"),
                    ));
                    return;
                }
            }
        };

        // Log user message immediately (unless caller already did)
        if !skip_user_logging {
            let mut logger = shared_logger.lock().unwrap();
            logger.log_message("USER", &user_message);
        }

        // Create token streaming channel
        let (token_sender, mut token_receiver) = mpsc::unbounded_channel::<TokenData>();

        // Forward tokens from tokio mpsc → crossbeam on a REAL OS thread.
        // The generation loop is synchronous (no yield points), so a tokio::spawn
        // task on this single-threaded runtime would be starved until generation ends.
        // A real thread polls try_recv() independently of the tokio runtime.
        let tx_clone = tx.clone();
        let forward_thread = thread::spawn(move || {
            loop {
                match token_receiver.try_recv() {
                    Ok(token_data) => {
                        let response = WorkerResponse::ok(
                            req_id,
                            WorkerPayload::Token {
                                token: token_data.token,
                                tokens_used: token_data.tokens_used,
                                max_tokens: token_data.max_tokens,
                            },
                        );
                        if tx_clone.send(response).is_err() {
                            break; // Main loop exited
                        }
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                        thread::sleep(std::time::Duration::from_millis(1));
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
                }
            }
        });

        // Run generation
        let result = generate_llama_response(
            &user_message,
            llama_state,
            shared_logger.clone(),
            Some(token_sender),
            true, // skip_user_logging — we already logged above
            &db,
            cancel,
        )
        .await;

        // Drop the sender so the forward thread sees Disconnected
        let _ = result.as_ref().ok(); // ensure token_sender is dropped (moved into generate)

        // Wait for forwarding thread to finish
        let _ = forward_thread.join();

        // Get the conversation ID from the logger
        let final_conv_id = shared_logger
            .lock()
            .map(|l| l.get_conversation_id())
            .unwrap_or_default();

        match result {
            Ok(output) => {
                let _ = tx.send(WorkerResponse::ok(
                    req_id,
                    WorkerPayload::GenerationComplete {
                        conversation_id: final_conv_id,
                        tokens_used: output.tokens_used,
                        max_tokens: output.max_tokens,
                        prompt_tok_per_sec: output.prompt_tok_per_sec,
                        gen_tok_per_sec: output.gen_tok_per_sec,
                    },
                ));
            }
            Err(e) if e == "Cancelled" => {
                let _ = tx.send(WorkerResponse::ok(
                    req_id,
                    WorkerPayload::GenerationCancelled,
                ));
            }
            Err(e) => {
                eprintln!("[WORKER] Generation error: {e}");
                let _ = tx.send(WorkerResponse::error(req_id, e));
            }
        }
    });
}

/// Write a JSON response line to stdout, flushing immediately.
fn write_response(stdout: &mut io::StdoutLock, response: &WorkerResponse) {
    if let Ok(json) = serde_json::to_string(response) {
        let _ = writeln!(stdout, "{json}");
        let _ = stdout.flush();
    }
}
