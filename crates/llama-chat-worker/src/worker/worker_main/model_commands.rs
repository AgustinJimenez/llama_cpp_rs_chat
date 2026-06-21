//! LoadModel, UnloadModel, GetModelStatus command handlers.

use std::io::Write;
use std::sync::Arc;

use llama_chat_db::SharedDatabase;
use llama_chat_engine::model_manager::{get_model_status, load_model, ModelParams};
use llama_chat_types::models::SharedLlamaState;

use super::super::ipc_types::*;
use super::stdout::write_response;

/// Handle LoadModel command. Polls progress and writes LoadingProgress messages inline.
pub fn handle_load_model(
    req_id: u64,
    model_path: String,
    gpu_layers: Option<u32>,
    mmproj_path: Option<String>,
    agent_id: Option<String>,
    llama_state: SharedLlamaState,
    db: &SharedDatabase,
    ipc_writer: &mut impl Write,
) {
    eprintln!("[WORKER] Loading model: {model_path} (gpu_layers: {gpu_layers:?}, mmproj: {mmproj_path:?}, agent: {agent_id:?})");

    let db_config = if let Some(ref id) = agent_id {
        db.load_config_for_agent(id)
    } else {
        db.load_config()
    };
    let model_params = ModelParams {
        use_mlock: db_config.use_mlock,
        use_mmap: db_config.use_mmap,
        main_gpu: db_config.main_gpu,
        split_mode: db_config.split_mode.clone(),
    };

    // Progress tracking: AtomicU8 written by llama.cpp callback, polled inline below.
    let progress = Arc::new(std::sync::atomic::AtomicU8::new(0));
    let progress_for_load = progress.clone();

    // Run model loading in a background thread so we can poll progress from here
    let state_for_load = llama_state.clone();
    let load_handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime");
        rt.block_on(load_model(
            state_for_load,
            &model_path,
            gpu_layers,
            Some(&model_params),
            mmproj_path.as_deref(),
            Some(progress_for_load),
        ))
    });

    // Poll progress from the main thread (which owns ipc_writer) and write directly
    let mut last_sent: u8 = 0;
    while !load_handle.is_finished() {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let current = progress.load(std::sync::atomic::Ordering::Relaxed);
        if current != last_sent {
            write_response(ipc_writer, &WorkerResponse::ok(
                req_id,
                WorkerPayload::LoadingProgress { progress: current },
            ));
            last_sent = current;
        }
    }

    let result = load_handle.join().expect("Model load thread panicked");

    match result {
        Ok(()) => {
            let guard = llama_state.lock().unwrap();
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
            write_response(ipc_writer, &WorkerResponse::ok(0, WorkerPayload::LoadingProgress { progress: 101 }));

            // Pre-evaluate system prompt into KV cache for faster first response.
            // Run in background thread with 30s timeout to prevent hanging the
            // main IPC loop if context.decode() stalls (CUDA deadlock, debug build, etc.)
            let warmup_state = llama_state.clone();
            let warmup_db = db.clone();
            let warmup_done = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let warmup_done_clone = warmup_done.clone();
            std::thread::spawn(move || {
                match llama_chat_engine::warmup_system_prompt(warmup_state, warmup_db.as_ref(), agent_id.as_deref()) {
                    Ok(()) => eprintln!("[WORKER] System prompt warmup complete"),
                    Err(e) => eprintln!("[WORKER] System prompt warmup failed (non-fatal): {e}"),
                }
                warmup_done_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            });
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while !warmup_done.load(std::sync::atomic::Ordering::SeqCst) && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            if !warmup_done.load(std::sync::atomic::Ordering::SeqCst) {
                eprintln!("[WORKER] System prompt warmup timed out after 30s, continuing without warmup cache");
            }

            write_response(ipc_writer, &WorkerResponse::ok(req_id, payload));
        }
        Err(e) => {
            eprintln!("[WORKER] Model load failed: {e}");
            write_response(ipc_writer, &WorkerResponse::error(req_id, e));
        }
    }
}

/// Handle UnloadModel command.
pub fn handle_unload_model(
    req_id: u64,
    llama_state: &SharedLlamaState,
    ipc_writer: &mut impl Write,
) {
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
    write_response(ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::ModelUnloaded));
}

/// Handle GetModelStatus command.
pub fn handle_get_model_status(
    req_id: u64,
    llama_state: &SharedLlamaState,
    ipc_writer: &mut impl Write,
) {
    let status = get_model_status(llama_state);
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
    write_response(ipc_writer, &WorkerResponse::ok(req_id, payload));
}
