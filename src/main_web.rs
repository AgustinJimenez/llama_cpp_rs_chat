// Simple web server version of LLaMA Chat (without Tauri)
mod web;  // Declare web module for model capabilities and utilities

// Import all types and functions from web modules
use web::*;

use std::net::SocketAddr;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};

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
) -> std::result::Result<Response<Body>, Infallible> {
    handle_request_impl(req, Some(llama_state)).await
}

#[cfg(feature = "mock")]
async fn handle_request(
    req: Request<Body>,
) -> std::result::Result<Response<Body>, Infallible> {
    handle_request_impl(req, None).await
}

async fn handle_request_impl(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))]
    llama_state: Option<SharedLlamaState>,
    #[cfg(feature = "mock")]
    _llama_state: Option<()>,
) -> std::result::Result<Response<Body>, Infallible> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();

    #[cfg(not(feature = "mock"))]
    let state = llama_state.unwrap();

    #[cfg(feature = "mock")]
    let state = ();

    let response = match (&method, path.as_str()) {
        // Health check
        (&Method::GET, "/health") => {
            web::routes::health::handle(state).await?
        }

        // System monitoring
        (&Method::GET, "/api/system/usage") => {
            web::routes::system::handle_system_usage().await?
        }

        // Chat endpoints
        (&Method::POST, "/api/chat") => {
            web::routes::chat::handle_post_chat(req, state).await?
        }

        (&Method::POST, "/api/chat/stream") => {
            web::routes::chat::handle_post_chat_stream(req, state).await?
        }

        (&Method::GET, "/ws/chat/stream") => {
            web::routes::chat::handle_websocket_chat_stream(req, state).await?
        }

        (&Method::GET, path) if path.starts_with("/ws/conversation/watch/") => {
            web::routes::chat::handle_conversation_watch_websocket(req, path, state).await?
        }

        // Configuration endpoints
        (&Method::GET, "/api/config") => {
            web::routes::config::handle_get_config(state).await?
        }

        (&Method::POST, "/api/config") => {
            web::routes::config::handle_post_config(req, state).await?
        }

        // Conversation endpoints
        (&Method::GET, path) if path.starts_with("/api/conversation/") => {
            web::routes::conversation::handle_get_conversation(path, state).await?
        }

        (&Method::GET, "/api/conversations") => {
            web::routes::conversation::handle_get_conversations(state).await?
        }

        (&Method::DELETE, path) if path.starts_with("/api/conversations/") => {
            web::routes::conversation::handle_delete_conversation(path, state).await?
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

        // File operations
        (&Method::GET, "/api/browse") => {
            web::routes::files::handle_get_browse(req, state).await?
        }

        (&Method::POST, "/api/upload") => {
            web::routes::files::handle_post_upload(req, state).await?
        }

        // Tool execution
        (&Method::POST, "/api/tools/execute") => {
            web::routes::tools::handle_post_tools_execute(req, state).await?
        }

        // CORS preflight
        (&Method::OPTIONS, _) => {
            web::routes::static_files::handle_options(state).await?
        }

        // Static file serving
        (&Method::GET, "/") => {
            web::routes::static_files::handle_index(state).await?
        }

        (&Method::GET, path) if path.starts_with("/assets/") || path.ends_with(".svg") || path.ends_with(".ico") || path.ends_with(".png") => {
            web::routes::static_files::handle_static_asset(path, state).await?
        }

        // 404 Not Found
        _ => {
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("Not Found"))
                .unwrap()
        }
    };

    Ok(response)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Create shared LLaMA state
    #[cfg(not(feature = "mock"))]
    let llama_state: SharedLlamaState = Arc::new(Mutex::new(None));

    // Note: ConversationLogger will be created per chat request, not globally

    // Create HTTP service
    let make_svc = make_service_fn({
        #[cfg(not(feature = "mock"))]
        let llama_state = llama_state.clone();

        move |_conn| {
            #[cfg(not(feature = "mock"))]
            let llama_state = llama_state.clone();

            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    #[cfg(not(feature = "mock"))]
                    {
                        handle_request(req, llama_state.clone())
                    }
                    #[cfg(feature = "mock")]
                    {
                        handle_request(req)
                    }
                }))
            }
        }
    });

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], 8000));
    let server = Server::bind(&addr).serve(make_svc);

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
    println!("  POST /api/upload           - Upload model file");
    println!("  GET  /api/conversations    - List conversation files");
    println!("  POST /api/tools/execute    - Execute tool calls");
    println!("  GET  /api/browse           - Browse model files");
    println!("  GET  /                     - Web interface");

    server.await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    Ok(())
}
