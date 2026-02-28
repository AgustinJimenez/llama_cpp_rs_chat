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

        // System monitoring
        (&Method::GET, "/api/system/usage") => web::routes::system::handle_system_usage().await?,

        // Frontend log ingestion (web-only)
        (&Method::POST, "/api/logs/frontend") => {
            web::routes::frontend_logs::handle_post_frontend_logs(req).await?
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

        // Conversation config (must be before the catch-all /api/conversation/ route)
        (&Method::GET, path) if path.starts_with("/api/conversations/") && path.ends_with("/config") => {
            web::routes::config::handle_get_conversation_config(path, bridge.clone(), db.clone()).await?
        }

        (&Method::POST, path) if path.starts_with("/api/conversations/") && path.ends_with("/config") => {
            web::routes::config::handle_post_conversation_config(req, path, bridge.clone(), db.clone()).await?
        }

        // Conversation metrics
        (&Method::GET, path) if path.starts_with("/api/conversations/") && path.ends_with("/metrics") => {
            web::routes::conversation::handle_get_conversation_metrics(path, db.clone()).await?
        }

        // Conversation endpoints
        (&Method::GET, path) if path.starts_with("/api/conversation/") => {
            web::routes::conversation::handle_get_conversation(path, bridge.clone(), db.clone()).await?
        }

        (&Method::GET, "/api/conversations") => {
            web::routes::conversation::handle_get_conversations(bridge.clone(), db.clone()).await?
        }

        (&Method::DELETE, path) if path.starts_with("/api/conversations/") => {
            web::routes::conversation::handle_delete_conversation(path, bridge.clone(), db.clone()).await?
        }

        // Model endpoints
        (&Method::GET, "/api/model/info") => {
            web::routes::model::handle_get_model_info(req, bridge.clone()).await?
        }

        (&Method::GET, "/api/model/status") => {
            web::routes::model::handle_get_model_status(bridge.clone()).await?
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

        // HuggingFace Hub search & download
        (&Method::GET, "/api/hub/search") => web::routes::hub::handle_search(req).await?,
        (&Method::GET, "/api/hub/tree") => web::routes::hub::handle_tree(req).await?,
        (&Method::POST, "/api/hub/download") => web::routes::download::handle_post_download(req, db.clone()).await?,
        (&Method::GET, "/api/hub/downloads") => web::routes::download::handle_get_downloads(db.clone()).await?,
        (&Method::POST, "/api/hub/downloads/verify") => web::routes::download::handle_post_verify(db.clone()).await?,

        // File operations
        (&Method::GET, "/api/browse") => web::routes::files::handle_get_browse(req, bridge.clone()).await?,
        (&Method::POST, "/api/browse/pick-directory") => web::routes::files::handle_post_pick_directory(bridge.clone()).await?,
        (&Method::POST, "/api/browse/pick-file") => web::routes::files::handle_post_pick_file(bridge.clone()).await?,

        (&Method::POST, "/api/upload") => {
            web::routes::files::handle_post_upload(req, bridge.clone()).await?
        }

        // Tool execution
        (&Method::POST, "/api/tools/execute") => {
            web::routes::tools::handle_post_tools_execute(req, bridge.clone()).await?
        }

        // Web fetch (GET endpoint for easy curl access from model)
        (&Method::GET, "/api/tools/web-fetch") => {
            web::routes::tools::handle_get_web_fetch(req).await?
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
    // Check for --worker flag BEFORE creating tokio runtime.
    // The worker creates its own runtimes internally for async operations,
    // so it must not run inside an existing tokio runtime.
    let args: Vec<String> = std::env::args().collect();
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
    let addr = SocketAddr::from(([0, 0, 0, 0], 8000));
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
