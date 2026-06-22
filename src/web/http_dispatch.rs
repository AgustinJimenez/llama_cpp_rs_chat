//! Shared HTTP API dispatcher + server bootstrap.
//!
//! Hosts the `/api/*` route table used by both the standalone web binary
//! (`main_web.rs`) and the Tauri desktop app, so the desktop app can serve the
//! same API (agents, conversations, config, …) against its own database.

use std::convert::Infallible;

use hyper::{Body, Method, Request, Response, StatusCode};
use llama_chat_web::remote;

#[cfg(not(feature = "mock"))]
use super::worker_pool::WorkerPool;
use super::database::SharedDatabase;

#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub async fn dispatch(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] worker_pool: Option<WorkerPool>,
    #[cfg(feature = "mock")] _worker_bridge: Option<()>,
    db: SharedDatabase,
    peer_addr: std::net::SocketAddr,
) -> std::result::Result<Response<Body>, Infallible> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();

    // Auth: non-localhost requests must carry a valid Bearer token.
    // Exempt: health check, static assets, CORS preflight, remote status (to show QR),
    //         and WebSocket upgrades (token passed in URL hash, validated on connect).
    let is_local = peer_addr.ip().is_loopback()
        && req.headers().get("x-forwarded-for").is_none();
    // Paths that never need a token
    let auth_exempt = path == "/health"
        || path == "/api/remote/status"
        || path.starts_with("/assets/")
        || path.ends_with(".svg")
        || path.ends_with(".ico")
        || path.ends_with(".png")
        || path == "/"
        || method == Method::OPTIONS
        || path.starts_with("/ws/"); // WS token handled separately

    if !is_local && !auth_exempt {
        if let Some(token) = db.get_remote_access_token() {
            if !token.is_empty() {
                let auth_header = req.headers().get("authorization").and_then(|v| v.to_str().ok());
                if !remote::check_bearer_token(auth_header, &token) {
                    return Ok(Response::builder()
                        .status(StatusCode::UNAUTHORIZED)
                        .header("www-authenticate", "Bearer")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"error":"Unauthorized"}"#))
                        .unwrap());
                }
            }
        }
    }

    #[cfg(not(feature = "mock"))]
    let pool = worker_pool.expect("Worker pool missing");

    #[cfg(not(feature = "mock"))]
    let bridge = pool
        .get("default")
        .expect("Default worker missing from pool");

    #[cfg(feature = "mock")]
    let bridge = ();
    #[cfg(feature = "mock")]
    let pool = ();

    let response = match (&method, path.as_str()) {
        // Health check
        (&Method::GET, "/health") => super::routes::health::handle(bridge.clone()).await?,

        // App info & API docs
        (&Method::GET, "/api/info") => super::routes::system::handle_app_info().await?,
        (&Method::GET, "/api/docs") => super::routes::system::handle_api_docs().await?,

        // System monitoring
        (&Method::GET, "/api/system/usage") => super::routes::system::handle_system_usage().await?,
        (&Method::GET, "/api/system/processes") => {
            super::routes::system::handle_background_processes(db.clone()).await?
        }
        (&Method::GET, path)
            if path.starts_with("/api/system/processes/") && path.ends_with("/output") =>
        {
            let pid_str = &path["/api/system/processes/".len()..path.len() - "/output".len()];
            match pid_str.parse::<u32>() {
                Ok(pid) => super::routes::system::handle_process_output(pid).await?,
                Err(_) => Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Body::from("Invalid PID"))
                    .unwrap(),
            }
        }
        (&Method::POST, "/api/system/processes/kill") => {
            super::routes::system::handle_kill_process(req, db.clone()).await?
        }
        (&Method::POST, "/api/desktop/abort") => {
            super::routes::system::handle_desktop_abort().await?
        }

        // Browser panel (web mode — opens wry native WebView window)
        (&Method::POST, "/api/browser/navigate") => {
            super::routes::system::handle_browser_navigate(req).await?
        }
        (&Method::POST, "/api/browser/close") => {
            super::routes::system::handle_browser_close().await?
        }

        // Frontend log ingestion (web-only)
        (&Method::POST, "/api/logs/frontend") => {
            super::routes::frontend_logs::handle_post_frontend_logs(req).await?
        }

        // App-level frontend/runtime errors
        (&Method::POST, "/api/errors") => {
            super::routes::app_errors::handle_record_app_error(req, db.clone()).await?
        }
        (&Method::GET, "/api/errors") => {
            super::routes::app_errors::handle_get_app_errors(&req, db.clone()).await?
        }
        (&Method::DELETE, "/api/errors") => {
            super::routes::app_errors::handle_clear_app_errors(db.clone()).await?
        }

        // Chat endpoints
        (&Method::POST, "/api/chat") => {
            super::routes::chat::handle_post_chat(req, pool.clone(), db.clone()).await?
        }

        (&Method::POST, "/api/chat/stream") => {
            super::routes::chat::handle_post_chat_stream(req, pool.clone(), db.clone()).await?
        }

        (&Method::POST, "/api/chat/cancel") => {
            super::routes::chat::handle_post_chat_cancel(bridge.clone()).await?
        }

        (&Method::GET, "/ws/chat/stream") => {
            super::routes::chat::handle_websocket_chat_stream(req, pool.clone(), db.clone()).await?
        }

        (&Method::GET, path) if path.starts_with("/ws/conversation/watch/") => {
            super::routes::chat::handle_conversation_watch_websocket(
                req,
                path,
                pool.clone(),
                db.clone(),
            )
            .await?
        }

        (&Method::GET, "/ws/status") => {
            super::routes::status::handle_status_websocket(req, bridge.clone()).await?
        }

        // Configuration endpoints
        (&Method::GET, "/api/config") => {
            super::routes::config::handle_get_config(bridge.clone(), db.clone()).await?
        }

        (&Method::POST, "/api/config") => {
            super::routes::config::handle_post_config(req, bridge.clone(), db.clone()).await?
        }

        // Provider API keys
        (&Method::GET, "/api/config/provider-keys") => {
            super::routes::config::handle_get_provider_keys(db.clone()).await?
        }
        (&Method::POST, "/api/config/provider-keys") => {
            super::routes::config::handle_set_provider_key(req, db.clone()).await?
        }
        (&Method::GET, "/api/config/active-provider") => {
            super::routes::config::handle_get_active_provider(db.clone()).await?
        }
        (&Method::POST, "/api/config/active-provider") => {
            super::routes::config::handle_set_active_provider(req, db.clone()).await?
        }

        // Conversation config (must be before the catch-all /api/conversation/ route)
        (&Method::GET, path)
            if path.starts_with("/api/conversations/") && path.ends_with("/config") =>
        {
            super::routes::config::handle_get_conversation_config(path, bridge.clone(), db.clone())
                .await?
        }

        // Conversation event log (in-memory debug events)
        (&Method::GET, path)
            if path.starts_with("/api/conversations/") && path.ends_with("/events") =>
        {
            super::routes::conversation::handle_get_conversation_events(
                path,
                pool.clone(),
                db.clone(),
            )
            .await?
        }

        // Conversation token analysis
        (&Method::GET, path)
            if path.starts_with("/api/conversations/") && path.ends_with("/token-analysis") =>
        {
            let id = &path["/api/conversations/".len()..path.len() - "/token-analysis".len()];
            super::routes::conversation::handle_conversation_token_analysis(id, db.clone()).await?
        }

        // Conversation metrics
        (&Method::GET, path)
            if path.starts_with("/api/conversations/") && path.ends_with("/metrics") =>
        {
            super::routes::conversation::handle_get_conversation_metrics(path, db.clone()).await?
        }

        // Conversation truncate (for message editing)
        (&Method::POST, path)
            if path.starts_with("/api/conversations/") && path.ends_with("/truncate") =>
        {
            super::routes::conversation::handle_truncate_conversation(req, path, db.clone()).await?
        }

        // Conversation compact (manual compaction from UI)
        (&Method::POST, path)
            if path.starts_with("/api/conversations/") && path.ends_with("/compact") =>
        {
            let id = &path["/api/conversations/".len()..path.len() - "/compact".len()];
            super::routes::conversation::handle_compact_conversation(id, pool.clone(), db.clone())
                .await?
        }

        // Summary edit/delete (PATCH/DELETE must be before generic catch-alls)
        (&Method::PATCH, path)
            if path.starts_with("/api/conversations/") && path.ends_with("/summary") =>
        {
            let id = &path["/api/conversations/".len()..path.len() - "/summary".len()];
            super::routes::conversation::handle_update_summary(req, id, db.clone()).await?
        }
        (&Method::DELETE, path)
            if path.starts_with("/api/conversations/") && path.ends_with("/summary") =>
        {
            let id = &path["/api/conversations/".len()..path.len() - "/summary".len()];
            super::routes::conversation::handle_delete_summary(id, db.clone()).await?
        }

        // Conversation rename (PATCH must be before DELETE catch-all)
        (&Method::PATCH, path)
            if path.starts_with("/api/conversations/") && path.ends_with("/title") =>
        {
            let id = &path["/api/conversations/".len()..path.len() - "/title".len()];
            super::routes::conversation::handle_rename_conversation(req, id, db.clone()).await?
        }

        // Conversation export (must be before generic /api/conversation/{id})
        (&Method::GET, path)
            if path.starts_with("/api/conversation/") && path.ends_with("/export") =>
        {
            let id = &path["/api/conversation/".len()..path.len() - "/export".len()];
            super::routes::conversation::handle_export_conversation(&req, id, db.clone()).await?
        }

        // Conversation endpoints
        (&Method::POST, path)
            if path.starts_with("/api/conversation/") && path.ends_with("/queue") =>
        {
            let id = &path["/api/conversation/".len()..path.len() - "/queue".len()];
            super::routes::providers::handle_queue_message(req, db.clone(), id).await?
        }

        (&Method::GET, path) if path.starts_with("/api/conversation/") => {
            super::routes::conversation::handle_get_conversation(path, bridge.clone(), db.clone())
                .await?
        }

        (&Method::GET, "/api/conversations") => {
            super::routes::conversation::handle_get_conversations(&req, bridge.clone(), db.clone())
                .await?
        }

        (&Method::POST, "/api/conversations") => {
            super::routes::conversation::handle_create_conversation(req, pool.clone(), db.clone())
                .await?
        }

        (&Method::PATCH, path)
            if path.starts_with("/api/conversations/") && path.ends_with("/worker") =>
        {
            let id = &path["/api/conversations/".len()..path.len() - "/worker".len()];
            super::routes::workers::handle_patch_conversation_worker(
                req,
                id,
                pool.clone(),
                db.clone(),
            )
            .await?
        }

        // Batch delete (must be before single delete /api/conversations/{id})
        (&Method::DELETE, "/api/conversations/batch") => {
            super::routes::conversation::handle_batch_delete_conversations(req, db.clone()).await?
        }

        (&Method::DELETE, path) if path.starts_with("/api/conversations/") => {
            super::routes::conversation::handle_delete_conversation(path, bridge.clone(), db.clone())
                .await?
        }

        // Provider endpoints
        (&Method::GET, "/api/providers") => {
            super::routes::providers::handle_list_providers(db.clone()).await?
        }
        (&Method::GET, "/api/providers/configured") => {
            super::routes::providers::handle_list_configured_providers(db.clone()).await?
        }
        (&Method::GET, "/api/providers/cli-status") => {
            super::routes::providers::handle_list_cli_providers().await?
        }
        (&Method::GET, path)
            if path.starts_with("/api/providers/") && path.ends_with("/models") =>
        {
            let provider_id = path
                .trim_start_matches("/api/providers/")
                .trim_end_matches("/models")
                .trim_end_matches('/');
            super::routes::providers::handle_provider_models(provider_id, db.clone()).await?
        }
        (&Method::POST, path)
            if path.starts_with("/api/providers/") && path.ends_with("/stream") =>
        {
            let provider_id = path
                .trim_start_matches("/api/providers/")
                .trim_end_matches("/stream")
                .trim_end_matches('/');
            super::routes::providers::handle_provider_stream(req, db.clone(), provider_id, Some(bridge.clone())).await?
        }
        (&Method::POST, path)
            if path.starts_with("/api/providers/") && path.ends_with("/generate") =>
        {
            let provider_id = path
                .trim_start_matches("/api/providers/")
                .trim_end_matches("/generate")
                .trim_end_matches('/');
            super::routes::providers::handle_provider_generate(req, db.clone(), provider_id, Some(bridge.clone())).await?
        }

        // Multi-worker management
        (&Method::GET, "/api/workers") => {
            super::routes::workers::handle_list_workers(pool.clone()).await?
        }
        (&Method::POST, "/api/workers") => {
            super::routes::workers::handle_create_worker(req, pool.clone()).await?
        }
        (&Method::GET, path) if path.starts_with("/api/workers/") && path.ends_with("/status") => {
            let worker_id = &path["/api/workers/".len()..path.len() - "/status".len()];
            super::routes::workers::handle_get_worker_status(
                worker_id.trim_end_matches('/'),
                pool.clone(),
            )
            .await?
        }
        (&Method::DELETE, path) if path.starts_with("/api/workers/") => {
            let worker_id = &path["/api/workers/".len()..];
            super::routes::workers::handle_delete_worker(worker_id, pool.clone(), db.clone()).await?
        }

        // Model endpoints
        (&Method::GET, "/api/model/info") => {
            super::routes::model::handle_get_model_info(req, bridge.clone()).await?
        }

        (&Method::GET, "/api/model/status") => {
            super::routes::model::handle_get_model_status(pool.clone(), db.clone()).await?
        }

        (&Method::GET, "/api/model/history") => {
            super::routes::model::handle_get_model_history(bridge.clone(), db.clone()).await?
        }

        (&Method::POST, "/api/model/history") => {
            super::routes::model::handle_post_model_history(req, bridge.clone(), db.clone()).await?
        }

        (&Method::POST, "/api/model/load") => {
            super::routes::model::handle_post_model_load(req, bridge.clone(), pool.clone(), db.clone()).await?
        }

        (&Method::POST, "/api/model/unload") => {
            super::routes::model::handle_post_model_unload(bridge.clone()).await?
        }

        (&Method::POST, "/api/model/hard-unload") => {
            super::routes::model::handle_post_model_hard_unload(bridge.clone()).await?
        }

        (&Method::GET, "/api/backends") => {
            super::routes::model::handle_get_backends(bridge.clone()).await?
        }

        (&Method::POST, "/api/backends/install") => {
            super::routes::model::handle_post_backends_install().await?
        }

        // HuggingFace Hub search & download
        (&Method::GET, "/api/hub/search") => super::routes::hub::handle_search(req).await?,
        (&Method::GET, "/api/hub/tree") => super::routes::hub::handle_tree(req).await?,
        (&Method::POST, "/api/hub/download") => {
            super::routes::download::handle_post_download(req, db.clone()).await?
        }
        (&Method::GET, "/api/hub/downloads") => {
            super::routes::download::handle_get_downloads(db.clone()).await?
        }
        (&Method::DELETE, "/api/hub/downloads") => {
            super::routes::download::handle_delete_download(req, db.clone()).await?
        }
        (&Method::POST, "/api/hub/downloads/verify") => {
            super::routes::download::handle_post_verify(db.clone()).await?
        }

        // MCP (Model Context Protocol) server management
        (&Method::GET, "/api/mcp/servers") => {
            super::routes::mcp::handle_list_mcp_servers(db.clone()).await?
        }
        (&Method::POST, "/api/mcp/servers") => {
            super::routes::mcp::handle_save_mcp_server(req, db.clone()).await?
        }
        (&Method::DELETE, path) if path.starts_with("/api/mcp/servers/") => {
            let id = &path["/api/mcp/servers/".len()..];
            super::routes::mcp::handle_delete_mcp_server(id, db.clone()).await?
        }
        (&Method::POST, path)
            if path.starts_with("/api/mcp/servers/") && path.ends_with("/toggle") =>
        {
            let id = &path["/api/mcp/servers/".len()..path.len() - "/toggle".len()];
            super::routes::mcp::handle_toggle_mcp_server(req, id, db.clone()).await?
        }
        (&Method::POST, "/api/mcp/refresh") => {
            super::routes::mcp::handle_refresh_mcp(bridge.clone()).await?
        }
        (&Method::GET, "/api/mcp/tools") => {
            super::routes::mcp::handle_list_mcp_tools(bridge.clone()).await?
        }

        // Agent management
        (&Method::GET, "/api/agents") => {
            super::routes::agents::handle_list_agents(db.clone()).await?
        }
        (&Method::POST, "/api/agents") => {
            super::routes::agents::handle_create_agent(req, db.clone()).await?
        }
        // Agent statuses — must be before /api/agents/:id catch-all
        (&Method::GET, "/api/agents/statuses") => {
            super::routes::agents::handle_get_agent_statuses(pool.clone(), db.clone()).await?
        }
        (&Method::GET, path) if path.starts_with("/api/agents/") => {
            let id = &path["/api/agents/".len()..];
            super::routes::agents::handle_get_agent(id, db.clone()).await?
        }
        (&Method::PUT, path) if path.starts_with("/api/agents/") => {
            let id = &path["/api/agents/".len()..];
            super::routes::agents::handle_update_agent(req, id, db.clone()).await?
        }
        (&Method::DELETE, path) if path.starts_with("/api/agents/") => {
            let id = &path["/api/agents/".len()..];
            super::routes::agents::handle_delete_agent(id, db.clone()).await?
        }
        // Agent lifecycle (activate/stop) — must be before /api/agents/:id PUT catch-all
        (&Method::POST, path)
            if path.starts_with("/api/agents/") && path.ends_with("/activate") =>
        {
            let id = &path["/api/agents/".len()..path.len() - "/activate".len()];
            super::routes::agents::handle_activate_agent(id, pool.clone(), db.clone()).await?
        }
        (&Method::POST, path)
            if path.starts_with("/api/agents/") && path.ends_with("/stop") =>
        {
            let id = &path["/api/agents/".len()..path.len() - "/stop".len()];
            super::routes::agents::handle_stop_agent(id, pool.clone(), db.clone()).await?
        }
        (&Method::GET, path)
            if path.starts_with("/api/conversations/") && path.ends_with("/agent") =>
        {
            let conv_id = &path["/api/conversations/".len()..path.len() - "/agent".len()];
            super::routes::agents::handle_get_conversation_agent(conv_id, db.clone()).await?
        }
        (&Method::POST, path)
            if path.starts_with("/api/conversations/") && path.ends_with("/agent") =>
        {
            let conv_id = &path["/api/conversations/".len()..path.len() - "/agent".len()];
            {
                #[cfg(not(feature = "mock"))]
                { super::routes::agents::handle_set_conversation_agent(req, conv_id, pool.clone(), db.clone()).await? }
                #[cfg(feature = "mock")]
                { super::routes::agents::handle_set_conversation_agent(req, conv_id, db.clone()).await? }
            }
        }
        // File operations
        (&Method::GET, "/api/browse") => {
            super::routes::files::handle_get_browse(req, bridge.clone()).await?
        }
        (&Method::POST, "/api/browse/pick-directory") => {
            super::routes::files::handle_post_pick_directory(bridge.clone(), db.clone()).await?
        }
        (&Method::POST, "/api/browse/pick-file") => {
            super::routes::files::handle_post_pick_file(bridge.clone()).await?
        }

        (&Method::POST, "/api/upload") => {
            super::routes::files::handle_post_upload(req, bridge.clone()).await?
        }

        // Tool execution
        (&Method::GET, "/api/tools/available") => {
            super::routes::tools::handle_get_available_tools(bridge.clone()).await?
        }
        (&Method::POST, "/api/tools/execute") => {
            super::routes::tools::handle_post_tools_execute(req, bridge.clone()).await?
        }

        // Web fetch (GET endpoint for easy curl access from model)
        (&Method::GET, "/api/tools/web-fetch") => {
            super::routes::tools::handle_get_web_fetch(req).await?
        }

        // File text extraction (for drag-and-drop attachments)
        (&Method::POST, "/api/file/extract-text") => {
            super::routes::tools::handle_post_extract_text(req).await?
        }

        // Serve persisted screenshot images
        (&Method::GET, path) if path.starts_with("/api/images/") => {
            let rel_path = &path["/api/images/".len()..];
            let file_path = std::path::PathBuf::from("assets/images").join(rel_path);
            if file_path.exists()
                && file_path
                    .extension()
                    .is_some_and(|e| e == "jpg" || e == "jpeg" || e == "png")
            {
                let bytes = std::fs::read(&file_path).unwrap_or_default();
                let content_type = if file_path.extension().is_some_and(|e| e == "png") {
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

        // Per-conversation agent heartbeat
        (&Method::GET, path)
            if path.starts_with("/api/conversations/") && path.ends_with("/heartbeat") =>
        {
            let id = &path["/api/conversations/".len()..path.len() - "/heartbeat".len()];
            super::routes::agent_heartbeat::handle_get_heartbeat(id, db.clone()).await?
        }
        (&Method::POST, path)
            if path.starts_with("/api/conversations/") && path.ends_with("/heartbeat") =>
        {
            let id = &path["/api/conversations/".len()..path.len() - "/heartbeat".len()];
            super::routes::agent_heartbeat::handle_post_heartbeat(req, id, db.clone()).await?
        }
        (&Method::POST, path)
            if path.starts_with("/api/conversations/") && path.ends_with("/heartbeat/fire") =>
        {
            let id = &path["/api/conversations/".len()..path.len() - "/heartbeat/fire".len()];
            super::routes::agent_heartbeat::handle_fire_heartbeat(id, pool.clone(), db.clone())
                .await?
        }
        (&Method::POST, path)
            if path.starts_with("/api/conversations/") && path.ends_with("/heartbeat/clear") =>
        {
            let id = &path["/api/conversations/".len()..path.len() - "/heartbeat/clear".len()];
            super::routes::agent_heartbeat::handle_clear_heartbeat(id, db.clone()).await?
        }

        // Remote access: LAN discovery, UPnP, token management
        (&Method::GET, "/api/remote/status") => {
            super::routes::remote::handle_get_status(db.clone()).await?
        }
        (&Method::POST, "/api/remote/upnp/enable") => {
            super::routes::remote::handle_upnp_enable(db.clone()).await?
        }
        (&Method::POST, "/api/remote/upnp/disable") => {
            super::routes::remote::handle_upnp_disable(req).await?
        }
        (&Method::POST, "/api/remote/token/regenerate") => {
            super::routes::remote::handle_regenerate_token(db.clone()).await?
        }

        // OpenAI-compatible server endpoints (for clients like openclaw)
        (&Method::GET, "/v1/models") => {
            super::routes::openai_compat_server::handle_get_models(bridge.clone()).await?
        }
        (&Method::POST, "/v1/chat/completions") => {
            super::routes::openai_compat_server::handle_post_chat_completions(req, bridge.clone())
                .await?
        }

        // CORS preflight
        (&Method::OPTIONS, _) => super::routes::static_files::handle_options(bridge.clone()).await?,

        // Static file serving
        (&Method::GET, "/") => super::routes::static_files::handle_index(bridge.clone()).await?,

        (&Method::GET, path)
            if path.starts_with("/assets/")
                || path.ends_with(".svg")
                || path.ends_with(".ico")
                || path.ends_with(".png") =>
        {
            super::routes::static_files::handle_static_asset(path, bridge.clone()).await?
        }

        // 404 Not Found
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not Found"))
            .unwrap(),
    };

    Ok(response)
}

/// Run the HTTP API server on `addr`, reusing an existing worker pool + database.
///
/// Used by the desktop app so the webview's `/api` fetches are served locally
/// against the desktop database. The standalone web binary uses its own
/// bootstrap (`server.rs`), so this is unused in that binary.
#[cfg(not(feature = "mock"))]
#[allow(dead_code)]
pub async fn serve(
    db: SharedDatabase,
    worker_pool: WorkerPool,
    addr: std::net::SocketAddr,
) -> std::io::Result<()> {
    use hyper::service::{make_service_fn, service_fn};
    use hyper::Server;

    let make_svc = make_service_fn(move |_conn| {
        let worker_pool = worker_pool.clone();
        let db = db.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                dispatch(req, Some(worker_pool.clone()), db.clone())
            }))
        }
    });

    Server::bind(&addr)
        .serve(make_svc)
        .await
        .map_err(std::io::Error::other)?;
    Ok(())
}
