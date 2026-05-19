//! Embedded MCP server for UI testing — runs inside the Tauri app process.
//!
//! Exposes tools that let Claude Code interact with the app's UI directly
//! via WebView JS injection and Tauri state access. No screenshots, no VRAM,
//! no separate process — always available when the app is running.
//!
//! Starts on http://localhost:18091/mcp (HTTP/SSE transport).
//!
//! # Module layout
//! - [`eval`]           — JS evaluation in WebView2 via raw COM vtable
//! - [`cdp`]            — Chrome DevTools Protocol helpers + screenshot capture
//! - [`browser_tools`]  — `browser_*` tool handlers + REST bridge endpoints
//! - [`app_tools`]      — `app_*` tool handlers
//! - [`tools`]          — Tool schema definitions (`build_tools`)

mod app_tools;
mod browser_tools;
mod cdp;
mod eval;
mod tools;

use std::sync::Arc;

use axum::{body::Bytes, extract::State, routing::post};
use rmcp::{
    ServerHandler,
    model::{
        CallToolRequestParams, CallToolResult, Content, Implementation, ListToolsResult,
        ServerCapabilities, ServerInfo, Tool,
    },
    service::RequestContext,
    service::RoleServer,
};
use serde_json::{Value, json};
use tauri::AppHandle;
use tauri::Manager;

use crate::web::worker::worker_bridge::SharedWorkerBridge;
use browser_tools::{bridge_browser_close, bridge_browser_navigate};
use cdp::capture_webview_screenshot;
use eval::eval_js_in;
use tools::build_tools;

/// Default port for the embedded MCP server.
const DEFAULT_PORT: u16 = 18091;

// ─── Server struct ────────────────────────────────────────────────

struct AppUiServer {
    app: AppHandle,
    tools: Vec<Tool>,
}

impl AppUiServer {
    fn new(app: AppHandle) -> Self {
        Self { app, tools: build_tools() }
    }
}

// ─── Tool dispatch ────────────────────────────────────────────────

async fn dispatch_tool(server: &AppUiServer, name: &str, args: &Value) -> Result<String, String> {
    // Try app tools first, then browser tools
    if let Some(result) = app_tools::dispatch_app_tool(&server.app, name, args).await {
        return result;
    }
    if let Some(result) = browser_tools::dispatch_browser_tool(&server.app, name, args).await {
        return result;
    }
    Err(format!("Unknown tool: {name}"))
}

// ─── ServerHandler impl ───────────────────────────────────────────

impl ServerHandler for AppUiServer {
    fn get_info(&self) -> ServerInfo {
        let capabilities = ServerCapabilities::builder()
            .enable_tools()
            .build();
        ServerInfo::new(capabilities)
            .with_server_info(Implementation::new(
                "app-ui",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "LLaMA Chat app UI server. Use these tools to interact with the app's \
                 UI elements, load models, send messages, and read responses. All tools \
                 operate on the live app — no screenshots needed."
            )
    }

    fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, rmcp::ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult {
            tools: self.tools.clone(),
            next_cursor: None,
            meta: None,
        }))
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, rmcp::ErrorData>> + Send + '_ {
        async move {
            let name = &request.name;
            let args = request.arguments.as_ref()
                .map(|m| Value::Object(m.clone()))
                .unwrap_or(json!({}));
            match dispatch_tool(self, name, &args).await {
                Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
                Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
            }
        }
    }
}

// ─── Plain REST endpoints (bypasses MCP protocol, fast) ──────────

async fn rest_get_state(State(app): State<AppHandle>) -> axum::response::Response {
    let bridge: SharedWorkerBridge = app.state::<SharedWorkerBridge>().inner().clone();
    let meta = bridge.model_status().await;
    let generating = bridge.is_generating().await;
    let loading = bridge.is_loading();
    let body = serde_json::json!({
        "model_loaded": meta.is_some(),
        "model_path": meta.as_ref().map(|m| &m.model_path),
        "generating": generating,
        "loading": loading,
    });
    axum::response::Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .header("access-control-allow-origin", "*")
        .body(axum::body::Body::from(body.to_string()))
        .unwrap()
}

async fn rest_load_model(State(app): State<AppHandle>, body: Bytes) -> axum::response::Response {
    let path = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("path").and_then(|p| p.as_str()).map(|s| s.to_string()));
    let msg = match path {
        Some(p) => {
            let bridge: SharedWorkerBridge = app.state::<SharedWorkerBridge>().inner().clone();
            match bridge.load_model(&p, None, None).await {
                Ok(_) => format!("{{\"ok\":true,\"model\":\"{p}\"}}"),
                Err(e) => format!("{{\"ok\":false,\"error\":\"{e}\"}}"),
            }
        }
        None => "{\"ok\":false,\"error\":\"path required\"}".into(),
    };
    axum::response::Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .header("access-control-allow-origin", "*")
        .body(axum::body::Body::from(msg))
        .unwrap()
}

async fn rest_eval(State(app): State<AppHandle>, body: Bytes) -> axum::response::Response {
    let data = serde_json::from_slice::<serde_json::Value>(&body).ok();
    let js = data.as_ref()
        .and_then(|v| v.get("js").and_then(|j| j.as_str()).map(|s| s.to_string()));
    let target = data.as_ref()
        .and_then(|v| v.get("target").and_then(|t| t.as_str()))
        .unwrap_or("main");
    let result = match js {
        Some(code) => eval_js_in(&app, &code, target).await.unwrap_or_else(|e| e),
        None => "\"js required\"".into(),
    };
    axum::response::Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .header("access-control-allow-origin", "*")
        .body(axum::body::Body::from(result))
        .unwrap()
}

async fn rest_screenshot(State(app): State<AppHandle>) -> axum::response::Response {
    let result = eval_js_in(&app, "document.body.innerText.slice(0, 30000)", "main")
        .await
        .unwrap_or_else(|e| format!("\"error: {e}\""));
    axum::response::Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .header("access-control-allow-origin", "*")
        .body(axum::body::Body::from(result))
        .unwrap()
}

async fn rest_browser_click(State(app): State<AppHandle>, body: Bytes) -> axum::response::Response {
    let data = serde_json::from_slice::<Value>(&body).ok();
    let selector = data.as_ref().and_then(|v| v.get("selector").and_then(|s| s.as_str()));
    let target = data.as_ref()
        .and_then(|v| v.get("target").and_then(|t| t.as_str()))
        .unwrap_or("agent-browser");
    let result = match selector {
        Some(sel) => cdp::cdp_click(&app, target, sel)
            .await
            .unwrap_or_else(|e| e),
        None => "\"selector required\"".into(),
    };
    axum::response::Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .header("access-control-allow-origin", "*")
        .body(axum::body::Body::from(format!(
            "\"{}\"",
            result.replace('"', "\\\"")
        )))
        .unwrap()
}

async fn rest_browser_screenshot(State(app): State<AppHandle>) -> axum::response::Response {
    // Try browser-panel first (user-visible), fall back to agent-browser
    use tauri::Manager;
    let target = if app.webviews().get("browser-panel").is_some() {
        "browser-panel"
    } else {
        "agent-browser"
    };
    match capture_webview_screenshot(&app, target).await {
        Ok(path) => match std::fs::read(&path) {
            Ok(bytes) => axum::response::Response::builder()
                .status(200)
                .header("content-type", "image/png")
                .header("access-control-allow-origin", "*")
                .body(axum::body::Body::from(bytes))
                .unwrap(),
            Err(e) => axum::response::Response::builder()
                .status(500)
                .body(axum::body::Body::from(format!("Read failed: {e}")))
                .unwrap(),
        },
        Err(e) => axum::response::Response::builder()
            .status(500)
            .header("content-type", "text/plain")
            .body(axum::body::Body::from(e))
            .unwrap(),
    }
}

// ─── Server startup ───────────────────────────────────────────────

/// Start the embedded MCP HTTP server. Call from Tauri's `.setup()`.
pub async fn start(app: AppHandle, port: u16) {
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService,
        session::local::LocalSessionManager,
    };
    use tokio_util::sync::CancellationToken;

    let ct = CancellationToken::new();

    let mut config = StreamableHttpServerConfig::default();
    config.stateful_mode = true;
    config.sse_keep_alive = Some(std::time::Duration::from_secs(15));
    config.sse_retry = Some(std::time::Duration::from_secs(3));
    config.json_response = false;
    config.cancellation_token = ct.child_token();

    let session_manager = Arc::new(LocalSessionManager::default());
    let app_for_factory = app.clone();

    let service = StreamableHttpService::new(
        move || Ok(AppUiServer::new(app_for_factory.clone())),
        session_manager,
        config,
    );

    // Return 404 for OAuth discovery — tells Claude Code "no auth needed".
    async fn oauth_not_found() -> axum::response::Response {
        axum::response::Response::builder()
            .status(404)
            .body(axum::body::Body::from("Not Found"))
            .unwrap()
    }

    let router = axum::Router::new()
        // Plain REST (fast, no MCP overhead)
        .route("/api/state", axum::routing::get(rest_get_state))
        .route("/api/load-model", post(rest_load_model))
        .route("/api/eval", post(rest_eval))
        .route("/api/screenshot", axum::routing::get(rest_screenshot))
        .route("/api/browser/screenshot", axum::routing::get(rest_browser_screenshot))
        .route("/api/browser/click", post(rest_browser_click))
        // Bridge endpoints
        .route("/bridge/browser/navigate", post(bridge_browser_navigate))
        .route("/bridge/browser/close", post(bridge_browser_close))
        .route(
            "/.well-known/oauth-authorization-server",
            axum::routing::get(oauth_not_found),
        )
        .with_state(app.clone())
        .nest_service("/mcp", service);

    let addr = format!("127.0.0.1:{port}");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[MCP_UI] Failed to bind to {addr}: {e}");
            return;
        }
    };

    eprintln!("[MCP_UI] App UI MCP server listening on http://{addr}/mcp");

    if let Err(e) = axum::serve(listener, router)
        .with_graceful_shutdown(async move { ct.cancelled().await })
        .await
    {
        eprintln!("[MCP_UI] HTTP server error: {e}");
    }
}

/// Start the MCP server on the default port.
pub async fn start_default(app: AppHandle) {
    start(app, DEFAULT_PORT).await;
}
