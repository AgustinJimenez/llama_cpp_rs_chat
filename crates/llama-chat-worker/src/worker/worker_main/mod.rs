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

use crossbeam_channel::{self, Receiver, Sender};

use super::ipc_types::*;
use llama_chat_db::{Database, SharedDatabase};
use crate::mcp::McpManager;
use llama_chat_types::models::SharedLlamaState;

mod crash_handler;
mod generation;
mod model_commands;
mod other_commands;
mod stdout;

use crash_handler::install_crash_handler;
use generation::{GenerationParams, run_generation};
use stdout::{steal_stdout_for_ipc, write_response, write_response_no_flush};

/// Run the worker process. This function never returns normally.
pub fn run_worker(db_path: &str) {
    // Disable upstream attention rotation — causes CUDA sync deadlocks
    // with Qwen3.5/3.6 models (llama.cpp issue #21383)
    std::env::set_var("LLAMA_ATTN_ROT_DISABLE", "1");

    // Install crash handler to log info before process dies from segfault
    install_crash_handler();

    // CRITICAL: Steal the real stdout fd for IPC before anything else.
    // After this, C printf/fprintf(stdout) goes to stderr.
    let ipc_out = steal_stdout_for_ipc();

    eprintln!("[WORKER] Starting model worker process (pid={})", std::process::id());

    // Open database
    let db: SharedDatabase = Arc::new(
        Database::new(db_path).expect("Worker: failed to open database"),
    );
    eprintln!("[WORKER] Database opened: {db_path}");

    // Create MCP manager for external tool servers (lazy — connects on first use)
    let mcp_manager = Arc::new(McpManager::new());
    eprintln!("[WORKER] MCP manager created (lazy — servers connect on first use)");

    // Initialize background process tracking with DB persistence
    let bg_session_id = uuid::Uuid::new_v4().to_string();
    llama_chat_command::background::init_background_tracking(db.clone(), bg_session_id);

    // Initialize event log with DB persistence
    llama_chat_db::event_log::init_event_log(db.clone());

    // Detect and kill orphaned processes from previous sessions
    let orphans = llama_chat_command::background::get_orphaned_processes(&db);
    if !orphans.is_empty() {
        eprintln!("[WORKER] ⚠️ Found {} orphaned background process(es) from previous session — killing:", orphans.len());
        for (pid, cmd, started_at) in &orphans {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let age_secs = (now - started_at).max(0);
            let age_str = if age_secs >= 3600 {
                format!("{}h{}m ago", age_secs / 3600, (age_secs % 3600) / 60)
            } else if age_secs >= 60 {
                format!("{}m{}s ago", age_secs / 60, age_secs % 60)
            } else {
                format!("{age_secs}s ago")
            };
            eprintln!("  Killing PID {pid}: {cmd} (started {age_str})");
            llama_chat_command::background::kill_background_process_by_pid(*pid);
        }
    }
    // Clean up records for processes that no longer exist
    llama_chat_command::background::cleanup_dead_process_records(&db);

    // LlamaState — owned directly, wrapped in Arc<Mutex> for generate_llama_response compatibility
    let llama_state: SharedLlamaState = Arc::new(Mutex::new(None));

    // Channels
    let (stdin_tx, stdin_rx): (Sender<String>, Receiver<String>) = crossbeam_channel::unbounded();
    let (token_tx, token_rx): (Sender<WorkerResponse>, Receiver<WorkerResponse>) =
        crossbeam_channel::unbounded();

    // Cancellation flag for generation
    let cancel_flag = Arc::new(AtomicBool::new(false));

    // Shutdown guard: kills tracked background processes when the worker exits
    struct BgProcessGuard;
    impl Drop for BgProcessGuard {
        fn drop(&mut self) {
            llama_chat_command::background::kill_all_session_processes();
        }
    }
    let _bg_guard = BgProcessGuard;

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
    // Clone the IPC file handle for use by blocking-operation status threads.
    let ipc_for_status: Arc<Mutex<std::fs::File>> = Arc::new(Mutex::new(
        ipc_out.try_clone().expect("Failed to clone IPC file handle"),
    ));
    // 64KB buffer reduces OS-level pipe writes during high-throughput token streaming.
    let mut ipc_writer = io::BufWriter::with_capacity(64 * 1024, ipc_out);

    eprintln!("[WORKER] Ready, waiting for commands...");

    loop {
        // Check if generation thread finished
        if let Some(ref handle) = generation_thread {
            if handle.is_finished() {
                generation_thread = None;
            }
        }

        // Wait for either a token or a command.
        // Tokens are batched with time-based flushing to reduce pipe I/O pressure
        // during CUDA compute.
        const FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(33); // ~30fps
        let line = loop {
            crossbeam_channel::select! {
                recv(token_rx) -> msg => {
                    if let Ok(first) = msg {
                        // Write first token, then collect more within the flush window
                        write_response_no_flush(&mut ipc_writer, &first);
                        let deadline = std::time::Instant::now() + FLUSH_INTERVAL;
                        loop {
                            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                            if remaining.is_zero() {
                                break;
                            }
                            match token_rx.recv_timeout(remaining) {
                                Ok(response) => {
                                    write_response_no_flush(&mut ipc_writer, &response);
                                }
                                Err(crossbeam_channel::RecvTimeoutError::Timeout) => break,
                                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                            }
                        }
                        let _ = ipc_writer.flush();
                    } // Channel disconnected, generation ended
                },
                recv(stdin_rx) -> msg => {
                    match msg {
                        Ok(l) => {
                            // Drain any pending tokens before processing command
                            let mut has_tokens = false;
                            while let Ok(response) = token_rx.try_recv() {
                                write_response_no_flush(&mut ipc_writer, &response);
                                has_tokens = true;
                            }
                            if has_tokens {
                                let _ = ipc_writer.flush();
                            }
                            break l;
                        }
                        Err(_) => {
                            eprintln!("[WORKER] Stdin channel disconnected, shutting down");
                            write_response(&mut ipc_writer, &WorkerResponse::error(0, "stdin closed".to_string()));
                            crate::prevent_sleep::force_release();
                            std::process::exit(0);
                        }
                    }
                },
            }
            // If only tokens were received, loop back to select
            if let Some(ref handle) = generation_thread {
                if handle.is_finished() {
                    generation_thread = None;
                }
            }
        };

        // Parse command
        let request: WorkerRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[WORKER] Failed to parse command: {e}");
                write_response(
                    &mut ipc_writer,
                    &WorkerResponse::error(0, format!("Parse error: {e}")),
                );
                continue;
            }
        };

        let req_id = request.id;

        match request.command {
            WorkerCommand::RefreshMcpServers => {
                other_commands::handle_refresh_mcp_servers(req_id, &mcp_manager, &db, &mut ipc_writer);
            }

            WorkerCommand::GetMcpStatus => {
                other_commands::handle_get_mcp_status(req_id, &mcp_manager, &mut ipc_writer);
            }

            WorkerCommand::CallMcpTool { name, args_json } => {
                other_commands::handle_call_mcp_tool(req_id, &name, &args_json, &mcp_manager, &mut ipc_writer);
            }

            WorkerCommand::GetMcpToolDefinitions => {
                other_commands::handle_get_mcp_tool_definitions(req_id, &mcp_manager, &mut ipc_writer);
            }

            WorkerCommand::GetConversationEvents { conversation_id } => {
                other_commands::handle_get_conversation_events(req_id, &conversation_id, &mut ipc_writer);
            }

            WorkerCommand::GetGlobalStatus => {
                other_commands::handle_get_global_status(req_id, &mut ipc_writer);
            }

            WorkerCommand::GetAvailableBackends => {
                other_commands::handle_get_available_backends(req_id, &mut ipc_writer);
            }

            WorkerCommand::CompactConversation { conversation_id } => {
                // Reject if generation is in progress
                if generation_thread.as_ref().map(|h| !h.is_finished()).unwrap_or(false) {
                    write_response(&mut ipc_writer, &WorkerResponse::error(req_id, "Cannot compact while generation is in progress"));
                    continue;
                }
                other_commands::handle_compact_conversation(
                    req_id,
                    conversation_id,
                    llama_state.clone(),
                    &db,
                    ipc_for_status.clone(),
                    &mut ipc_writer,
                );
            }

            WorkerCommand::Ping => {
                write_response(&mut ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::Pong));
            }

            WorkerCommand::Shutdown => {
                eprintln!("[WORKER] Shutdown requested");
                cancel_flag.store(true, Ordering::SeqCst);
                if let Some(handle) = generation_thread.take() {
                    let _ = handle.join();
                }
                write_response(&mut ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::Pong));
                break;
            }

            WorkerCommand::LoadModel { model_path, gpu_layers, mmproj_path, agent_id } => {
                if generation_thread.is_some() {
                    write_response(
                        &mut ipc_writer,
                        &WorkerResponse::error(req_id, "Cannot load model while generation is in progress"),
                    );
                    continue;
                }
                model_commands::handle_load_model(
                    req_id,
                    model_path,
                    gpu_layers,
                    mmproj_path,
                    agent_id,
                    llama_state.clone(),
                    &db,
                    &mut ipc_writer,
                );
            }

            WorkerCommand::UnloadModel => {
                if generation_thread.is_some() {
                    cancel_flag.store(true, Ordering::SeqCst);
                    if let Some(handle) = generation_thread.take() {
                        let _ = handle.join();
                    }
                }
                model_commands::handle_unload_model(req_id, &llama_state, &mut ipc_writer);
            }

            WorkerCommand::GetModelStatus => {
                model_commands::handle_get_model_status(req_id, &llama_state, &mut ipc_writer);
            }

            WorkerCommand::CancelGeneration => {
                cancel_flag.store(true, Ordering::SeqCst);
                eprintln!("[WORKER] Cancellation flag set");
                // No response needed for cancel (fire-and-forget)
            }

            WorkerCommand::GenerateTitle {
                conversation_id,
                prompt,
            } => {
                // Clean up finished generation thread before checking availability
                if let Some(handle) = generation_thread.take() {
                    if handle.is_finished() {
                        let _ = handle.join();
                    } else {
                        // Still actually running — put it back and reject
                        generation_thread = Some(handle);
                        write_response(
                            &mut ipc_writer,
                            &WorkerResponse::error(req_id, "Cannot generate title while generation is in progress"),
                        );
                        continue;
                    }
                }
                other_commands::handle_generate_title(
                    req_id,
                    conversation_id,
                    prompt,
                    llama_state.clone(),
                    &mut ipc_writer,
                );
            }

            WorkerCommand::Generate {
                user_message,
                conversation_id,
                skip_user_logging,
                image_data,
                agent_id,
            } => {
                // Clean up finished generation thread before checking availability.
                if let Some(handle) = generation_thread.take() {
                    if handle.is_finished() {
                        let _ = handle.join();
                    } else if cancel_flag.load(Ordering::SeqCst) {
                        // Cancel was requested — wait up to 3s for the thread to finish
                        eprintln!("[WORKER] Waiting for cancelled generation to finish...");
                        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
                        while !handle.is_finished() && std::time::Instant::now() < deadline {
                            std::thread::sleep(std::time::Duration::from_millis(50));
                        }
                        if handle.is_finished() {
                            let _ = handle.join();
                            eprintln!("[WORKER] Cancelled generation cleaned up");
                        } else {
                            // Still stuck after 3s — reject
                            generation_thread = Some(handle);
                            write_response(
                                &mut ipc_writer,
                                &WorkerResponse::error(req_id, "Generation still cancelling, please wait"),
                            );
                            continue;
                        }
                    } else {
                        // Still actually running, not cancelled — reject
                        generation_thread = Some(handle);
                        write_response(
                            &mut ipc_writer,
                            &WorkerResponse::error(req_id, "Generation already in progress"),
                        );
                        continue;
                    }
                }

                // Reset cancel flag
                cancel_flag.store(false, Ordering::SeqCst);

                let state = llama_state.clone();
                let db = db.clone();
                let cancel = cancel_flag.clone();
                let tx = token_tx.clone();
                let mcp = mcp_manager.clone();

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
                            image_data,
                            agent_id,
                            llama_state: state,
                            db,
                            cancel,
                            tx,
                            mcp_manager: mcp,
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
    crate::prevent_sleep::force_release();
    std::process::exit(0);
}
