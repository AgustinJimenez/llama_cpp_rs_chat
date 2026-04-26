// Simple web server version of LLaMA Chat (without Tauri)
mod web; // Declare web module for model capabilities and utilities

// Import all types and functions from web modules
use web::database::{Database, SharedDatabase};

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

#[cfg(not(feature = "mock"))]
use web::worker::process_manager::ProcessManager;
#[cfg(not(feature = "mock"))]
use web::worker::worker_bridge::{SharedWorkerBridge, WorkerBridge};

// HTTP server using hyper
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};

// Note: All struct definitions (SamplerConfig, TokenData, ChatRequest, ChatResponse, etc.)
// and helper functions (load_config, add_to_model_history, get_model_status, etc.)
// are now imported from web modules (web::config, web::command, web::model_manager, etc.)

// All helper functions and struct definitions are imported from web modules

#[cfg(not(feature = "mock"))]
async fn handle_request(
    req: Request<Body>,
    worker_bridge: SharedWorkerBridge,
    db: SharedDatabase,
) -> std::result::Result<Response<Body>, Infallible> {
    handle_request_impl(req, Some(worker_bridge), db).await
}

#[cfg(feature = "mock")]
async fn handle_request(
    req: Request<Body>,
    db: SharedDatabase,
) -> std::result::Result<Response<Body>, Infallible> {
    handle_request_impl(req, None, db).await
}

async fn handle_request_impl(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] worker_bridge: Option<SharedWorkerBridge>,
    #[cfg(feature = "mock")] _worker_bridge: Option<()>,
    db: SharedDatabase,
) -> std::result::Result<Response<Body>, Infallible> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();

    #[cfg(not(feature = "mock"))]
    let bridge = worker_bridge.unwrap();

    #[cfg(feature = "mock")]
    let bridge = ();

    let response = match (&method, path.as_str()) {
        // Health check
        (&Method::GET, "/health") => web::routes::health::handle(bridge.clone()).await?,

        // App info & API docs
        (&Method::GET, "/api/info") => web::routes::system::handle_app_info().await?,
        (&Method::GET, "/api/docs") => web::routes::system::handle_api_docs().await?,


        // System monitoring
        (&Method::GET, "/api/system/usage") => web::routes::system::handle_system_usage().await?,
        (&Method::GET, "/api/system/processes") => web::routes::system::handle_background_processes(db.clone()).await?,
        (&Method::POST, "/api/system/processes/kill") => web::routes::system::handle_kill_process(req, db.clone()).await?,
        (&Method::POST, "/api/desktop/abort") => web::routes::system::handle_desktop_abort().await?,

        // Frontend log ingestion (web-only)
        (&Method::POST, "/api/logs/frontend") => {
            web::routes::frontend_logs::handle_post_frontend_logs(req).await?
        }

        // App-level frontend/runtime errors
        (&Method::POST, "/api/errors") => {
            web::routes::app_errors::handle_record_app_error(req, db.clone()).await?
        }
        (&Method::GET, "/api/errors") => {
            web::routes::app_errors::handle_get_app_errors(&req, db.clone()).await?
        }
        (&Method::DELETE, "/api/errors") => {
            web::routes::app_errors::handle_clear_app_errors(db.clone()).await?
        }

        // Chat endpoints
        (&Method::POST, "/api/chat") => {
            web::routes::chat::handle_post_chat(req, bridge.clone(), db.clone()).await?
        }

        (&Method::POST, "/api/chat/stream") => {
            web::routes::chat::handle_post_chat_stream(req, bridge.clone(), db.clone()).await?
        }

        (&Method::POST, "/api/chat/cancel") => {
            web::routes::chat::handle_post_chat_cancel(bridge.clone()).await?
        }

        (&Method::GET, "/ws/chat/stream") => {
            web::routes::chat::handle_websocket_chat_stream(req, bridge.clone(), db.clone()).await?
        }

        (&Method::GET, path) if path.starts_with("/ws/conversation/watch/") => {
            web::routes::chat::handle_conversation_watch_websocket(req, path, bridge.clone(), db.clone())
                .await?
        }

        (&Method::GET, "/ws/status") => {
            web::routes::status::handle_status_websocket(req, bridge.clone()).await?
        }


        // Configuration endpoints
        (&Method::GET, "/api/config") => {
            web::routes::config::handle_get_config(bridge.clone(), db.clone()).await?
        }

        (&Method::POST, "/api/config") => {
            web::routes::config::handle_post_config(req, bridge.clone(), db.clone()).await?
        }

        // Provider API keys
        (&Method::GET, "/api/config/provider-keys") => {
            web::routes::config::handle_get_provider_keys(db.clone()).await?
        }
        (&Method::POST, "/api/config/provider-keys") => {
            web::routes::config::handle_set_provider_key(req, db.clone()).await?
        }
        (&Method::GET, "/api/config/active-provider") => {
            web::routes::config::handle_get_active_provider(db.clone()).await?
        }
        (&Method::POST, "/api/config/active-provider") => {
            web::routes::config::handle_set_active_provider(req, db.clone()).await?
        }

        // Conversation config (must be before the catch-all /api/conversation/ route)
        (&Method::GET, path) if path.starts_with("/api/conversations/") && path.ends_with("/config") => {
            web::routes::config::handle_get_conversation_config(path, bridge.clone(), db.clone()).await?
        }

        (&Method::POST, path) if path.starts_with("/api/conversations/") && path.ends_with("/config") => {
            web::routes::config::handle_post_conversation_config(req, path, bridge.clone(), db.clone()).await?
        }

        // Conversation event log (in-memory debug events)
        (&Method::GET, path) if path.starts_with("/api/conversations/") && path.ends_with("/events") => {
            web::routes::conversation::handle_get_conversation_events(path, bridge.clone()).await?
        }

        // Conversation token analysis
        (&Method::GET, path) if path.starts_with("/api/conversations/") && path.ends_with("/token-analysis") => {
            let id = &path["/api/conversations/".len()..path.len()-"/token-analysis".len()];
            web::routes::conversation::handle_conversation_token_analysis(id, db.clone()).await?
        }

        // Conversation metrics
        (&Method::GET, path) if path.starts_with("/api/conversations/") && path.ends_with("/metrics") => {
            web::routes::conversation::handle_get_conversation_metrics(path, db.clone()).await?
        }

        // Conversation truncate (for message editing)
        (&Method::POST, path) if path.starts_with("/api/conversations/") && path.ends_with("/truncate") => {
            web::routes::conversation::handle_truncate_conversation(req, path, db.clone()).await?
        }

        // Conversation rename (PATCH must be before DELETE catch-all)
        (&Method::PATCH, path) if path.starts_with("/api/conversations/") && path.ends_with("/title") => {
            let id = &path["/api/conversations/".len()..path.len() - "/title".len()];
            web::routes::conversation::handle_rename_conversation(req, id, db.clone()).await?
        }

        // Conversation export (must be before generic /api/conversation/{id})
        (&Method::GET, path) if path.starts_with("/api/conversation/") && path.ends_with("/export") => {
            let id = &path["/api/conversation/".len()..path.len()-"/export".len()];
            web::routes::conversation::handle_export_conversation(&req, id, db.clone()).await?
        }

        // Conversation endpoints
        (&Method::GET, path) if path.starts_with("/api/conversation/") => {
            web::routes::conversation::handle_get_conversation(path, bridge.clone(), db.clone()).await?
        }

        (&Method::GET, "/api/conversations") => {
            web::routes::conversation::handle_get_conversations(&req, bridge.clone(), db.clone()).await?
        }

        (&Method::POST, "/api/conversations") => {
            web::routes::conversation::handle_create_conversation(req, db.clone()).await?
        }

        // Batch delete (must be before single delete /api/conversations/{id})
        (&Method::DELETE, "/api/conversations/batch") => {
            web::routes::conversation::handle_batch_delete_conversations(req, db.clone()).await?
        }

        (&Method::DELETE, path) if path.starts_with("/api/conversations/") => {
            web::routes::conversation::handle_delete_conversation(path, bridge.clone(), db.clone()).await?
        }

        // Provider endpoints
        (&Method::GET, "/api/providers") => {
            web::routes::providers::handle_list_providers(db.clone()).await?
        }
        (&Method::GET, path) if path.starts_with("/api/providers/") && path.ends_with("/models") => {
            let provider_id = path
                .trim_start_matches("/api/providers/")
                .trim_end_matches("/models")
                .trim_end_matches('/');
            web::routes::providers::handle_provider_models(provider_id, db.clone()).await?
        }
        (&Method::POST, path) if path.starts_with("/api/providers/") && path.ends_with("/stream") => {
            let provider_id = path
                .trim_start_matches("/api/providers/")
                .trim_end_matches("/stream")
                .trim_end_matches('/');
            web::routes::providers::handle_provider_stream(req, db.clone(), provider_id).await?
        }
        (&Method::POST, path) if path.starts_with("/api/providers/") && path.ends_with("/generate") => {
            let provider_id = path
                .trim_start_matches("/api/providers/")
                .trim_end_matches("/generate")
                .trim_end_matches('/');
            web::routes::providers::handle_provider_generate(req, db.clone(), provider_id).await?
        }

        // Model endpoints
        (&Method::GET, "/api/model/info") => {
            web::routes::model::handle_get_model_info(req, bridge.clone()).await?
        }

        (&Method::GET, "/api/model/status") => {
            web::routes::model::handle_get_model_status(bridge.clone(), db.clone()).await?
        }

        (&Method::GET, "/api/model/history") => {
            web::routes::model::handle_get_model_history(bridge.clone(), db.clone()).await?
        }

        (&Method::POST, "/api/model/history") => {
            web::routes::model::handle_post_model_history(req, bridge.clone(), db.clone()).await?
        }

        (&Method::POST, "/api/model/load") => {
            web::routes::model::handle_post_model_load(req, bridge.clone(), db.clone()).await?
        }

        (&Method::POST, "/api/model/unload") => {
            web::routes::model::handle_post_model_unload(bridge.clone()).await?
        }

        (&Method::POST, "/api/model/hard-unload") => {
            web::routes::model::handle_post_model_hard_unload(bridge.clone()).await?
        }

        (&Method::GET, "/api/backends") => {
            web::routes::model::handle_get_backends(bridge.clone()).await?
        }

        (&Method::POST, "/api/backends/install") => {
            web::routes::model::handle_post_backends_install().await?
        }

        // HuggingFace Hub search & download
        (&Method::GET, "/api/hub/search") => web::routes::hub::handle_search(req).await?,
        (&Method::GET, "/api/hub/tree") => web::routes::hub::handle_tree(req).await?,
        (&Method::POST, "/api/hub/download") => web::routes::download::handle_post_download(req, db.clone()).await?,
        (&Method::GET, "/api/hub/downloads") => web::routes::download::handle_get_downloads(db.clone()).await?,
        (&Method::DELETE, "/api/hub/downloads") => web::routes::download::handle_delete_download(req, db.clone()).await?,
        (&Method::POST, "/api/hub/downloads/verify") => web::routes::download::handle_post_verify(db.clone()).await?,

        // MCP (Model Context Protocol) server management
        (&Method::GET, "/api/mcp/servers") => {
            web::routes::mcp::handle_list_mcp_servers(db.clone()).await?
        }
        (&Method::POST, "/api/mcp/servers") => {
            web::routes::mcp::handle_save_mcp_server(req, db.clone()).await?
        }
        (&Method::DELETE, path) if path.starts_with("/api/mcp/servers/") => {
            let id = &path["/api/mcp/servers/".len()..];
            web::routes::mcp::handle_delete_mcp_server(id, db.clone()).await?
        }
        (&Method::POST, path) if path.starts_with("/api/mcp/servers/") && path.ends_with("/toggle") => {
            let id = &path["/api/mcp/servers/".len()..path.len() - "/toggle".len()];
            web::routes::mcp::handle_toggle_mcp_server(req, id, db.clone()).await?
        }
        (&Method::POST, "/api/mcp/refresh") => {
            web::routes::mcp::handle_refresh_mcp(bridge.clone()).await?
        }
        (&Method::GET, "/api/mcp/tools") => {
            web::routes::mcp::handle_list_mcp_tools(bridge.clone()).await?
        }

        // File operations
        (&Method::GET, "/api/browse") => web::routes::files::handle_get_browse(req, bridge.clone()).await?,
        (&Method::POST, "/api/browse/pick-directory") => web::routes::files::handle_post_pick_directory(bridge.clone(), db.clone()).await?,
        (&Method::POST, "/api/browse/pick-file") => web::routes::files::handle_post_pick_file(bridge.clone()).await?,

        (&Method::POST, "/api/upload") => {
            web::routes::files::handle_post_upload(req, bridge.clone()).await?
        }

        // Tool execution
        (&Method::GET, "/api/tools/available") => {
            web::routes::tools::handle_get_available_tools(bridge.clone()).await?
        }
        (&Method::POST, "/api/tools/execute") => {
            web::routes::tools::handle_post_tools_execute(req, bridge.clone()).await?
        }

        // Web fetch (GET endpoint for easy curl access from model)
        (&Method::GET, "/api/tools/web-fetch") => {
            web::routes::tools::handle_get_web_fetch(req).await?
        }

        // File text extraction (for drag-and-drop attachments)
        (&Method::POST, "/api/file/extract-text") => {
            web::routes::tools::handle_post_extract_text(req).await?
        }

        // Serve persisted screenshot images
        (&Method::GET, path) if path.starts_with("/api/images/") => {
            let rel_path = &path["/api/images/".len()..];
            let file_path = std::path::PathBuf::from("assets/images").join(rel_path);
            if file_path.exists() && file_path.extension().map_or(false, |e| e == "jpg" || e == "jpeg" || e == "png") {
                let bytes = std::fs::read(&file_path).unwrap_or_default();
                let content_type = if file_path.extension().map_or(false, |e| e == "png") {
                    "image/png"
                } else {
                    "image/jpeg"
                };
                Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", content_type)
                    .header("Cache-Control", "public, max-age=86400")
                    .body(Body::from(bytes))
                    .unwrap()
            } else {
                Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from("Not Found"))
                    .unwrap()
            }
        }

        // CORS preflight
        (&Method::OPTIONS, _) => web::routes::static_files::handle_options(bridge.clone()).await?,

        // Static file serving
        (&Method::GET, "/") => web::routes::static_files::handle_index(bridge.clone()).await?,

        (&Method::GET, path)
            if path.starts_with("/assets/")
                || path.ends_with(".svg")
                || path.ends_with(".ico")
                || path.ends_with(".png") =>
        {
            web::routes::static_files::handle_static_asset(path, bridge.clone()).await?
        }

        // 404 Not Found
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not Found"))
            .unwrap(),
    };

    Ok(response)
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // VLM OCR subprocess mode
    if args.iter().any(|a| a == "--vlm-ocr") {
        return vlm_ocr_main(&args);
    }

    // Check for --worker flag BEFORE creating tokio runtime.
    if args.iter().any(|a| a == "--worker") {
        let db_path = args
            .windows(2)
            .find(|w| w[0] == "--db-path")
            .map(|w| w[1].as_str())
            .unwrap_or("assets/llama_chat.db");
        web::worker::worker_main::run_worker(db_path);
        return Ok(());
    }

    // Create tokio runtime for the server
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(server_main())
}

/// VLM OCR subprocess: loads PaddleOCR-VL, runs OCR on an image, prints extracted text to stdout.
/// Runs on CPU (0 GPU layers) so it doesn't interfere with the main model on GPU.
#[cfg(feature = "vision")]
fn vlm_ocr_main(args: &[String]) -> std::io::Result<()> {
    use llama_cpp_2::llama_backend::LlamaBackend;
    use llama_cpp_2::model::LlamaModel;
    use llama_cpp_2::model::params::LlamaModelParams;
    use llama_cpp_2::context::params::LlamaContextParams;
    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::sampling::LlamaSampler;
    use llama_cpp_2::mtmd::{MtmdContext, MtmdContextParams, MtmdBitmap, MtmdInputText};
    use std::ffi::CString;

    let get_arg = |flag: &str| -> Option<&str> {
        args.windows(2).find(|w| w[0] == flag).map(|w| w[1].as_str())
    };
    let model_path = get_arg("--model").unwrap_or("assets/ocr-vlm/PaddleOCR-VL-1.5.gguf");
    let mmproj_path = get_arg("--mmproj").unwrap_or("assets/ocr-vlm/PaddleOCR-VL-1.5-mmproj.gguf");
    let image_path = match get_arg("--image") {
        Some(p) => p,
        None => { eprintln!("Error: --image required"); std::process::exit(1); }
    };

    let io_err = |msg: String| std::io::Error::new(std::io::ErrorKind::Other, msg);

    // Init backend
    let backend = LlamaBackend::init().map_err(|e| io_err(format!("Backend: {e}")))?;

    // Load model on CPU (0 GPU layers — doesn't interfere with main model on GPU)
    let llama_model_params = LlamaModelParams::default().with_n_gpu_layers(0);
    let model = LlamaModel::load_from_file(&backend, model_path, &llama_model_params)
        .map_err(|e| io_err(format!("Model load: {e}")))?;

    // Load mmproj for vision
    let mtmd_params = MtmdContextParams {
        use_gpu: false,
        print_timings: false,
        n_threads: 4,
        media_marker: CString::new("<__media__>").unwrap(),
    };
    let vision = MtmdContext::init_from_file(mmproj_path, &model, &mtmd_params)
        .map_err(|e| io_err(format!("Mmproj: {e}")))?;

    // Create context
    let n_ctx = std::num::NonZeroU32::new(8192);
    let mut ctx_params = LlamaContextParams::default()
        .with_n_ctx(n_ctx)
        .with_n_batch(512)
        .with_flash_attention_policy(0); // GLM-OCR requires flash-attn OFF
    if vision.decode_use_non_causal() {
        ctx_params = ctx_params.with_flash_attention_policy(0);
    }
    let mut ctx = model.new_context(&backend, ctx_params)
        .map_err(|e| io_err(format!("Context: {e}")))?;

    // Load image
    let img_bytes = std::fs::read(image_path)?;
    let bitmap = MtmdBitmap::from_buffer(&vision, &img_bytes)
        .map_err(|e| io_err(format!("Image: {e}")))?;

    // Build prompt with image marker — use simple OCR prompt
    let prompt = "<__media__>OCR the text in this image:";
    let text_input = MtmdInputText {
        text: prompt.to_string(),
        add_special: true,
        parse_special: true,
    };
    let chunks = vision.tokenize(text_input, &[&bitmap])
        .map_err(|e| io_err(format!("Tokenize: {e}")))?;

    // Evaluate prompt + image through the model
    let n_past = chunks.eval_chunks(&vision, &ctx, 0, 0, 512, true)
        .map_err(|e| io_err(format!("Eval: {e}")))?;

    // Generate text output
    // Greedy decoding with repetition penalty to avoid loops
    let mut sampler = LlamaSampler::chain_simple(vec![
        LlamaSampler::penalties(2048, 1.3, 0.0, 0.0), // repeat_penalty=1.3
        LlamaSampler::temp(0.0),
        LlamaSampler::greedy(),
    ]);

    let mut batch = LlamaBatch::new(1, 1);
    let mut output = String::new();
    let mut token_pos = n_past;
    let eos = model.token_eos();

    for _ in 0..2048 {
        let token = sampler.sample(&ctx, -1);
        if token == eos { break; }

        #[allow(deprecated)]
        let s = model.token_to_str(token, llama_cpp_2::model::Special::Tokenize)
            .unwrap_or_default();

        // Stop on special tokens like <|user|>, <|endoftext|>
        if s.contains("<|user|>") || s.contains("<|endoftext|>") || s.contains("<|assistant|>") {
            break;
        }
        output.push_str(&s);

        batch.clear();
        if batch.add(token, token_pos, &[0], true).is_err() { break; }
        if ctx.decode(&mut batch).is_err() { break; }
        token_pos += 1;
    }

    print!("{}", output.trim());
    Ok(())
}

#[cfg(not(feature = "vision"))]
fn vlm_ocr_main(_args: &[String]) -> std::io::Result<()> {
    eprintln!("VLM OCR requires the 'vision' feature");
    std::process::exit(1);
}

async fn server_main() -> std::io::Result<()> {
    // Initialize SQLite database
    let db: SharedDatabase = Arc::new(
        Database::new("assets/llama_chat.db").expect("Failed to initialize SQLite database"),
    );
    println!("📦 SQLite database initialized at assets/llama_chat.db");

    // Run migrations for existing file-based data
    match web::database::migration::migrate_existing_conversations(&db) {
        Ok(count) if count > 0 => {
            println!("📂 Migrated {count} existing conversations to SQLite");
        }
        Ok(_) => {}
        Err(e) => {
            eprintln!("⚠️  Warning: Conversation migration failed: {e}");
        }
    }

    match web::database::migration::migrate_config(&db) {
        Ok(true) => {
            println!("⚙️  Migrated config.json to SQLite");
        }
        Ok(false) => {}
        Err(e) => {
            eprintln!("⚠️  Warning: Config migration failed: {e}");
        }
    }

    // Apply file logging setting from config
    {
        let config = db.load_config();
        web::logger::LOGGER.set_enabled(!config.disable_file_logging);
        if config.disable_file_logging {
            println!("📝 File logging disabled (enable in settings)");
        }
    }

    // Spawn worker process
    #[cfg(not(feature = "mock"))]
    let worker_bridge: SharedWorkerBridge = {
        let pm = Arc::new(
            ProcessManager::spawn("assets/llama_chat.db")
                .expect("Failed to spawn worker process"),
        );
        Arc::new(WorkerBridge::new(pm))
    };

    // Create HTTP service
    let make_svc = make_service_fn({
        #[cfg(not(feature = "mock"))]
        let worker_bridge = worker_bridge.clone();
        let db = db.clone();

        move |_conn| {
            #[cfg(not(feature = "mock"))]
            let worker_bridge = worker_bridge.clone();
            let db = db.clone();

            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    let db = db.clone();
                    #[cfg(not(feature = "mock"))]
                    {
                        handle_request(req, worker_bridge.clone(), db)
                    }
                    #[cfg(feature = "mock")]
                    {
                        handle_request(req, db)
                    }
                }))
            }
        }
    });

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], 18080));
    let server = Server::bind(&addr).serve(make_svc);

    println!("🦙 LLaMA Chat Web Server starting on http://{addr}");
    println!("📡 Worker process spawned for model inference");
    println!("Available endpoints:");
    println!("  GET  /health               - Health check");
    println!("  POST /api/chat             - Chat with LLaMA");
    println!("  GET  /api/config           - Get sampler configuration");
    println!("  POST /api/config           - Update sampler configuration");
    println!("  GET  /api/model/status     - Get current model status");
    println!("  GET  /api/model/history    - Get model path history");
    println!("  POST /api/model/history    - Add model path to history");
    println!("  POST /api/model/load       - Load a specific model");
    println!("  POST /api/model/unload     - Unload current model");
    println!("  POST /api/model/hard-unload - Force kill worker (reclaim all memory)");
    println!("  POST /api/upload           - Upload model file");
    println!("  GET  /api/conversations    - List conversation files");
    println!("  POST /api/tools/execute    - Execute tool calls");
    println!("  GET  /api/tools/web-fetch  - Fetch web page as text");
    println!("  GET  /api/browse           - Browse model files");
    println!("  GET  /                     - Web interface");

    server
        .await
        .map_err(std::io::Error::other)?;

    Ok(())
}
