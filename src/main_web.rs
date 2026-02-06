// Simple web server version of LLaMA Chat (without Tauri)
mod web; // Declare web module for model capabilities and utilities

// Import all types and functions from web modules
use web::database::{Database, SharedDatabase};
use web::*;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
#[cfg(not(feature = "mock"))]
use std::sync::Mutex;

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
    llama_state: SharedLlamaState,
    db: SharedDatabase,
) -> std::result::Result<Response<Body>, Infallible> {
    handle_request_impl(req, Some(llama_state), db).await
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
    #[cfg(not(feature = "mock"))] llama_state: Option<SharedLlamaState>,
    #[cfg(feature = "mock")] _llama_state: Option<()>,
    db: SharedDatabase,
) -> std::result::Result<Response<Body>, Infallible> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();

    #[cfg(not(feature = "mock"))]
    let state = llama_state.unwrap();

    #[cfg(feature = "mock")]
    let state = ();

    let response = match (&method, path.as_str()) {
        // Health check
        (&Method::GET, "/health") => web::routes::health::handle(state).await?,

        // System monitoring
        (&Method::GET, "/api/system/usage") => web::routes::system::handle_system_usage().await?,

        // Frontend log ingestion (web-only)
        (&Method::POST, "/api/logs/frontend") => {
            web::routes::frontend_logs::handle_post_frontend_logs(req).await?
        }

        // Chat endpoints
        (&Method::POST, "/api/chat") => {
            web::routes::chat::handle_post_chat(req, state, db.clone()).await?
        }

        (&Method::POST, "/api/chat/stream") => {
            web::routes::chat::handle_post_chat_stream(req, state, db.clone()).await?
        }

        (&Method::GET, "/ws/chat/stream") => {
            web::routes::chat::handle_websocket_chat_stream(req, state, db.clone()).await?
        }

        (&Method::GET, path) if path.starts_with("/ws/conversation/watch/") => {
            web::routes::chat::handle_conversation_watch_websocket(req, path, state, db.clone())
                .await?
        }

        // Configuration endpoints
        (&Method::GET, "/api/config") => web::routes::config::handle_get_config(state).await?,

        (&Method::POST, "/api/config") => {
            web::routes::config::handle_post_config(req, state).await?
        }

        // Conversation endpoints
        (&Method::GET, path) if path.starts_with("/api/conversation/") => {
            web::routes::conversation::handle_get_conversation(path, state, db.clone()).await?
        }

        (&Method::GET, "/api/conversations") => {
            web::routes::conversation::handle_get_conversations(state, db.clone()).await?
        }

        (&Method::DELETE, path) if path.starts_with("/api/conversations/") => {
            web::routes::conversation::handle_delete_conversation(path, state, db.clone()).await?
        }

        // Model endpoints
        (&Method::GET, "/api/model/info") => {
            web::routes::model::handle_get_model_info(req, state).await?
        }

        (&Method::GET, "/api/model/status") => {
            web::routes::model::handle_get_model_status(state).await?
        }

        (&Method::GET, "/api/model/history") => {
            web::routes::model::handle_get_model_history(state).await?
        }

        (&Method::POST, "/api/model/history") => {
            web::routes::model::handle_post_model_history(req, state).await?
        }

        (&Method::POST, "/api/model/load") => {
            web::routes::model::handle_post_model_load(req, state).await?
        }

        (&Method::POST, "/api/model/unload") => {
            web::routes::model::handle_post_model_unload(state).await?
        }

        (&Method::POST, "/api/model/force-reset") => {
            web::routes::model::handle_post_model_force_reset(state).await?
        }

        (&Method::POST, "/api/generation/abort") => {
            web::routes::model::handle_post_generation_abort(state).await?
        }

        // File operations
        (&Method::GET, "/api/browse") => web::routes::files::handle_get_browse(req, state).await?,

        (&Method::POST, "/api/upload") => {
            web::routes::files::handle_post_upload(req, state).await?
        }

        // Tool execution
        (&Method::POST, "/api/tools/execute") => {
            web::routes::tools::handle_post_tools_execute(req, state).await?
        }

        // CORS preflight
        (&Method::OPTIONS, _) => web::routes::static_files::handle_options(state).await?,

        // Static file serving - DISABLED (use port 4000 for frontend with hot reload)
        // (&Method::GET, "/") => web::routes::static_files::handle_index(state).await?,
        //
        // (&Method::GET, path)
        //     if path.starts_with("/assets/")
        //         || path.ends_with(".svg")
        //         || path.ends_with(".ico")
        //         || path.ends_with(".png") =>
        // {
        //     web::routes::static_files::handle_static_asset(path, state).await?
        // }

        // 404 Not Found
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not Found"))
            .unwrap(),
    };

    Ok(response)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Initialize SQLite database
    let db: SharedDatabase = Arc::new(
        Database::new("assets/llama_chat.db").expect("Failed to initialize SQLite database"),
    );
    println!("📦 SQLite database initialized at assets/llama_chat.db");

    // Run migrations for existing file-based data
    match web::database::migration::migrate_existing_conversations(&db) {
        Ok(count) if count > 0 => {
            println!("📂 Migrated {} existing conversations to SQLite", count);
        }
        Ok(_) => {}
        Err(e) => {
            eprintln!("⚠️  Warning: Conversation migration failed: {}", e);
        }
    }

    match web::database::migration::migrate_config(&db) {
        Ok(true) => {
            println!("⚙️  Migrated config.json to SQLite");
        }
        Ok(false) => {}
        Err(e) => {
            eprintln!("⚠️  Warning: Config migration failed: {}", e);
        }
    }

    // Create shared LLaMA state
    #[cfg(not(feature = "mock"))]
    let llama_state: SharedLlamaState = Arc::new(Mutex::new(None));

    // Create HTTP service
    let make_svc = make_service_fn({
        #[cfg(not(feature = "mock"))]
        let llama_state = llama_state.clone();
        let db = db.clone();

        move |_conn| {
            #[cfg(not(feature = "mock"))]
            let llama_state = llama_state.clone();
            let db = db.clone();

            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    let db = db.clone();
                    #[cfg(not(feature = "mock"))]
                    {
                        handle_request(req, llama_state.clone(), db)
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

    println!("🦙 Binding to port 8000...");
    let server = match Server::try_bind(&addr) {
        Ok(builder) => {
            println!("✅ Successfully bound to {}", addr);
            builder.serve(make_svc)
        }
        Err(e) => {
            eprintln!("❌ Failed to bind to {}: {}", addr, e);
            eprintln!("   Port 8000 might be in use by another process");
            return Err(std::io::Error::new(std::io::ErrorKind::AddrInUse, e));
        }
    };

    println!("🦙 LLaMA Chat Web Server starting on http://{}", addr);
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
    println!("  POST /api/model/force-reset - Force-reset model state (use if unload fails)");
    println!("  POST /api/upload           - Upload model file");
    println!("  GET  /api/conversations    - List conversation files");
    println!("  POST /api/tools/execute    - Execute tool calls");
    println!("  GET  /api/browse           - Browse model files");
    println!("  GET  /                     - Web interface");
    println!("\n✅ Server is now listening and ready to accept connections!");
    println!("   Press Ctrl+C to stop\n");

    match server.await {
        Ok(_) => {
            println!("Server shut down gracefully");
            Ok(())
        }
        Err(e) => {
            eprintln!("❌ Server error: {}", e);
            Err(std::io::Error::new(std::io::ErrorKind::Other, e))
        }
    }
}
