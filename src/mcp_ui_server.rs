//! Embedded MCP server for UI testing — runs inside the Tauri app process.
//!
//! Exposes tools that let Claude Code interact with the app's UI directly
//! via WebView JS injection and Tauri state access. No screenshots, no VRAM,
//! no separate process — always available when the app is running.
//!
//! Starts on http://localhost:18091/mcp (HTTP/SSE transport).

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
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tokio::sync::oneshot;

use crate::web::worker::worker_bridge::SharedWorkerBridge;

/// Default port for the embedded MCP server.
const DEFAULT_PORT: u16 = 18091;

/// Timeout for JS eval results (ms).
const EVAL_TIMEOUT_MS: u64 = 10_000;

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

    /// Execute JavaScript in a webview and return the result.
    ///
    /// Uses WebView2's `ExecuteScript` COM API directly (via `with_webview`),
    /// which returns JS eval results through a COM callback — bypasses CSP,
    /// CORS, and mixed-content restrictions that plagued the old HTTP callback approach.
    async fn eval_js_in(&self, js: &str, target: &str) -> Result<String, String> {
        let (tx, rx) = oneshot::channel::<String>();

        // Wrap user JS to always return a JSON string.
        // IMPORTANT: Must be synchronous — WebView2 ExecuteScript does NOT
        // await Promises (returns `{}` for Promise objects).
        //
        // Handles 4 cases:
        // 1. Arrow function `() => {...}` → call it as IIFE
        // 2. Multi-statement with `return` → wrap in IIFE
        // 3. Multi-statement (const/let/var) → wrap in IIFE, return last expression
        // 4. Simple expression → use directly
        let trimmed = js.trim().trim_end_matches(';').trim();
        let is_multistatement = trimmed.starts_with("const ")
            || trimmed.starts_with("let ")
            || trimmed.starts_with("var ")
            || trimmed.contains(";\n")
            || trimmed.contains("; ");
        let is_iife = trimmed.starts_with('(')
            && (trimmed.ends_with(")()")  || trimmed.ends_with(")()\n"));
        let eval_expr = if trimmed.starts_with("() =>")
            || trimmed.starts_with("function(")
            || trimmed.starts_with("function (")
        {
            // Case 1: function definition — call it
            format!("({trimmed})()")
        } else if is_iife {
            // Case 1b: already an IIFE — use directly
            trimmed.to_string()
        } else if js.contains("return ") {
            // Case 2: has explicit return — wrap in IIFE
            format!("(function() {{ {js} }})()")
        } else if is_multistatement {
            // Case 3: multi-statement without return — wrap in IIFE.
            // Split on last `;`, add `return` before the final expression.
            // e.g. "const x = 1; x" → "(function() { const x = 1; return x; })()"
            let parts: Vec<&str> = trimmed.rsplitn(2, ';').collect();
            if parts.len() == 2 {
                let last_expr = parts[0].trim();
                let prefix = parts[1];
                if last_expr.is_empty() {
                    format!("(function() {{ {prefix}; }})()")
                } else {
                    format!("(function() {{ {prefix}; return ({last_expr}); }})()")
                }
            } else {
                format!("(function() {{ return ({trimmed}); }})()")
            }
        } else {
            // Case 4: simple expression
            js.to_string()
        };
        let wrapped_js = format!(
            r#"(function() {{
                try {{
                    var __val = {eval_expr};
                    return JSON.stringify(__val ?? null);
                }} catch (e) {{
                    return JSON.stringify({{ __error: e.message }});
                }}
            }})()"#,
        );

        // Find the target webview
        let webviews = self.app.webviews();
        let webview = if target == "browser-panel" || target == "agent-browser" {
            webviews.get(target)
                .ok_or("Browser panel not open. Use browser_navigate first.")?
                .clone()
        } else if let Some(wv) = self.app.get_webview_window("main") {
            wv.as_ref().clone()
        } else if let Some(wv) = webviews.values().next() {
            wv.clone()
        } else {
            return Err("No webview available".into());
        };

        // Use WebView2 ExecuteScript directly — returns result via COM callback.
        // Bypasses CSP/CORS since results come through the COM API, not HTTP.
        //
        // We call ExecuteScript through the raw COM vtable because webview2-com
        // depends on windows-core 0.61 while we depend on windows 0.62, making
        // the PCWSTR types incompatible at Rust's type level (same ABI though).
        #[cfg(windows)]
        {
            let js_for_closure = wrapped_js.clone();
            webview.with_webview(move |platform_wv| {
                let controller = platform_wv.controller();
                let core_wv = unsafe { controller.CoreWebView2() };

                match core_wv {
                    Ok(core) => {
                        let handler = webview2_com::ExecuteScriptCompletedHandler::create(
                            Box::new(move |_hr, result| {
                                let _ = tx.send(result);
                                Ok(())
                            }),
                        );
                        // Encode JS as null-terminated UTF-16
                        let wide: Vec<u16> = js_for_closure
                            .encode_utf16()
                            .chain(std::iter::once(0))
                            .collect();
                        // Call ExecuteScript via raw COM vtable to avoid
                        // PCWSTR version conflicts between windows 0.61/0.62.
                        // Vtable layout: IUnknown(3) + ICoreWebView2 methods.
                        // Index 29 = ExecuteScript (verified from ICoreWebView2_Vtbl).
                        unsafe {
                            let this: *mut std::ffi::c_void = std::mem::transmute_copy(&core);
                            let vtable = *(this as *const *const usize);
                            type ExecuteScriptFn = unsafe extern "system" fn(
                                this: *mut std::ffi::c_void,
                                js: *const u16,
                                handler: *mut std::ffi::c_void,
                            ) -> i32;
                            let func: ExecuteScriptFn =
                                std::mem::transmute(*vtable.add(29));
                            let handler_ptr: *mut std::ffi::c_void =
                                std::mem::transmute_copy(&handler);
                            func(this, wide.as_ptr(), handler_ptr);
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(format!(
                            r#"{{"__error":"CoreWebView2 unavailable: {e}"}}"#
                        ));
                    }
                }
            }).map_err(|e| format!("with_webview failed: {e}"))?;
        }

        // Non-Windows: fall back to old eval (fire-and-forget, won't return values)
        #[cfg(not(windows))]
        {
            webview.eval(&wrapped_js).map_err(|e| format!("eval failed: {e}"))?;
            let _ = tx.send(r#""eval sent (no return value on this platform)""#.to_string());
        }

        // Wait for the result with timeout
        match tokio::time::timeout(
            std::time::Duration::from_millis(EVAL_TIMEOUT_MS),
            rx,
        ).await {
            Ok(Ok(value)) => {
                // WebView2 returns JSON-encoded strings (extra quotes), unwrap them
                let cleaned = if value.starts_with('"') && value.ends_with('"') {
                    // Parse the outer JSON string encoding added by WebView2
                    serde_json::from_str::<String>(&value).unwrap_or(value)
                } else {
                    value
                };
                // Check for JS errors
                if let Ok(parsed) = serde_json::from_str::<Value>(&cleaned) {
                    if let Some(err) = parsed.get("__error").and_then(|e| e.as_str()) {
                        return Err(format!("JS error: {err}"));
                    }
                }
                Ok(cleaned)
            }
            Ok(Err(_)) => Err("Result channel closed".into()),
            Err(_) => Err(format!("JS eval timed out ({EVAL_TIMEOUT_MS}ms)")),
        }
    }

    /// Execute JavaScript in the main webview.
    async fn eval_js(&self, js: &str) -> Result<String, String> {
        self.eval_js_in(js, "main").await
    }

    /// Execute JavaScript in the browser panel webview.
    #[allow(dead_code)]
    async fn eval_browser_panel(&self, js: &str) -> Result<String, String> {
        self.eval_js_in(js, "agent-browser").await
    }
}

/// Call a Chrome DevTools Protocol method on a webview.
/// Uses WebView2's CallDevToolsProtocolMethod COM API (vtable index 36).
#[allow(unused_variables)]
async fn cdp_call(app: &AppHandle, target: &str, method: &str, params: &Value) -> Result<String, String> {
    let (tx, rx) = oneshot::channel::<String>();

    let webviews = app.webviews();
    let webview = webviews.get(target).cloned()
        .ok_or_else(|| format!("Webview '{target}' not open"))?;

    #[cfg(windows)]
    {
        let method_str = method.to_string();
        let params_str = params.to_string();

        webview.with_webview(move |platform_wv| {
            let controller = platform_wv.controller();
            let core_wv = unsafe { controller.CoreWebView2() };

            match core_wv {
                Ok(core) => {
                    let handler = webview2_com::CallDevToolsProtocolMethodCompletedHandler::create(
                        Box::new(move |_hr, result| {
                            let _ = tx.send(result);
                            Ok(())
                        }),
                    );
                    let method_wide: Vec<u16> = method_str.encode_utf16()
                        .chain(std::iter::once(0)).collect();
                    let params_wide: Vec<u16> = params_str.encode_utf16()
                        .chain(std::iter::once(0)).collect();

                    // CallDevToolsProtocolMethod vtable index = 36
                    unsafe {
                        let this: *mut std::ffi::c_void = std::mem::transmute_copy(&core);
                        let vtable = *(this as *const *const usize);
                        type CdpFn = unsafe extern "system" fn(
                            this: *mut std::ffi::c_void,
                            method: *const u16,
                            params: *const u16,
                            handler: *mut std::ffi::c_void,
                        ) -> i32;
                        let func: CdpFn = std::mem::transmute(*vtable.add(36));
                        let handler_ptr: *mut std::ffi::c_void = std::mem::transmute_copy(&handler);
                        func(this, method_wide.as_ptr(), params_wide.as_ptr(), handler_ptr);
                    }
                }
                Err(e) => {
                    let _ = tx.send(format!(r#"{{"error":"CoreWebView2 unavailable: {e}"}}"#));
                }
            }
        }).map_err(|e| format!("with_webview failed: {e}"))?;
    }

    #[cfg(not(windows))]
    {
        let _ = tx.send(r#"{"error":"CDP not available on this platform"}"#.to_string());
    }

    match tokio::time::timeout(std::time::Duration::from_secs(10), rx).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(_)) => Err("CDP channel closed".into()),
        Err(_) => Err("CDP call timed out (10s)".into()),
    }
}

/// Click an element using CDP Input.dispatchMouseEvent.
/// First finds element bounds via JS eval, then sends real mouse events via CDP.
async fn cdp_click(app: &AppHandle, target: &str, selector: &str) -> Result<String, String> {
    let server = AppUiServer::new(app.clone());
    // Step 1: Find element bounds via JS
    let js = format!(
        r#"(() => {{
            const el = document.querySelector({sel});
            if (!el) return {{error: 'Element not found: ' + {sel}}};
            const rect = el.getBoundingClientRect();
            return {{
                x: Math.round(rect.left + rect.width / 2),
                y: Math.round(rect.top + rect.height / 2),
                text: (el.textContent || '').trim().slice(0, 50),
                tag: el.tagName
            }};
        }})()"#,
        sel = serde_json::to_string(selector).unwrap()
    );
    let bounds_str = server.eval_js_in(&js, target).await?;
    // eval_js_in wraps in JSON.stringify, so bounds_str is already valid JSON
    let bounds: Value = serde_json::from_str(&bounds_str)
        .map_err(|e| format!("Failed to parse bounds: {e} — raw: {bounds_str}"))?;

    if let Some(err) = bounds.get("error").and_then(|e| e.as_str()) {
        return Err(err.to_string());
    }

    let x = bounds["x"].as_f64().ok_or("Missing x coordinate")?;
    let y = bounds["y"].as_f64().ok_or("Missing y coordinate")?;
    let text = bounds["text"].as_str().unwrap_or("");
    let tag = bounds["tag"].as_str().unwrap_or("?");

    // Step 2: Send CDP mouse events (mousePressed + mouseReleased = full click)
    let press_params = json!({
        "type": "mousePressed",
        "x": x,
        "y": y,
        "button": "left",
        "clickCount": 1
    });
    cdp_call(app, target, "Input.dispatchMouseEvent", &press_params).await?;

    // Small delay between press and release
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let release_params = json!({
        "type": "mouseReleased",
        "x": x,
        "y": y,
        "button": "left",
        "clickCount": 1
    });
    cdp_call(app, target, "Input.dispatchMouseEvent", &release_params).await?;

    Ok(format!("CDP clicked {tag}: {text} at ({x}, {y})"))
}

/// Capture a screenshot of a webview and save as PNG.
/// Uses WebView2's CapturePreview COM API via raw vtable.
#[allow(unused_variables)]
async fn capture_webview_screenshot(app: &AppHandle, target: &str) -> Result<String, String> {
    let webviews = app.webviews();
    let webview = webviews.get(target).cloned()
        .ok_or_else(|| format!("Webview '{target}' not open. Use browser_navigate first."))?;

    let screenshot_dir = app.path().app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("screenshots");
    let _ = std::fs::create_dir_all(&screenshot_dir);
    let filename = format!("browser_{}.png", chrono::Utc::now().format("%Y%m%d_%H%M%S"));
    let filepath = screenshot_dir.join(&filename);
    let filepath_str = filepath.to_string_lossy().to_string();

    #[cfg(windows)]
    {
        let (tx, rx) = oneshot::channel::<Result<Vec<u8>, String>>();
        let filepath_clone = filepath.clone();

        webview.with_webview(move |platform_wv| {
            let controller = platform_wv.controller();
            let core_wv = unsafe { controller.CoreWebView2() };

            match core_wv {
                Ok(core) => {
                    // Create an IStream in memory to receive the PNG
                    let stream = unsafe {
                        windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal(
                            windows::Win32::Foundation::HGLOBAL::default(),
                            true,
                        )
                    };
                    match stream {
                        Ok(stream) => {
                            let stream_clone = stream.clone();
                            let handler = webview2_com::CapturePreviewCompletedHandler::create(
                                Box::new(move |hr| {
                                    if hr.is_err() {
                                        let _ = tx.send(Err(format!("CapturePreview failed: {hr:?}")));
                                        return Ok(());
                                    }
                                    // Read PNG bytes from stream
                                    use windows::Win32::System::Com::STREAM_SEEK_SET;
                                    unsafe {
                                        let _ = stream_clone.Seek(0, STREAM_SEEK_SET, None);
                                        // Read all bytes
                                        let mut buf = vec![0u8; 16 * 1024 * 1024]; // 16MB max
                                        let mut bytes_read = 0u32;
                                        let _ = stream_clone.Read(
                                            buf.as_mut_ptr() as *mut _,
                                            buf.len() as u32,
                                            Some(&mut bytes_read),
                                        );
                                        buf.truncate(bytes_read as usize);
                                        let _ = tx.send(Ok(buf));
                                    }
                                    Ok(())
                                }),
                            );
                            // CapturePreview vtable index:
                            // ICoreWebView2 methods after IUnknown(3):
                            // Index 37 = CapturePreview (COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT, IStream, handler)
                            unsafe {
                                let this: *mut std::ffi::c_void = std::mem::transmute_copy(&core);
                                let vtable = *(this as *const *const usize);
                                type CapturePreviewFn = unsafe extern "system" fn(
                                    this: *mut std::ffi::c_void,
                                    image_format: i32, // 0 = PNG
                                    stream: *mut std::ffi::c_void,
                                    handler: *mut std::ffi::c_void,
                                ) -> i32;
                                let func: CapturePreviewFn =
                                    std::mem::transmute(*vtable.add(30));
                                let stream_ptr: *mut std::ffi::c_void =
                                    std::mem::transmute_copy(&stream);
                                let handler_ptr: *mut std::ffi::c_void =
                                    std::mem::transmute_copy(&handler);
                                func(this, 0, stream_ptr, handler_ptr); // 0 = PNG format
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(format!("CreateStreamOnHGlobal failed: {e}")));
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("CoreWebView2 unavailable: {e}")));
                }
            }
        }).map_err(|e| format!("with_webview failed: {e}"))?;

        match tokio::time::timeout(std::time::Duration::from_secs(10), rx).await {
            Ok(Ok(Ok(png_bytes))) => {
                if png_bytes.is_empty() {
                    return Err("Screenshot capture returned empty data".into());
                }
                std::fs::write(&filepath, &png_bytes)
                    .map_err(|e| format!("Failed to save screenshot: {e}"))?;
                Ok(filepath_str)
            }
            Ok(Ok(Err(e))) => Err(e),
            Ok(Err(_)) => Err("Screenshot channel closed".into()),
            Err(_) => Err("Screenshot timed out (10s)".into()),
        }
    }

    #[cfg(not(windows))]
    Err("Screenshot not available on this platform".into())
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

    // Create/navigate the hidden agent webview (separate from user's browser-panel).
    // Uses label "agent-browser" to avoid conflicting with the user's browser panel.
    let parsed = url.parse::<tauri::Url>()
        .map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;
    let webview_count = app.webviews().len();
    eprintln!("[MCP_BROWSER] navigate to {url}, webviews: {webview_count}");
    if let Some(existing) = app.webviews().get("agent-browser").cloned() {
        eprintln!("[MCP_BROWSER] reusing existing agent-browser webview");
        let _ = existing.navigate(parsed);
    } else if let Some(window) = app.get_window("main") {
        let builder = tauri::webview::WebviewBuilder::new(
            "agent-browser",
            tauri::WebviewUrl::External(parsed),
        );
        // Hidden child webview — zero height so it's invisible, but
        // still functional for COM ExecuteScript calls.
        match window.add_child(
            builder,
            tauri::LogicalPosition::new(0.0, 0.0),
            tauri::LogicalSize::new(800.0, 0.0),
        ) {
            Ok(wv) => eprintln!("[MCP_BROWSER] created child webview: {:?}", wv.label()),
            Err(e) => eprintln!("[MCP_BROWSER] add_child FAILED: {e}"),
        }
    }

    Ok(format!("Browser view opened: {url}"))
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

        // ─── Browser panel tools (operate on the agent-browser webview) ───
        //
        // These use the "agent-browser" hidden webview created by the bridge.
        // Unlike the user-visible "browser-panel", this one is always available
        // and doesn't interfere with the UI.

        "browser_navigate" => {
            let url = args.get("url").and_then(|v| v.as_str())
                .ok_or("'url' is required")?;
            // Create/navigate the hidden agent-browser webview
            let full_url = if url.starts_with("http://") || url.starts_with("https://") {
                url.to_string()
            } else {
                format!("https://{url}")
            };
            let parsed = full_url.parse::<tauri::Url>()
                .map_err(|e| format!("Invalid URL: {e}"))?;
            if let Some(existing) = server.app.webviews().get("agent-browser").cloned() {
                existing.navigate(parsed).map_err(|e| format!("Navigate failed: {e}"))?;
            } else if let Some(window) = server.app.get_window("main") {
                let builder = tauri::webview::WebviewBuilder::new(
                    "agent-browser",
                    tauri::WebviewUrl::External(parsed),
                );
                window.add_child(
                    builder,
                    tauri::LogicalPosition::new(0.0, 0.0),
                    tauri::LogicalSize::new(800.0, 0.0),
                ).map_err(|e| format!("Failed to create browser: {e}"))?;
            } else {
                return Err("Main window not found".into());
            }
            // Wait briefly for page to start loading
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            Ok(format!("Navigated to {full_url}"))
        }

        "browser_read" => {
            let selector = args.get("selector").and_then(|v| v.as_str());
            let max_len = args.get("max_length").and_then(|v| v.as_u64()).unwrap_or(30000);
            let js = if let Some(sel) = selector {
                format!(
                    r#"(() => {{
                        const el = document.querySelector({sel});
                        if (!el) return 'Element not found: ' + {sel};
                        return (el.innerText || el.textContent || '').trim().slice(0, {max_len});
                    }})()"#,
                    sel = serde_json::to_string(sel).unwrap(),
                    max_len = max_len,
                )
            } else {
                format!(
                    r#"(() => {{
                        const body = document.body;
                        if (!body) return 'No body element';
                        return body.innerText.slice(0, {max_len});
                    }})()"#,
                    max_len = max_len,
                )
            };
            server.eval_js_in(&js, "agent-browser").await
        }

        "browser_click" => {
            let sel = args.get("selector").and_then(|v| v.as_str())
                .ok_or("'selector' is required")?;
            // Use CDP Input.dispatchMouseEvent for real clicks (works on React SPAs)
            cdp_click(&server.app, "agent-browser", sel).await
        }

        "browser_type" => {
            let sel = args.get("selector").and_then(|v| v.as_str())
                .ok_or("'selector' is required")?;
            let text = args.get("text").and_then(|v| v.as_str())
                .ok_or("'text' is required")?;
            let submit = args.get("submit").and_then(|v| v.as_bool()).unwrap_or(false);
            let js = format!(
                r#"(() => {{
                    const el = document.querySelector({sel});
                    if (!el) return 'Element not found: ' + {sel};
                    el.focus();
                    el.value = {text};
                    el.dispatchEvent(new Event('input', {{ bubbles: true }}));
                    el.dispatchEvent(new Event('change', {{ bubbles: true }}));
                    {submit_js}
                    return 'Typed into ' + {sel};
                }})()"#,
                sel = serde_json::to_string(sel).unwrap(),
                text = serde_json::to_string(text).unwrap(),
                submit_js = if submit {
                    r#"const form = el.closest('form');
                    if (form) form.requestSubmit();
                    else {
                        const btn = document.querySelector('button[type="submit"], input[type="submit"]');
                        if (btn) setTimeout(() => btn.click(), 100);
                    }"#
                } else { "" }
            );
            server.eval_js_in(&js, "agent-browser").await
        }

        "browser_eval" => {
            let js = args.get("js").and_then(|v| v.as_str())
                .ok_or("'js' is required")?;
            server.eval_js_in(js, "agent-browser").await
        }

        "browser_get_url" => {
            // Get URL from the agent-browser webview directly
            let webviews = server.app.webviews();
            if let Some(wv) = webviews.get("agent-browser").cloned() {
                let url = wv.url().map(|u| u.to_string()).unwrap_or_default();
                Ok(url)
            } else {
                Err("Browser panel not open. Use browser_navigate first.".into())
            }
        }

        "browser_list_links" => {
            let filter = args.get("filter").and_then(|v| v.as_str()).unwrap_or("");
            let js = format!(
                r#"(() => {{
                    const links = Array.from(document.querySelectorAll('a[href]'));
                    const filter = {filter}.toLowerCase();
                    return JSON.stringify(links
                        .map(a => ({{
                            text: (a.textContent || '').trim().slice(0, 80),
                            href: a.href,
                        }}))
                        .filter(l => l.text && (!filter || l.text.toLowerCase().includes(filter) || l.href.toLowerCase().includes(filter)))
                        .slice(0, 100)
                    );
                }})()"#,
                filter = serde_json::to_string(filter).unwrap()
            );
            server.eval_js_in(&js, "agent-browser").await
        }

        "browser_list_elements" => {
            let filter = args.get("filter").and_then(|v| v.as_str()).unwrap_or("");
            let js = format!(
                r#"(() => {{
                    const sels = 'button, input, textarea, select, a, [role=button], [role=link], [role=tab]';
                    const els = Array.from(document.querySelectorAll(sels));
                    const filter = {filter}.toLowerCase();
                    return JSON.stringify(els
                        .map((el, i) => ({{
                            index: i,
                            tag: el.tagName.toLowerCase(),
                            type: el.type || null,
                            text: (el.textContent || el.placeholder || el.title || el.ariaLabel || '').trim().slice(0, 80),
                            href: el.href || null,
                            selector: el.id ? '#' + el.id : (el.className ? el.tagName.toLowerCase() + '.' + el.className.trim().split(/\s+/).join('.') : null),
                            visible: el.offsetParent !== null || el.offsetHeight > 0,
                        }}))
                        .filter(e => e.visible && (!filter || e.text.toLowerCase().includes(filter) || (e.selector && e.selector.toLowerCase().includes(filter))))
                        .slice(0, 100)
                    );
                }})()"#,
                filter = serde_json::to_string(filter).unwrap()
            );
            server.eval_js_in(&js, "agent-browser").await
        }

        "browser_scroll" => {
            let direction = args.get("direction").and_then(|v| v.as_str()).unwrap_or("down");
            let amount = args.get("amount").and_then(|v| v.as_u64()).unwrap_or(500);
            let js = format!(
                r#"(() => {{
                    const dy = {dir} === 'up' ? -{amount} : {amount};
                    window.scrollBy(0, dy);
                    return 'Scrolled ' + {dir} + ' by ' + Math.abs(dy) + 'px. Page at y=' + window.scrollY + '/' + document.body.scrollHeight;
                }})()"#,
                dir = serde_json::to_string(direction).unwrap(),
                amount = amount,
            );
            server.eval_js_in(&js, "agent-browser").await
        }

        "browser_screenshot" => {
            capture_webview_screenshot(&server.app, "agent-browser").await
        }

        "browser_close" => {
            close_browser_view_js(&server.app).await
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
        // ─── Browser panel tools ───
        ("browser_navigate", "Open a URL in the browser panel (user-visible embedded browser). Opens the browser panel if not already open.", json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "URL to navigate to" }
            },
            "required": ["url"]
        })),
        ("browser_read", "Read text content from the browser panel page. Optionally scope to a CSS selector.", json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector to read from (default: entire page)" },
                "max_length": { "type": "integer", "description": "Max characters to return (default: 30000)" }
            }
        })),
        ("browser_click", "Click an element in the browser panel by CSS selector.", json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector of the element to click" }
            },
            "required": ["selector"]
        })),
        ("browser_type", "Type text into an input field in the browser panel.", json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector of the input" },
                "text": { "type": "string", "description": "Text to type" },
                "submit": { "type": "boolean", "description": "Submit the form after typing (default: false)" }
            },
            "required": ["selector", "text"]
        })),
        ("browser_eval", "Execute JavaScript in the browser panel webview. Returns the result.", json!({
            "type": "object",
            "properties": {
                "js": { "type": "string", "description": "JavaScript to evaluate in the browser panel" }
            },
            "required": ["js"]
        })),
        ("browser_get_url", "Get the current URL of the browser panel.", json!({
            "type": "object", "properties": {}
        })),
        ("browser_list_links", "List all links on the browser panel page. Optionally filter by text or URL.", json!({
            "type": "object",
            "properties": {
                "filter": { "type": "string", "description": "Filter links by text or URL substring" }
            }
        })),
        ("browser_list_elements", "List interactive elements (buttons, inputs, links) in the browser panel.", json!({
            "type": "object",
            "properties": {
                "filter": { "type": "string", "description": "Filter by element text or selector" }
            }
        })),
        ("browser_scroll", "Scroll the browser panel page up or down.", json!({
            "type": "object",
            "properties": {
                "direction": { "type": "string", "description": "Scroll direction: 'up' or 'down' (default: 'down')" },
                "amount": { "type": "integer", "description": "Scroll amount in pixels (default: 500)" }
            }
        })),
        ("browser_screenshot", "Take a screenshot of the browser panel. Returns the file path of the saved PNG.", json!({
            "type": "object", "properties": {}
        })),
        ("browser_close", "Close the browser panel.", json!({
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

    // ─── Plain REST API (bypasses MCP protocol, fast) ───
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
        let js = data.as_ref().and_then(|v| v.get("js").and_then(|j| j.as_str()).map(|s| s.to_string()));
        let target = data.as_ref().and_then(|v| v.get("target").and_then(|t| t.as_str())).unwrap_or("main");
        let server = AppUiServer::new(app);
        let result = match js {
            Some(code) => server.eval_js_in(&code, target).await.unwrap_or_else(|e| e),
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
        let server = AppUiServer::new(app);
        let result = server.eval_js("document.body.innerText.slice(0, 30000)").await
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
        let target = data.as_ref().and_then(|v| v.get("target").and_then(|t| t.as_str())).unwrap_or("agent-browser");
        let result = match selector {
            Some(sel) => cdp_click(&app, target, sel).await.unwrap_or_else(|e| e),
            None => "\"selector required\"".into(),
        };
        axum::response::Response::builder()
            .status(200)
            .header("content-type", "application/json")
            .header("access-control-allow-origin", "*")
            .body(axum::body::Body::from(format!("\"{}\"", result.replace('"', "\\\""))))
            .unwrap()
    }

    async fn rest_browser_screenshot(State(app): State<AppHandle>) -> axum::response::Response {
        // Try browser-panel first (user-visible), fall back to agent-browser
        let target = if app.webviews().get("browser-panel").is_some() {
            "browser-panel"
        } else {
            "agent-browser"
        };
        match capture_webview_screenshot(&app, target).await {
            Ok(path) => {
                // Return the PNG file directly
                match std::fs::read(&path) {
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
                }
            }
            Err(e) => axum::response::Response::builder()
                .status(500)
                .header("content-type", "text/plain")
                .body(axum::body::Body::from(e))
                .unwrap(),
        }
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

