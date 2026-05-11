//! Native WebView browser window for web mode (standalone server without Tauri).
//! Uses `wry` (same engine as Tauri) + `tao` for cross-platform WebView support:
//! Windows: WebView2, macOS: WKWebView, Linux: WebKitGTK.
//!
//! The WebView runs on a dedicated thread with its own event loop.
//! Tool threads communicate via channels (sync → event loop → sync).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc};

/// Commands sent from tool threads to the WebView event loop thread.
#[derive(Debug)]
pub enum WryCommand {
    Navigate {
        url: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
    Evaluate {
        id: String,
        js: String,
        reply: mpsc::Sender<Result<String, String>>,
    },
    Close,
}

/// Handle to the running WebView thread. Holds the proxy to send commands.
struct WryHandle {
    proxy: tao::event_loop::EventLoopProxy<WryCommand>,
}

static WRY: Mutex<Option<WryHandle>> = Mutex::new(None);
/// Set to true if wry window creation failed (e.g., headless server). Don't retry.
static WRY_FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Pending eval replies — keyed by UUID, fulfilled by the IPC handler.
type PendingReplies = Arc<Mutex<HashMap<String, mpsc::Sender<Result<String, String>>>>>;

/// Get or launch the wry WebView. Returns the event loop proxy for sending commands.
fn get_or_launch() -> Result<tao::event_loop::EventLoopProxy<WryCommand>, String> {
    if WRY_FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("wry: window creation previously failed (headless?)".into());
    }

    let mut guard = WRY.lock().map_err(|e| format!("wry lock: {e}"))?;
    if let Some(ref handle) = *guard {
        return Ok(handle.proxy.clone());
    }

    eprintln!("[WRY] Launching native WebView window...");
    let (ready_tx, ready_rx) = mpsc::channel();

    std::thread::spawn(move || {
        run_event_loop(ready_tx);
        eprintln!("[WRY] Event loop thread exited");
        // Clear singleton so next call can recreate
        if let Ok(mut guard) = WRY.lock() {
            *guard = None;
        }
    });

    // Wait for the event loop to be ready (or fail)
    match ready_rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(Ok(proxy)) => {
            eprintln!("[WRY] Native WebView launched successfully");
            *guard = Some(WryHandle { proxy: proxy.clone() });
            Ok(proxy)
        }
        Ok(Err(e)) => {
            eprintln!("[WRY] Window creation failed: {e}");
            WRY_FAILED.store(true, std::sync::atomic::Ordering::Relaxed);
            Err(e)
        }
        Err(_) => {
            eprintln!("[WRY] Timeout waiting for WebView to start");
            WRY_FAILED.store(true, std::sync::atomic::Ordering::Relaxed);
            Err("wry: timeout waiting for event loop".into())
        }
    }
}

/// Run the tao event loop with a wry WebView. Blocks the calling thread.
fn run_event_loop(
    ready_tx: mpsc::Sender<Result<tao::event_loop::EventLoopProxy<WryCommand>, String>>,
) {
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tao::window::WindowBuilder;

    let mut builder = EventLoopBuilder::<WryCommand>::with_user_event();
    // Allow running on non-main thread (Windows)
    #[cfg(windows)]
    {
        use tao::platform::windows::EventLoopBuilderExtWindows;
        builder.with_any_thread(true);
    }
    let event_loop = builder.build();
    let proxy = event_loop.create_proxy();

    let window = match WindowBuilder::new()
        .with_title("LLaMA Chat — Browser")
        .with_inner_size(tao::dpi::LogicalSize::new(1280.0, 900.0))
        .build(&event_loop)
    {
        Ok(w) => w,
        Err(e) => {
            let _ = ready_tx.send(Err(format!("Failed to create window: {e}")));
            return;
        }
    };

    // Pending eval replies — shared between IPC handler and event loop
    let pending: PendingReplies = Arc::new(Mutex::new(HashMap::new()));
    let pending_for_ipc = pending.clone();

    let webview = match wry::WebViewBuilder::new()
        .with_url("about:blank")
        .with_ipc_handler(move |msg| {
            // Messages from JS: {"id": "uuid", "result": "..."}
            let body = msg.body();
            eprintln!("[WRY_IPC] Received: {} bytes", body.len());
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) {
                if let (Some(id), Some(result)) = (
                    parsed.get("id").and_then(|v| v.as_str()),
                    parsed.get("result"),
                ) {
                    let result_str = match result {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    if let Ok(mut map) = pending_for_ipc.lock() {
                        if let Some(tx) = map.remove(id) {
                            let _ = tx.send(Ok(result_str));
                        }
                    }
                }
            }
        })
        .build(&window)
    {
        Ok(wv) => wv,
        Err(e) => {
            let _ = ready_tx.send(Err(format!("Failed to create WebView: {e}")));
            return;
        }
    };

    // Signal ready
    let _ = ready_tx.send(Ok(proxy));

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                eprintln!("[WRY] Window closed by user");
                *control_flow = ControlFlow::Exit;
            }

            Event::UserEvent(cmd) => match cmd {
                WryCommand::Navigate { url, reply } => {
                    eprintln!("[WRY] Navigate: {url}");
                    match webview.load_url(&url) {
                        Ok(()) => { let _ = reply.send(Ok(())); }
                        Err(e) => { let _ = reply.send(Err(format!("wry navigate: {e}"))); }
                    }
                }
                WryCommand::Evaluate { id, js, reply } => {
                    // Register pending reply
                    if let Ok(mut map) = pending.lock() {
                        map.insert(id.clone(), reply.clone());
                    }
                    // Wrap JS to send result via IPC.
                    // Use backtick template to avoid escaping issues with the user JS.
                    let escaped_id = id.replace('\\', "\\\\").replace('`', "\\`").replace("${", "\\${");
                    let escaped_js = js.replace('\\', "\\\\").replace('`', "\\`").replace("${", "\\${");
                    let wrapped = format!(
                        "(function(){{try{{var __r=eval(`{escaped_js}`);window.ipc.postMessage(JSON.stringify({{id:`{escaped_id}`,result:typeof __r==='string'?__r:JSON.stringify(__r)}}))}}catch(__e){{window.ipc.postMessage(JSON.stringify({{id:`{escaped_id}`,result:'Error: '+__e.message}}))}}}})();"
                    );
                    if let Err(e) = webview.evaluate_script(&wrapped) {
                        // Script injection failed — reply immediately
                        if let Ok(mut map) = pending.lock() {
                            if let Some(tx) = map.remove(&id) {
                                let _ = tx.send(Err(format!("wry eval inject: {e}")));
                            }
                        }
                    }
                }
                WryCommand::Close => {
                    eprintln!("[WRY] Close requested");
                    *control_flow = ControlFlow::Exit;
                }
            },

            _ => {}
        }
    });
}

// ─── Public API (called from tool threads) ─────────────────────

/// Navigate the wry WebView to a URL.
pub fn navigate(url: &str) -> Result<(), String> {
    let proxy = get_or_launch()?;
    let (tx, rx) = mpsc::channel();
    proxy.send_event(WryCommand::Navigate {
        url: url.to_string(),
        reply: tx,
    }).map_err(|_| "wry: event loop closed".to_string())?;

    rx.recv_timeout(std::time::Duration::from_secs(15))
        .map_err(|_| "wry navigate: timeout".to_string())?
}

/// Evaluate JavaScript in the wry WebView and return the result.
pub fn evaluate(js: &str) -> Result<String, String> {
    let proxy = get_or_launch()?;
    let id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = mpsc::channel();
    proxy.send_event(WryCommand::Evaluate {
        id,
        js: js.to_string(),
        reply: tx,
    }).map_err(|_| "wry: event loop closed".to_string())?;

    rx.recv_timeout(std::time::Duration::from_secs(15))
        .map_err(|_| "wry eval: timeout (15s)".to_string())?
}

/// Close the wry WebView window.
pub fn close() -> Result<(), String> {
    let mut guard = WRY.lock().map_err(|e| format!("wry lock: {e}"))?;
    if let Some(handle) = guard.take() {
        let _ = handle.proxy.send_event(WryCommand::Close);
    }
    Ok(())
}
