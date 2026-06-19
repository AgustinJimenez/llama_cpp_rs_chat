//! Native WebView browser window for web mode (standalone server without Tauri).
//! Uses `wry` (same engine as Tauri) + `tao` for cross-platform WebView support:
//! Windows: WebView2, macOS: WKWebView, Linux: WebKitGTK.
//!
//! Multi-tab support: each `tab_id` gets its own Window + WebView, created on demand
//! the first time that tab is navigated and destroyed when it is closed.
//!
//! Threading model differs by platform:
//! - Windows/Linux: the event loop runs on a dedicated spawned thread (Windows opts into
//!   `with_any_thread(true)`); tab windows are created lazily inside the loop.
//! - macOS: AppKit requires the event loop on the MAIN thread, AND a WKWebView must be
//!   created AFTER the NSApplication run loop has started. So `main()` calls
//!   `serve_browser_on_main_thread()`, which publishes the proxy immediately and creates
//!   tab windows lazily inside the loop (on the first UserEvent for each tab).
//!
//! Tool threads communicate via channels (sync → event loop → sync).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::mpsc;

// ─── Command types ─────────────────────────────────────────────────────────

/// Commands sent from tool threads to the WebView event loop thread.
#[derive(Debug)]
pub enum WryCommand {
    Navigate {
        tab_id: String,
        url: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
    Evaluate {
        tab_id: String,
        id: String,
        js: String,
        reply: mpsc::Sender<Result<String, String>>,
    },
    Close {
        tab_id: String,
    },
}

impl WryCommand {
    fn tab_id(&self) -> &str {
        match self {
            WryCommand::Navigate { tab_id, .. } => tab_id,
            WryCommand::Evaluate { tab_id, .. } => tab_id,
            WryCommand::Close { tab_id, .. } => tab_id,
        }
    }
}

// ─── Per-tab state ─────────────────────────────────────────────────────────

/// Pending eval replies — keyed by UUID, fulfilled by the IPC handler.
type PendingReplies = Arc<Mutex<HashMap<String, mpsc::Sender<Result<String, String>>>>>;
/// Pending navigate reply — fulfilled when the page fires its load event via IPC.
type PendingNav = Arc<Mutex<Option<mpsc::Sender<Result<(), String>>>>>;

struct WryTab {
    window: tao::window::Window,
    webview: wry::WebView,
    pending: PendingReplies,
    pending_nav: PendingNav,
}

// ─── Global state ──────────────────────────────────────────────────────────

/// Non-macOS: event loop runs on a dedicated spawned thread.
#[cfg(not(target_os = "macos"))]
struct WryHandle {
    proxy: tao::event_loop::EventLoopProxy<WryCommand>,
}

#[cfg(not(target_os = "macos"))]
static WRY: Mutex<Option<WryHandle>> = Mutex::new(None);

/// Set to true if wry window creation failed (e.g., headless server). Don't retry.
static WRY_FAILED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// macOS: event loop runs on the main thread; proxy is published here once it starts.
#[cfg(target_os = "macos")]
static MACOS_PROXY: Mutex<Option<tao::event_loop::EventLoopProxy<WryCommand>>> =
    Mutex::new(None);
#[cfg(target_os = "macos")]
static MACOS_PROXY_CV: std::sync::Condvar = std::sync::Condvar::new();

// ─── Constants ────────────────────────────────────────────────────────────

/// Real-Chrome UA — removes the "Edg/" suffix WebView2 adds and avoids Google
/// fingerprinting this as an embedded WebView.
const WRY_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// Anti-automation fingerprint + page-load signal injected into every page.
const WRY_INIT_SCRIPT: &str = r#"
(function() {
    // 1. Hide navigator.webdriver
    try {
        Object.defineProperty(navigator, 'webdriver', { get: () => undefined, configurable: true });
    } catch(e) {}

    // 2. Make chrome.webview non-enumerable (hides from Object.keys scans).
    //    We cannot delete it — WebView2's postMessage validates its presence at call time,
    //    so removing it silently breaks all IPC (eval + nav_complete both stop working).
    //    window.chrome is configurable:false so we can't replace it with a Proxy either.
    //    window.ipc is Object.freeze()d so we can't rebuild it with a captured reference.
    try {
        if (window.chrome && window.chrome.webview) {
            const orig = window.chrome.webview;
            Object.defineProperty(window.chrome, 'webview', {
                value: orig, enumerable: false, configurable: true, writable: true
            });
        }
    } catch(e) {}

    // 3. Hide Edge-specific chrome properties (edgeMarketingPagePrivate, appPinningPrivate)
    //    that are not present in real Chrome and identify this as a WebView2/Edge context.
    try {
        ['edgeMarketingPagePrivate', 'appPinningPrivate'].forEach(function(k) {
            if (window.chrome && window.chrome[k] !== undefined) {
                Object.defineProperty(window.chrome, k, {
                    value: undefined, enumerable: false, configurable: true, writable: true
                });
            }
        });
    } catch(e) {}

    // 4. Make window.ipc non-enumerable
    try {
        if (window.ipc) {
            const orig = window.ipc;
            Object.defineProperty(window, 'ipc', { value: orig, enumerable: false, configurable: true, writable: true });
        }
    } catch(e) {}

    // 5. Signal page load — used by navigate() to know when the page is ready.
    //    Skip about:blank — its load event fires on window creation and would
    //    prematurely fulfill a pending navigate before the real page loads.
    window.addEventListener('load', function() {
        if (document.URL === 'about:blank') return;
        try { window.ipc.postMessage(JSON.stringify({id:'__nav_complete__',result:'ok'})); } catch(e) {}
    });
})();
"#;

// ─── WebView construction ─────────────────────────────────────────────────

/// Build the WebView for a window, wiring the persistent WebContext, UA, fingerprint
/// script, and IPC handler.
fn build_webview(
    window: &tao::window::Window,
    web_context: &mut wry::WebContext,
    pending: PendingReplies,
    pending_nav: PendingNav,
) -> Result<wry::WebView, String> {
    wry::WebViewBuilder::new_with_web_context(web_context)
        .with_url("about:blank")
        .with_user_agent(WRY_USER_AGENT)
        .with_initialization_script(WRY_INIT_SCRIPT)
        .with_ipc_handler(move |msg| {
            let body = msg.body();
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) {
                if let (Some(id), Some(result)) = (
                    parsed.get("id").and_then(|v| v.as_str()),
                    parsed.get("result"),
                ) {
                    if id == "__nav_complete__" {
                        if let Ok(mut nav) = pending_nav.lock() {
                            if let Some(tx) = nav.take() {
                                let _ = tx.send(Ok(()));
                            }
                        }
                        return;
                    }
                    let result_str = match result {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    if let Ok(mut map) = pending.lock() {
                        if let Some(tx) = map.remove(id) {
                            let _ = tx.send(Ok(result_str));
                        }
                    }
                }
            }
        })
        .build(window)
        .map_err(|e| format!("Failed to create WebView: {e}"))
}

/// Create a new tab (Window + WebView) for the given tab_id.
/// The window starts hidden and is revealed on the first Navigate.
fn create_tab(
    tab_id: &str,
    target: &tao::event_loop::EventLoopWindowTarget<WryCommand>,
    web_context: &mut wry::WebContext,
) -> Result<WryTab, String> {
    let title = if tab_id == "main" || tab_id == "default" || tab_id.is_empty() {
        "LLaMA Chat — Browser".to_string()
    } else {
        format!("LLaMA Chat — Browser [{tab_id}]")
    };

    let window = tao::window::WindowBuilder::new()
        .with_title(title)
        .with_inner_size(tao::dpi::LogicalSize::new(1280.0_f64, 900.0_f64))
        .with_visible(false)
        .build(target)
        .map_err(|e| format!("Failed to create window for tab '{tab_id}': {e}"))?;

    let pending: PendingReplies = Arc::new(Mutex::new(HashMap::new()));
    let pending_nav: PendingNav = Arc::new(Mutex::new(None));

    let webview = build_webview(&window, web_context, pending.clone(), pending_nav.clone())?;

    eprintln!("[WRY] Tab '{tab_id}' created");
    Ok(WryTab { window, webview, pending, pending_nav })
}

// ─── Command dispatch ─────────────────────────────────────────────────────

/// Dispatch a command to its tab. The tab must already exist in `tabs`.
fn handle_tab_command(cmd: WryCommand, tabs: &mut HashMap<String, WryTab>) {
    match cmd {
        WryCommand::Navigate { tab_id, url, reply } => {
            let Some(tab) = tabs.get(&tab_id) else {
                let _ = reply.send(Err(format!("wry: tab '{tab_id}' not found")));
                return;
            };
            eprintln!("[WRY] Tab '{tab_id}' navigate: {url}");
            #[cfg(target_os = "macos")]
            tab.window.set_visible(true);
            #[cfg(not(target_os = "macos"))]
            tab.window.set_visible(true);
            match tab.webview.load_url(&url) {
                Ok(()) => {
                    if let Ok(mut nav) = tab.pending_nav.lock() {
                        if let Some(old_tx) = nav.take() {
                            let _ = old_tx.send(Ok(()));
                        }
                        *nav = Some(reply);
                    }
                }
                Err(e) => {
                    let _ = reply.send(Err(format!("wry navigate: {e}")));
                }
            }
        }

        WryCommand::Evaluate { tab_id, id, js, reply } => {
            let Some(tab) = tabs.get(&tab_id) else {
                let _ = reply.send(Err(format!("wry: tab '{tab_id}' not found")));
                return;
            };
            if let Ok(mut map) = tab.pending.lock() {
                map.insert(id.clone(), reply.clone());
            }
            let escaped_id = id.replace('\\', "\\\\").replace('`', "\\`").replace("${", "\\${");
            let escaped_js = js.replace('\\', "\\\\").replace('`', "\\`").replace("${", "\\${");
            let wrapped = format!(
                "(function(){{try{{var __r=eval(`{escaped_js}`);window.ipc.postMessage(JSON.stringify({{id:`{escaped_id}`,result:typeof __r==='string'?__r:JSON.stringify(__r)}}))}}catch(__e){{window.ipc.postMessage(JSON.stringify({{id:`{escaped_id}`,result:'Error: '+__e.message}}))}}}})();"
            );
            if let Err(e) = tab.webview.evaluate_script(&wrapped) {
                if let Ok(mut map) = tab.pending.lock() {
                    if let Some(tx) = map.remove(&id) {
                        let _ = tx.send(Err(format!("wry eval inject: {e}")));
                    }
                }
            }
        }

        WryCommand::Close { tab_id } => {
            if tabs.remove(&tab_id).is_some() {
                eprintln!("[WRY] Tab '{tab_id}' closed and removed");
            }
        }
    }
}

// ─── Proxy acquisition ────────────────────────────────────────────────────

fn get_or_launch() -> Result<tao::event_loop::EventLoopProxy<WryCommand>, String> {
    if WRY_FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("wry: window creation previously failed (headless?)".into());
    }

    #[cfg(target_os = "macos")]
    {
        let slot = MACOS_PROXY.lock().map_err(|e| format!("wry lock: {e}"))?;
        if let Some(ref p) = *slot {
            return Ok(p.clone());
        }
        let (slot, res) = MACOS_PROXY_CV
            .wait_timeout_while(slot, std::time::Duration::from_secs(10), |s| s.is_none())
            .map_err(|e| format!("wry wait: {e}"))?;
        if res.timed_out() {
            return Err(
                "wry: main-thread event loop not running (serve_browser_on_main_thread not called?)"
                    .into(),
            );
        }
        return slot
            .as_ref()
            .map(Clone::clone)
            .ok_or_else(|| "wry: proxy unavailable".to_string());
    }

    #[cfg(not(target_os = "macos"))]
    {
        let mut guard = WRY.lock().map_err(|e| format!("wry lock: {e}"))?;
        if let Some(ref handle) = *guard {
            return Ok(handle.proxy.clone());
        }

        eprintln!("[WRY] Launching native WebView event loop...");
        let (ready_tx, ready_rx) = mpsc::channel();

        std::thread::spawn(move || {
            run_event_loop(ready_tx);
            eprintln!("[WRY] Event loop thread exited");
            if let Ok(mut g) = WRY.lock() {
                *g = None;
            }
        });

        match ready_rx.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok(Ok(proxy)) => {
                eprintln!("[WRY] Event loop launched successfully");
                *guard = Some(WryHandle { proxy: proxy.clone() });
                Ok(proxy)
            }
            Ok(Err(e)) => {
                eprintln!("[WRY] Event loop launch failed: {e}");
                WRY_FAILED.store(true, std::sync::atomic::Ordering::Relaxed);
                Err(e)
            }
            Err(_) => {
                eprintln!("[WRY] Timeout waiting for event loop");
                WRY_FAILED.store(true, std::sync::atomic::Ordering::Relaxed);
                Err("wry: timeout waiting for event loop".into())
            }
        }
    }
}

// ─── Event loops ─────────────────────────────────────────────────────────

/// Run the wry/tao event loop on the **main** thread, blocking forever (macOS only).
/// Publishes the proxy immediately; tab windows are created lazily on first use.
#[cfg(target_os = "macos")]
pub fn serve_browser_on_main_thread() -> ! {
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoopBuilder};

    eprintln!("[WRY] Running native WebView event loop on the main thread (macOS)");
    let event_loop = EventLoopBuilder::<WryCommand>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    if let Ok(mut slot) = MACOS_PROXY.lock() {
        *slot = Some(proxy);
    }
    MACOS_PROXY_CV.notify_all();

    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("llama-chat-browser");
    let mut web_context = wry::WebContext::new(Some(data_dir));
    let mut tabs: HashMap<String, WryTab> = HashMap::new();

    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id,
                ..
            } => {
                // Hide the tab whose window was closed rather than destroying it.
                // The model may navigate it again; destroying and recreating is expensive.
                for tab in tabs.values() {
                    if tab.window.id() == window_id {
                        tab.window.set_visible(false);
                        break;
                    }
                }
            }

            Event::UserEvent(cmd) => {
                let tab_id = cmd.tab_id().to_string();

                // Close: just remove the tab (drops Window + WebView).
                if matches!(cmd, WryCommand::Close { .. }) {
                    tabs.remove(&tab_id);
                    eprintln!("[WRY] Tab '{tab_id}' closed");
                    return;
                }

                // Create tab on demand if it doesn't exist yet.
                if !tabs.contains_key(&tab_id) {
                    match create_tab(&tab_id, target, &mut web_context) {
                        Ok(tab) => { tabs.insert(tab_id.clone(), tab); }
                        Err(e) => {
                            eprintln!("[WRY] Failed to create tab '{tab_id}': {e}");
                            match cmd {
                                WryCommand::Navigate { reply, .. } => { let _ = reply.send(Err(e)); }
                                WryCommand::Evaluate { reply, .. } => { let _ = reply.send(Err(e)); }
                                _ => {}
                            }
                            return;
                        }
                    }
                }

                handle_tab_command(cmd, &mut tabs);
            }

            _ => {}
        }
    });
}

/// Run the tao event loop with wry WebViews on a spawned thread (Windows/Linux).
/// Tab windows are created lazily inside the loop when first used.
#[cfg(not(target_os = "macos"))]
fn run_event_loop(
    ready_tx: mpsc::Sender<Result<tao::event_loop::EventLoopProxy<WryCommand>, String>>,
) {
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoopBuilder};

    let mut builder = EventLoopBuilder::<WryCommand>::with_user_event();
    #[cfg(windows)]
    {
        use tao::platform::windows::EventLoopBuilderExtWindows;
        builder.with_any_thread(true);
    }
    let event_loop = builder.build();
    let proxy = event_loop.create_proxy();

    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("llama-chat-browser");
    let mut web_context = wry::WebContext::new(Some(data_dir));
    let mut tabs: HashMap<String, WryTab> = HashMap::new();

    let _ = ready_tx.send(Ok(proxy));

    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id,
                ..
            } => {
                // Hide the tab whose window was closed by the user.
                for tab in tabs.values() {
                    if tab.window.id() == window_id {
                        tab.window.set_visible(false);
                        break;
                    }
                }
            }

            Event::UserEvent(cmd) => {
                let tab_id = cmd.tab_id().to_string();

                if matches!(cmd, WryCommand::Close { .. }) {
                    tabs.remove(&tab_id);
                    eprintln!("[WRY] Tab '{tab_id}' closed");
                    return;
                }

                if !tabs.contains_key(&tab_id) {
                    match create_tab(&tab_id, target, &mut web_context) {
                        Ok(tab) => { tabs.insert(tab_id.clone(), tab); }
                        Err(e) => {
                            eprintln!("[WRY] Failed to create tab '{tab_id}': {e}");
                            match cmd {
                                WryCommand::Navigate { reply, .. } => { let _ = reply.send(Err(e)); }
                                WryCommand::Evaluate { reply, .. } => { let _ = reply.send(Err(e)); }
                                _ => {}
                            }
                            return;
                        }
                    }
                }

                handle_tab_command(cmd, &mut tabs);
            }

            _ => {}
        }
    });
}

// ─── Public API (called from tool threads) ─────────────────────────────────

/// Navigate the specified tab to a URL.
pub fn navigate(url: &str, tab_id: &str) -> Result<(), String> {
    let proxy = get_or_launch()?;
    let (tx, rx) = mpsc::channel();
    proxy
        .send_event(WryCommand::Navigate {
            tab_id: tab_id.to_string(),
            url: url.to_string(),
            reply: tx,
        })
        .map_err(|_| "wry: event loop closed".to_string())?;

    match rx.recv_timeout(std::time::Duration::from_secs(30)) {
        Ok(result) => result,
        Err(_) => {
            eprintln!("[WRY] Tab '{tab_id}' navigate: no load event within 30s, continuing anyway");
            Ok(())
        }
    }
}

/// Evaluate JavaScript in the specified tab and return the result.
pub fn evaluate(js: &str, tab_id: &str) -> Result<String, String> {
    let proxy = get_or_launch()?;
    let id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = mpsc::channel();
    proxy
        .send_event(WryCommand::Evaluate {
            tab_id: tab_id.to_string(),
            id,
            js: js.to_string(),
            reply: tx,
        })
        .map_err(|_| "wry: event loop closed".to_string())?;

    rx.recv_timeout(std::time::Duration::from_secs(15))
        .map_err(|_| "wry eval: timeout (15s)".to_string())?
}

/// Close the specified tab — drops its Window + WebView.
pub fn close(tab_id: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(slot) = MACOS_PROXY.lock() {
            if let Some(ref proxy) = *slot {
                let _ = proxy.send_event(WryCommand::Close { tab_id: tab_id.to_string() });
            }
        }
        return Ok(());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let guard = WRY.lock().map_err(|e| format!("wry lock: {e}"))?;
        if let Some(ref handle) = *guard {
            let _ = handle.proxy.send_event(WryCommand::Close { tab_id: tab_id.to_string() });
        }
        Ok(())
    }
}
