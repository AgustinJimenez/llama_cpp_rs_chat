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
use llama_chat_engine::{generate_llama_response, generate_title_text, warmup_system_prompt};
use llama_chat_db::{Database, SharedDatabase};
use crate::mcp::McpManager;
use llama_chat_engine::model_manager::{get_model_status, load_model, ModelParams};
use llama_chat_types::models::{SharedLlamaState, TokenData};

/// Redirect the C-level stdout file descriptor to stderr so that any native
/// C/C++ code (e.g. llama.cpp's clip_model_loader) that calls `printf` or
/// `fprintf(stdout, ...)` writes to stderr instead of polluting our JSON
/// Lines IPC pipe on stdout.
///
/// Returns a `File` wrapping the original stdout fd for exclusive IPC use.
#[cfg(windows)]
fn steal_stdout_for_ipc() -> std::fs::File {
    use std::os::windows::io::FromRawHandle;
    extern "C" {
        fn _dup(fd: i32) -> i32;
        fn _dup2(src: i32, dst: i32) -> i32;
    }
    unsafe {
        // 1 = stdout, 2 = stderr
        let ipc_fd = _dup(1); // duplicate real stdout → new fd for IPC
        assert!(ipc_fd >= 0, "Failed to _dup stdout");
        _dup2(2, 1); // redirect C stdout → stderr

        // Convert the raw fd into a File we can write to
        let handle = libc_fd_to_handle(ipc_fd);
        std::fs::File::from_raw_handle(handle as *mut _)
    }
}

#[cfg(windows)]
unsafe fn libc_fd_to_handle(fd: i32) -> usize {
    extern "C" {
        fn _get_osfhandle(fd: i32) -> isize;
    }
    unsafe { _get_osfhandle(fd) as usize }
}

#[cfg(not(windows))]
fn steal_stdout_for_ipc() -> std::fs::File {
    use std::os::unix::io::FromRawFd;
    unsafe {
        let ipc_fd = libc::dup(1);
        assert!(ipc_fd >= 0, "Failed to dup stdout");
        libc::dup2(2, 1);
        std::fs::File::from_raw_fd(ipc_fd)
    }
}

/// Run the worker process. This function never returns normally.
pub fn run_worker(db_path: &str) {
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

    // Detect orphaned processes from previous sessions
    let orphans = llama_chat_command::background::get_orphaned_processes(&db);
    if !orphans.is_empty() {
        eprintln!("[WORKER] ⚠️ Found {} orphaned background process(es) from previous session:", orphans.len());
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
                format!("{}s ago", age_secs)
            };
            eprintln!("  PID {}: {} (started {})", pid, cmd, age_str);
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
    let mut ipc_writer = io::BufWriter::new(ipc_out);

    eprintln!("[WORKER] Ready, waiting for commands...");

    loop {
        // Check if generation thread finished
        if let Some(ref handle) = generation_thread {
            if handle.is_finished() {
                generation_thread = None;
            }
        }

        // Wait for either a token or a command — no polling, no timeout.
        // crossbeam select! wakes instantly when either channel has data.
        let line = loop {
            crossbeam_channel::select! {
                recv(token_rx) -> msg => {
                    match msg {
                        Ok(first) => {
                            // Batch: drain all available tokens, write, single flush
                            write_response_no_flush(&mut ipc_writer, &first);
                            while let Ok(response) = token_rx.try_recv() {
                                write_response_no_flush(&mut ipc_writer, &response);
                            }
                            let _ = ipc_writer.flush();
                        }
                        Err(_) => {} // Channel disconnected, generation ended
                    }
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
                            // Use a sentinel to break the outer loop
                            write_response(&mut ipc_writer, &WorkerResponse::error(0, "stdin closed".to_string()));
                            crate::prevent_sleep::force_release();
                            std::process::exit(0);
                        }
                    }
                },
            }
            // If only tokens were received, loop back to select
            // Check generation thread status while we're here
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
                eprintln!("[WORKER] Refreshing MCP server connections...");
                match mcp_manager.refresh_connections(&db) {
                    Ok(()) => {
                        let tool_defs = mcp_manager.get_tool_definitions();
                        let connected: Vec<String> = mcp_manager.get_connected_server_names();
                        eprintln!("[WORKER] MCP refresh complete: {} servers, {} tools", connected.len(), tool_defs.len());
                        write_response(&mut ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::McpServersRefreshed {
                            connected_servers: connected,
                            total_tools: tool_defs.len(),
                        }));
                    }
                    Err(e) => {
                        eprintln!("[WORKER] MCP refresh failed: {e}");
                        write_response(&mut ipc_writer, &WorkerResponse::error(req_id, e));
                    }
                }
            }

            WorkerCommand::GetMcpStatus => {
                let statuses = mcp_manager.get_server_statuses();
                write_response(&mut ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::McpStatus {
                    servers: statuses,
                }));
            }

            WorkerCommand::GetConversationEvents { conversation_id } => {
                let events = llama_chat_db::event_log::get_events(&conversation_id);
                write_response(&mut ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::ConversationEvents { events }));
            }

            WorkerCommand::GetGlobalStatus => {
                let status = llama_chat_db::event_log::get_global_status();
                write_response(&mut ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::GlobalStatus { status }));
            }

            WorkerCommand::GetAvailableBackends => {
                // Ensure backends are loaded (needed for dynamic-backends mode)
                #[cfg(feature = "dynamic-backends")]
                {
                    let _backend = llama_cpp_2::llama_backend::LlamaBackend::init();
                    if let Ok(ref b) = _backend { b.load_all_backends(); }
                }
                let devices = llama_cpp_2::list_llama_ggml_backend_devices();
                let mut backend_map: std::collections::HashMap<String, Vec<super::ipc_types::BackendDeviceInfo>> = std::collections::HashMap::new();
                for dev in &devices {
                    let vram_mb = if dev.memory_total > 0 {
                        Some((dev.memory_total / (1024 * 1024)) as u64)
                    } else {
                        None
                    };
                    backend_map.entry(dev.backend.clone()).or_default().push(
                        super::ipc_types::BackendDeviceInfo {
                            name: dev.name.clone(),
                            description: dev.description.clone(),
                            vram_mb,
                        },
                    );
                }
                let backends: Vec<super::ipc_types::BackendInfo> = backend_map
                    .into_iter()
                    .map(|(name, devices)| super::ipc_types::BackendInfo {
                        available: true,
                        name,
                        devices,
                    })
                    .collect();
                write_response(&mut ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::AvailableBackends { backends }));
            }

            WorkerCommand::Ping => {
                write_response(&mut ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::Pong));
            }

            WorkerCommand::Shutdown => {
                eprintln!("[WORKER] Shutdown requested");
                // Cancel any in-progress generation
                cancel_flag.store(true, Ordering::SeqCst);
                if let Some(handle) = generation_thread.take() {
                    let _ = handle.join();
                }
                write_response(&mut ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::Pong));
                break;
            }

            WorkerCommand::LoadModel { model_path, gpu_layers, mmproj_path } => {
                if generation_thread.is_some() {
                    write_response(
                        &mut ipc_writer,
                        &WorkerResponse::error(req_id, "Cannot load model while generation is in progress"),
                    );
                    continue;
                }

                eprintln!("[WORKER] Loading model: {model_path} (gpu_layers: {gpu_layers:?}, mmproj: {mmproj_path:?})");
                let state = llama_state.clone();

                // Read model-level params from config DB
                let db_config = db.load_config();
                let model_params = ModelParams {
                    use_mlock: db_config.use_mlock,
                    use_mmap: db_config.use_mmap,
                    main_gpu: db_config.main_gpu,
                    split_mode: db_config.split_mode.clone(),
                };

                // Progress tracking: AtomicU8 written by llama.cpp callback, polled inline below.
                // We can't use token_tx here because the main loop (which reads token_rx) is
                // blocked running this handler — messages would queue but never reach stdout.
                let progress = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
                let progress_for_load = progress.clone();

                // Run model loading in a background thread so we can poll progress from here
                let state_for_load = state.clone();
                let load_handle = std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("Failed to create tokio runtime");
                    rt.block_on(load_model(state_for_load, &model_path, gpu_layers, Some(&model_params), mmproj_path.as_deref(), Some(progress_for_load)))
                });

                // Poll progress from the main thread (which owns ipc_writer) and write directly
                let mut last_sent: u8 = 0;
                while !load_handle.is_finished() {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    let current = progress.load(std::sync::atomic::Ordering::Relaxed);
                    if current != last_sent {
                        write_response(&mut ipc_writer, &WorkerResponse::ok(
                            req_id,
                            WorkerPayload::LoadingProgress { progress: current },
                        ));
                        last_sent = current;
                    }
                }

                let result = load_handle.join().expect("Model load thread panicked");

                match result {
                    Ok(()) => {
                        // Read back metadata from state
                        let guard = state.lock().unwrap();
                        let s = guard.as_ref().unwrap();
                        let block_count = s.current_model_path.as_deref()
                            .and_then(llama_chat_engine::vram_calculator::read_gguf_block_count);
                        let payload = WorkerPayload::ModelLoaded {
                            model_path: s.current_model_path.clone().unwrap_or_default(),
                            context_length: s.model_context_length,
                            chat_template_type: s.chat_template_type.clone(),
                            chat_template_string: s.chat_template_string.clone(),
                            gpu_layers: s.gpu_layers,
                            block_count,
                            general_name: s.general_name.clone(),
                            #[cfg(feature = "vision")]
                            has_vision: Some(s.vision_state.is_some()),
                            #[cfg(not(feature = "vision"))]
                            has_vision: Some(false),
                        };
                        drop(guard);
                        eprintln!("[WORKER] Model loaded successfully");

                        // Signal frontend that model file is loaded, now warming up system prompt
                        write_response(&mut ipc_writer, &WorkerResponse::ok(0, WorkerPayload::LoadingProgress { progress: 101 }));

                        // Pre-evaluate system prompt into KV cache for faster first response
                        match warmup_system_prompt(state.clone(), &db) {
                            Ok(()) => eprintln!("[WORKER] System prompt warmup complete"),
                            Err(e) => eprintln!("[WORKER] System prompt warmup failed (non-fatal): {e}"),
                        }

                        write_response(&mut ipc_writer, &WorkerResponse::ok(req_id, payload));
                    }
                    Err(e) => {
                        eprintln!("[WORKER] Model load failed: {e}");
                        write_response(&mut ipc_writer, &WorkerResponse::error(req_id, e));
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
                    #[cfg(feature = "vision")]
                    { state.vision_state = None; }
                    state.model = None;
                    state.current_model_path = None;
                    state.cached_system_prompt = None;
                    state.cached_prompt_key = None;
                }
                drop(guard);

                eprintln!("[WORKER] Model unloaded");
                write_response(
                    &mut ipc_writer,
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
                write_response(&mut ipc_writer, &WorkerResponse::ok(req_id, payload));
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
                // (same pattern as Generate handler — fixes race where thread sent
                // GenerationComplete but hasn't been joined yet)
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

                eprintln!("[WORKER] Generating title for conv={}", conversation_id);
                let state = llama_state.clone();

                match generate_title_text(&state, &prompt) {
                    Ok(title) => {
                        write_response(
                            &mut ipc_writer,
                            &WorkerResponse::ok(
                                req_id,
                                WorkerPayload::TitleGenerated {
                                    conversation_id,
                                    title,
                                },
                            ),
                        );
                    }
                    Err(e) => {
                        eprintln!("[WORKER] Title generation failed: {e}");
                        write_response(&mut ipc_writer, &WorkerResponse::error(req_id, e));
                    }
                }
            }

            WorkerCommand::Generate {
                user_message,
                conversation_id,
                skip_user_logging,
                image_data,
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

/// Install a crash handler to capture segfault/access violation info.
/// On Windows, uses SetUnhandledExceptionFilter to log the crash address
/// and exception code before the process dies.
fn install_crash_handler() {
    #[cfg(windows)]
    {
        use std::sync::Once;
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            unsafe {
                extern "system" fn crash_handler(
                    info: *mut std::ffi::c_void,
                ) -> i32 {
                    #[repr(C)]
                    struct ExceptionPointers {
                        record: *const ExceptionRecord,
                        _context: *const std::ffi::c_void,
                    }
                    #[repr(C)]
                    struct ExceptionRecord {
                        code: u32,
                        _flags: u32,
                        _nested: *const std::ffi::c_void,
                        address: *const std::ffi::c_void,
                    }

                    unsafe {
                        let ptrs = info as *const ExceptionPointers;
                        if !ptrs.is_null() && !(*ptrs).record.is_null() {
                            let rec = &*(*ptrs).record;
                            eprintln!("[CRASH] Exception code: 0x{:08X}, address: {:?}",
                                rec.code, rec.address);
                            if rec.code == 0xC0000005 {
                                eprintln!("[CRASH] ACCESS VIOLATION (segfault) — likely CUDA memory corruption");
                            }
                        } else {
                            eprintln!("[CRASH] Unhandled exception (no details available)");
                        }
                    }
                    eprintln!("[CRASH] Worker crashing. Check logs/last_prompt_dump.txt, last_inject_dump.txt, last_gen_tokens.txt");
                    // For C++ exceptions (0xE06D7363), do a controlled exit
                    // instead of letting Windows terminate() run, which is slower.
                    unsafe {
                        let ptrs2 = info as *const ExceptionPointers;
                        if !ptrs2.is_null() && !(*ptrs2).record.is_null() {
                            let code = (*(*ptrs2).record).code;
                            if code == 0xE06D7363 {
                                eprintln!("[CRASH] C++ exception — doing controlled exit(42) for fast restart");
                                std::process::exit(42);
                            }
                        }
                    }
                    0 // EXCEPTION_CONTINUE_SEARCH for other exceptions
                }

                // SetUnhandledExceptionFilter
                extern "system" {
                    fn SetUnhandledExceptionFilter(
                        filter: extern "system" fn(*mut std::ffi::c_void) -> i32,
                    ) -> *mut std::ffi::c_void;
                }
                SetUnhandledExceptionFilter(crash_handler);
                eprintln!("[WORKER] Crash handler installed");
            }
        });
    }

    #[cfg(not(windows))]
    {
        // On Unix, could use signal handler for SIGSEGV
        eprintln!("[WORKER] Crash handler not implemented for this platform");
    }
}

/// Parameters for a generation request.
struct GenerationParams {
    req_id: u64,
    user_message: String,
    conversation_id: Option<String>,
    skip_user_logging: bool,
    image_data: Option<Vec<String>>,
    llama_state: SharedLlamaState,
    db: SharedDatabase,
    cancel: Arc<AtomicBool>,
    tx: Sender<WorkerResponse>,
    mcp_manager: Arc<McpManager>,
}

/// Run a generation request on a background thread.
/// Sends Token and GenerationComplete/Error responses through the channel.
fn run_generation(params: GenerationParams) {
    use llama_chat_engine::config_ext::get_resolved_system_prompt;
    use llama_chat_db::conversation::ConversationLogger;
    use tokio::sync::mpsc;

    // Prevent system/display sleep for the duration of generation
    crate::prevent_sleep::retain();
    struct SleepGuard;
    impl Drop for SleepGuard {
        fn drop(&mut self) {
            crate::prevent_sleep::release();
        }
    }
    let _sleep_guard = SleepGuard;

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
        image_data,
        llama_state,
        db,
        cancel,
        tx,
        mcp_manager,
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

        // Notify the bridge of the conversation ID so the sidebar can show the generating indicator
        {
            let conv_id = shared_logger.lock().unwrap().get_conversation_id();
            let _ = tx.send(WorkerResponse::ok(req_id, WorkerPayload::GenerationStarted {
                conversation_id: conv_id,
            }));
        }

        // Log user message immediately (unless caller already did)
        if !skip_user_logging {
            let mut logger = shared_logger.lock().unwrap();
            let estimated_tokens = (user_message.len() / 4).max(1) as i32;
            logger.log_message_with_tokens("USER", &user_message, Some(estimated_tokens));
        }

        // Create token streaming channel
        let (token_sender, mut token_receiver) = mpsc::unbounded_channel::<TokenData>();

        // Forward tokens from tokio mpsc → crossbeam on a REAL OS thread.
        // The generation loop is synchronous (no yield points), so a tokio::spawn
        // task on this single-threaded runtime would be starved until generation ends.
        // Uses blocking_recv() which wakes instantly when a token arrives (no polling).
        let tx_clone = tx.clone();
        let forward_thread = thread::spawn(move || {
            loop {
                match token_receiver.blocking_recv() {
                    Some(token_data) => {
                        let response = WorkerResponse::ok(
                            req_id,
                            WorkerPayload::Token {
                                token: token_data.token,
                                tokens_used: token_data.tokens_used,
                                max_tokens: token_data.max_tokens,
                                status: token_data.status,
                            },
                        );
                        if tx_clone.send(response).is_err() {
                            break; // Main loop exited
                        }
                    }
                    None => break, // Channel closed — generation ended
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
            db.clone(),
            cancel,
            image_data.as_deref(),
            Some(mcp_manager),
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
                        gen_eval_ms: output.gen_eval_ms,
                        gen_tokens: output.gen_tokens,
                        prompt_eval_ms: output.prompt_eval_ms,
                        prompt_tokens: output.prompt_tokens,
                        finish_reason: Some(output.finish_reason),
                        token_breakdown: output.token_breakdown,
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

/// Write a JSON response line to the IPC pipe, flushing immediately.
fn write_response(writer: &mut impl Write, response: &WorkerResponse) {
    if let Ok(json) = serde_json::to_string(response) {
        let _ = writeln!(writer, "{json}");
        let _ = writer.flush();
    }
}

/// Write a response without flushing — for batched writes.
fn write_response_no_flush(writer: &mut impl Write, response: &WorkerResponse) {
    if let Ok(json) = serde_json::to_string(response) {
        let _ = writeln!(writer, "{json}");
    }
}
