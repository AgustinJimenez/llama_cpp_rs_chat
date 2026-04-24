//! Embedded MCP server for UI testing — runs inside the Tauri app process.
//!
//! Exposes tools that let Claude Code interact with the app's UI directly
//! via WebView JS injection and Tauri state access. No screenshots, no VRAM,
//! no separate process — always available when the app is running.
//!
//! Starts on http://localhost:18091/mcp (HTTP/SSE transport).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

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
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tokio::sync::{Mutex, oneshot};

use crate::web::worker::worker_bridge::SharedWorkerBridge;

/// Default port for the embedded MCP server.
const DEFAULT_PORT: u16 = 18091;

/// Timeout for JS eval results (ms).
const EVAL_TIMEOUT_MS: u64 = 10_000;

/// Pending JS evaluation results — shared between MCP server and __mcp_result command.
pub type McpPendingResults = Arc<Mutex<HashMap<u64, oneshot::Sender<String>>>>;

/// Counter for unique eval request IDs.
static EVAL_ID: AtomicU64 = AtomicU64::new(1);

// ─── Server struct ────────────────────────────────────────────────

struct AppUiServer {
    app: AppHandle,
    tools: Vec<Tool>,
}

impl AppUiServer {
    fn new(app: AppHandle) -> Self {
        Self {
            app,
            tools: build_tools(),
        }
    }

    /// Execute JavaScript in the main webview and return the result string.
    /// Uses the __mcp_result IPC callback pattern since eval() is fire-and-forget.
    async fn eval_js(&self, js: &str) -> Result<String, String> {
        let webview = self.app
            .get_webview_window("main")
            .ok_or("Main webview not found")?;

        let pending: McpPendingResults = self.app
            .try_state::<McpPendingResults>()
            .ok_or("McpPendingResults state not registered")?
            .inner()
            .clone();

        let id = EVAL_ID.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();

        // Register pending result
        {
            let mut map = pending.lock().await;
            map.insert(id, tx);
        }

        // Inject JS that evaluates the code and POSTs the result back via HTTP.
        // This bypasses Tauri IPC (which breaks after model generation) and uses
        // fetch() which always works in any webview state.
        let wrapper = format!(
            r#"(async () => {{
                let __val;
                try {{
                    __val = await (async () => {{ return {js} }})();
                    __val = JSON.stringify(__val ?? null);
                }} catch (e) {{
                    __val = JSON.stringify({{ __error: e.message }});
                }}
                try {{
                    await fetch('http://127.0.0.1:{port}/bridge/eval-result', {{
                        method: 'POST',
                        headers: {{ 'Content-Type': 'application/json' }},
                        body: JSON.stringify({{ id: {id}, value: __val }}),
                    }});
                }} catch (_) {{}}
            }})()"#,
            port = DEFAULT_PORT,
        );

        webview.eval(&wrapper).map_err(|e| format!("eval failed: {e}"))?;

        // Wait for result with timeout
        match tokio::time::timeout(
            std::time::Duration::from_millis(EVAL_TIMEOUT_MS),
            rx,
        ).await {
            Ok(Ok(value)) => {
                // Check for error
                if let Ok(parsed) = serde_json::from_str::<Value>(&value) {
                    if let Some(err) = parsed.get("__error").and_then(|e| e.as_str()) {
                        return Err(format!("JS error: {err}"));
                    }
                }
                Ok(value)
            }
            Ok(Err(_)) => Err("Result channel closed".into()),
            Err(_) => {
                // Clean up pending entry
                let mut map = pending.lock().await;
                map.remove(&id);
                Err("JS eval timed out (10s)".into())
            }
        }
    }
}

async fn open_browser_view_js(app: &AppHandle, url: &str) -> Result<String, String> {
    let server = AppUiServer::new(app.clone());
    let js = format!(
        r#"(() => {{
            if (window.__openBrowserView) {{
                window.__openBrowserView({url});
                return 'Browser view opened: ' + {url};
            }}
            return 'openBrowserView not available';
        }})()"#,
        url = serde_json::to_string(url).unwrap()
    );
    server.eval_js(&js).await
}

async fn close_browser_view_js(app: &AppHandle) -> Result<String, String> {
    let server = AppUiServer::new(app.clone());
    let js = r#"(() => {
        if (window.__closeBrowserView) {
            window.__closeBrowserView();
            return 'Browser view closed';
        }
        return 'closeBrowserView not available';
    })()"#;
    server.eval_js(js).await
}

async fn bridge_browser_navigate(
    State(app): State<AppHandle>,
    body: Bytes,
) -> Result<String, axum::http::StatusCode> {
    let body: Value = serde_json::from_slice(&body)
        .map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;
    let url = body.get("url")
        .and_then(|v| v.as_str())
        .ok_or(axum::http::StatusCode::BAD_REQUEST)?;
    open_browser_view_js(&app, url)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
}

async fn bridge_browser_close(
    State(app): State<AppHandle>,
) -> Result<String, axum::http::StatusCode> {
    close_browser_view_js(&app)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
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

            let result = dispatch_tool(self, name, &args).await;

            match result {
                Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
                Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
            }
        }
    }
}

// ─── Tool dispatch ────────────────────────────────────────────────

async fn dispatch_tool(server: &AppUiServer, name: &str, args: &Value) -> Result<String, String> {
    match name {
        "app_click" => {
            let sel = args.get("selector").and_then(|v| v.as_str())
                .ok_or("'selector' is required")?;
            let js = format!(
                r#"(() => {{
                    const el = document.querySelector({sel});
                    if (!el) return 'Element not found: ' + {sel};
                    el.click();
                    return 'Clicked ' + el.tagName + ': ' + (el.textContent || '').trim().slice(0, 50);
                }})()"#,
                sel = serde_json::to_string(sel).unwrap()
            );
            server.eval_js(&js).await
        }

        "app_type" => {
            let sel = args.get("selector").and_then(|v| v.as_str())
                .ok_or("'selector' is required")?;
            let text = args.get("text").and_then(|v| v.as_str())
                .ok_or("'text' is required")?;
            let submit = args.get("submit").and_then(|v| v.as_bool()).unwrap_or(false);
            let js = format!(
                r#"(() => {{
                    const el = document.querySelector({sel});
                    if (!el) return 'Element not found: ' + {sel};
                    const setter = Object.getOwnPropertyDescriptor(
                        el.tagName === 'TEXTAREA' ? window.HTMLTextAreaElement.prototype : window.HTMLInputElement.prototype,
                        'value'
                    )?.set;
                    if (setter) {{ setter.call(el, {text}); }}
                    else {{ el.value = {text}; }}
                    el.dispatchEvent(new Event('input', {{ bubbles: true }}));
                    el.dispatchEvent(new Event('change', {{ bubbles: true }}));
                    {submit_js}
                    return 'Typed into ' + {sel};
                }})()"#,
                sel = serde_json::to_string(sel).unwrap(),
                text = serde_json::to_string(text).unwrap(),
                submit_js = if submit {
                    r#"
                    const form = el.closest('form');
                    if (form) form.requestSubmit();
                    else {
                        const btn = document.querySelector('button[type="submit"]');
                        if (btn) setTimeout(() => btn.click(), 100);
                    }
                    "#
                } else { "" }
            );
            server.eval_js(&js).await
        }

        "app_read" => {
            let sel = args.get("selector").and_then(|v| v.as_str())
                .ok_or("'selector' is required")?;
            let js = format!(
                r#"(() => {{
                    const el = document.querySelector({sel});
                    if (!el) return 'Element not found: ' + {sel};
                    return (el.textContent || el.value || '').trim().slice(0, 50000);
                }})()"#,
                sel = serde_json::to_string(sel).unwrap()
            );
            server.eval_js(&js).await
        }

        "app_list_elements" => {
            let filter = args.get("filter").and_then(|v| v.as_str()).unwrap_or("");
            let js = format!(
                r#"(() => {{
                    const sels = 'button, input, textarea, select, a, [role=button], [data-testid]';
                    const els = Array.from(document.querySelectorAll(sels));
                    const filter = {filter}.toLowerCase();
                    return JSON.stringify(els
                        .map((el, i) => ({{
                            index: i,
                            tag: el.tagName.toLowerCase(),
                            type: el.type || null,
                            text: (el.textContent || el.placeholder || el.title || el.ariaLabel || '').trim().slice(0, 80),
                            selector: el.id ? '#' + el.id : (el.dataset.testid ? '[data-testid="' + el.dataset.testid + '"]' : null),
                            visible: el.offsetParent !== null || el.offsetHeight > 0,
                        }}))
                        .filter(e => e.visible && (!filter || e.text.toLowerCase().includes(filter) || (e.selector && e.selector.toLowerCase().includes(filter))))
                        .slice(0, 100)
                    );
                }})()"#,
                filter = serde_json::to_string(filter).unwrap()
            );
            server.eval_js(&js).await
        }

        "app_eval" => {
            let js = args.get("js").and_then(|v| v.as_str())
                .ok_or("'js' is required")?;
            // Wrap in return so expressions work
            let wrapped = format!("return ({js})");
            server.eval_js(&wrapped).await
        }

        "app_get_state" => {
            let bridge: SharedWorkerBridge = server.app
                .try_state::<SharedWorkerBridge>()
                .ok_or("WorkerBridge not available")?
                .inner()
                .clone();

            let meta = bridge.model_status().await;
            let generating = bridge.is_generating().await;
            let loading = bridge.is_loading();

            let state = json!({
                "model_loaded": meta.is_some(),
                "model_path": meta.as_ref().map(|m| &m.model_path),
                "generating": generating,
                "loading": loading,
            });
            Ok(state.to_string())
        }

        "app_load_model" => {
            let path = args.get("path").and_then(|v| v.as_str())
                .ok_or("'path' is required")?;

            let bridge: SharedWorkerBridge = server.app
                .try_state::<SharedWorkerBridge>()
                .ok_or("WorkerBridge not available")?
                .inner()
                .clone();

            match bridge.load_model(path, None, None).await {
                Ok(_) => Ok(format!("Model loaded: {path}")),
                Err(e) => Err(format!("Load failed: {e}")),
            }
        }

        "app_send_message" => {
            let text = args.get("text").and_then(|v| v.as_str())
                .ok_or("'text' is required")?;
            let js = format!(
                r#"(() => {{
                    const ta = document.querySelector('textarea[placeholder="Ask anything"]');
                    if (!ta) return 'Message input not found';
                    const setter = Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, 'value')?.set;
                    if (setter) setter.call(ta, {text});
                    else ta.value = {text};
                    ta.dispatchEvent(new Event('input', {{ bubbles: true }}));
                    setTimeout(() => {{
                        const btn = document.querySelector('button[type="submit"], button[aria-label="Send message"]');
                        if (btn) btn.click();
                    }}, 200);
                    return 'Message sent: ' + {text}.slice(0, 50);
                }})()"#,
                text = serde_json::to_string(text).unwrap()
            );
            server.eval_js(&js).await
        }

        "app_wait_for" => {
            let sel = args.get("selector").and_then(|v| v.as_str())
                .ok_or("'selector' is required")?;
            let timeout = args.get("timeout_ms").and_then(|v| v.as_u64()).unwrap_or(10_000);
            let js = format!(
                r#"new Promise((resolve) => {{
                    const t0 = Date.now();
                    const check = () => {{
                        if (document.querySelector({sel})) return resolve('Found: ' + {sel});
                        if (Date.now() - t0 >= {timeout}) return resolve('Timeout waiting for: ' + {sel});
                        setTimeout(check, 200);
                    }};
                    check();
                }})"#,
                sel = serde_json::to_string(sel).unwrap(),
                timeout = timeout,
            );
            server.eval_js(&js).await
        }

        "app_navigate_browser" => {
            let url = args.get("url").and_then(|v| v.as_str())
                .ok_or("'url' is required")?;
            open_browser_view_js(&server.app, url).await
        }

        "app_screenshot" => {
            // Return DOM structure as text (no image dependency)
            let js = r#"(() => {
                const body = document.body;
                if (!body) return 'No body element';
                return body.innerText.slice(0, 30000);
            })()"#;
            server.eval_js(js).await
        }

        _ => Err(format!("Unknown tool: {name}")),
    }
}

// ─── Tool definitions ─────────────────────────────────────────────

fn build_tools() -> Vec<Tool> {
    let defs: Vec<(&str, &str, Value)> = vec![
        ("app_click", "Click a UI element by CSS selector.", json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector (e.g. 'button.submit', '#login')" }
            },
            "required": ["selector"]
        })),
        ("app_type", "Type text into an input or textarea by CSS selector. Set submit=true to auto-click the send/submit button after.", json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector of the input" },
                "text": { "type": "string", "description": "Text to type" },
                "submit": { "type": "boolean", "description": "Click submit after typing (default: false)" }
            },
            "required": ["selector", "text"]
        })),
        ("app_read", "Read text content of a UI element by CSS selector.", json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector to read from" }
            },
            "required": ["selector"]
        })),
        ("app_list_elements", "List all interactive UI elements (buttons, inputs, links). Returns tag, text, and selector for each. Use filter to narrow results.", json!({
            "type": "object",
            "properties": {
                "filter": { "type": "string", "description": "Optional text filter to match element text or selector" }
            }
        })),
        ("app_eval", "Execute arbitrary JavaScript in the app's webview and return the result. Use for anything the other tools can't do.", json!({
            "type": "object",
            "properties": {
                "js": { "type": "string", "description": "JavaScript expression or statement to evaluate" }
            },
            "required": ["js"]
        })),
        ("app_get_state", "Get current app state: model loaded/path, generating status, loading status. No arguments needed.", json!({
            "type": "object", "properties": {}
        })),
        ("app_load_model", "Load a GGUF model by file path. Uses the app's worker bridge directly.", json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Full path to the .gguf model file" }
            },
            "required": ["path"]
        })),
        ("app_send_message", "Type a message into the chat input and send it. The model will start generating a response.", json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "Message text to send" }
            },
            "required": ["text"]
        })),
        ("app_wait_for", "Wait for a CSS selector to appear on the page (e.g. after navigation or generation). Returns when found or on timeout.", json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector to wait for" },
                "timeout_ms": { "type": "integer", "description": "Max wait time in ms (default: 10000)" }
            },
            "required": ["selector"]
        })),
        ("app_navigate_browser", "Open a URL in the app's browser view panel.", json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "URL to navigate to" }
            },
            "required": ["url"]
        })),
        ("app_screenshot", "Get the visible text content of the entire app page. Returns innerText of the body (no image).", json!({
            "type": "object", "properties": {}
        })),
    ];

    defs.into_iter()
        .filter_map(|(name, desc, schema)| {
            let map = match schema {
                Value::Object(m) => m,
                _ => return None,
            };
            Some(Tool::new(name, desc, Arc::new(map)))
        })
        .collect()
}

// ─── Server startup ───────────────────────────────────────────────

/// Start the embedded MCP HTTP server. Call from Tauri's .setup().
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
    // If we return 200 with any JSON, Claude Code tries to complete OAuth flow.
    // A 404 means "no OAuth server" = no auth required.
    async fn oauth_not_found() -> axum::response::Response {
        axum::response::Response::builder()
            .status(404)
            .body(axum::body::Body::from("Not Found"))
            .unwrap()
    }

    // Eval result handler — receives JS eval results via fetch from the webview.
    // Must include CORS headers since the webview origin differs from the MCP server.
    async fn bridge_eval_result(
        State(app): State<AppHandle>,
        body: Bytes,
    ) -> axum::response::Response {
        if let Ok(data) = serde_json::from_slice::<serde_json::Value>(&body) {
            let id = data.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let value = data.get("value").and_then(|v| v.as_str()).unwrap_or("null").to_string();
            if let Some(pending) = app.try_state::<McpPendingResults>() {
                let mut map = pending.lock().await;
                if let Some(tx) = map.remove(&id) {
                    let _ = tx.send(value);
                }
            }
        }
        axum::response::Response::builder()
            .status(200)
            .header("access-control-allow-origin", "*")
            .header("access-control-allow-methods", "POST, OPTIONS")
            .header("access-control-allow-headers", "content-type")
            .body(axum::body::Body::from("ok"))
            .unwrap()
    }

    // CORS preflight for eval-result
    async fn bridge_eval_result_options() -> axum::response::Response {
        axum::response::Response::builder()
            .status(204)
            .header("access-control-allow-origin", "*")
            .header("access-control-allow-methods", "POST, OPTIONS")
            .header("access-control-allow-headers", "content-type")
            .body(axum::body::Body::empty())
            .unwrap()
    }

    let router = axum::Router::new()
        .route("/bridge/browser/navigate", post(bridge_browser_navigate))
        .route("/bridge/browser/close", post(bridge_browser_close))
        .route("/bridge/eval-result", post(bridge_eval_result).options(bridge_eval_result_options))
        .route("/.well-known/oauth-authorization-server", axum::routing::get(oauth_not_found))
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

// ─── Tauri command for JS eval results ────────────────────────────

/// Receives evaluation results from injected JavaScript.
/// Registered as a Tauri command: `__mcp_result`.
#[tauri::command]
pub async fn __mcp_result(
    id: u64,
    value: String,
    results: tauri::State<'_, McpPendingResults>,
) -> Result<(), String> {
    let mut map = results.lock().await;
    if let Some(tx) = map.remove(&id) {
        let _ = tx.send(value);
    }
    Ok(())
}
