// Simple web server version of LLaMA Chat (without Tauri)
#[allow(unused_imports)]
#[macro_use]
extern crate llama_chat_types;

mod web; // Declare web module for model capabilities and utilities
mod vlm_ocr;
mod server;

// Import all types and functions from web modules
use web::database::SharedDatabase;

use std::convert::Infallible;

#[cfg(not(feature = "mock"))]
use web::worker::worker_bridge::SharedWorkerBridge;

// HTTP server using hyper
use hyper::{Body, Method, Request, Response, StatusCode};

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

        // Conversation compact (manual compaction from UI)
        (&Method::POST, path) if path.starts_with("/api/conversations/") && path.ends_with("/compact") => {
            let id = &path["/api/conversations/".len()..path.len()-"/compact".len()];
            web::routes::conversation::handle_compact_conversation(id, bridge.clone()).await?
        }

        // Summary edit/delete (PATCH/DELETE must be before generic catch-alls)
        (&Method::PATCH, path) if path.starts_with("/api/conversations/") && path.ends_with("/summary") => {
            let id = &path["/api/conversations/".len()..path.len() - "/summary".len()];
            web::routes::conversation::handle_update_summary(req, id, db.clone()).await?
        }
        (&Method::DELETE, path) if path.starts_with("/api/conversations/") && path.ends_with("/summary") => {
            let id = &path["/api/conversations/".len()..path.len() - "/summary".len()];
            web::routes::conversation::handle_delete_summary(id, db.clone()).await?
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
        (&Method::POST, path) if path.starts_with("/api/conversation/") && path.ends_with("/queue") => {
            let id = &path["/api/conversation/".len()..path.len()-"/queue".len()];
            web::routes::providers::handle_queue_message(req, db.clone(), id).await?
        }

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
        (&Method::GET, "/api/providers/configured") => {
            web::routes::providers::handle_list_configured_providers(db.clone()).await?
        }
        (&Method::GET, "/api/providers/cli-status") => {
            web::routes::providers::handle_list_cli_providers().await?
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
        return vlm_ocr::vlm_ocr_main(&args);
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
    rt.block_on(server::server_main())
}
