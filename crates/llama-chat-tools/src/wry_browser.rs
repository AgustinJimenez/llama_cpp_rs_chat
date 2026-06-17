//! Native WebView browser window for web mode (standalone server without Tauri).
//! Uses `wry` (same engine as Tauri) + `tao` for cross-platform WebView support:
//! Windows: WebView2, macOS: WKWebView, Linux: WebKitGTK.
//!
//! Threading model differs by platform:
//! - Windows/Linux: the event loop runs on a dedicated spawned thread (Windows opts into
//!   `with_any_thread(true)`); the window+webview are created eagerly before `run()`.
//! - macOS: AppKit requires the event loop on the MAIN thread, AND a WKWebView must be
//!   created AFTER the NSApplication run loop has started. So `main()` calls
//!   `serve_browser_on_main_thread()`, which publishes the proxy immediately and creates
//!   the window+webview lazily inside the loop (on the first `Resumed`/`Init` event).
//!
//! Tool threads communicate via channels (sync → event loop → sync).

use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};

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
/// Non-macOS only — on macOS the loop runs on the main thread and publishes via MACOS_PROXY.
#[cfg(not(target_os = "macos"))]
struct WryHandle {
    proxy: tao::event_loop::EventLoopProxy<WryCommand>,
}

#[cfg(not(target_os = "macos"))]
static WRY: Mutex<Option<WryHandle>> = Mutex::new(None);
/// Set to true if wry window creation failed (e.g., headless server). Don't retry.
static WRY_FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// macOS requires the windowing event loop on the MAIN thread (AppKit/NSApplication).
/// On macOS the loop is launched from `main()` via `serve_browser_on_main_thread()` and
/// publishes its proxy here; `get_or_launch()` waits on it instead of spawning a thread.
#[cfg(target_os = "macos")]
static MACOS_PROXY: Mutex<Option<tao::event_loop::EventLoopProxy<WryCommand>>> = Mutex::new(None);
#[cfg(target_os = "macos")]
static MACOS_PROXY_CV: Condvar = Condvar::new();

/// Pending eval replies — keyed by UUID, fulfilled by the IPC handler.
type PendingReplies = Arc<Mutex<HashMap<String, mpsc::Sender<Result<String, String>>>>>;
/// Pending navigate reply — fulfilled when the page fires its load event via IPC.
type PendingNav = Arc<Mutex<Option<mpsc::Sender<Result<(), String>>>>>;

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

/// Build the WebView for a window, wiring the persistent WebContext, UA, fingerprint
/// script, and IPC handler. Shared by the eager (Windows/Linux) and lazy (macOS) loops.
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
            // Messages from JS: {"id": "uuid", "result": "..."}
            let body = msg.body();
            eprintln!("[WRY_IPC] Received: {} bytes", body.len());
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) {
                if let (Some(id), Some(result)) = (
                    parsed.get("id").and_then(|v| v.as_str()),
                    parsed.get("result"),
                ) {
                    // Reserved: page load complete signal from init script
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

/// Handle a command on the live webview. Returns `true` if the loop should exit.
fn handle_wry_command(
    cmd: WryCommand,
    webview: &wry::WebView,
    window: &tao::window::Window,
    pending: &PendingReplies,
    pending_nav: &PendingNav,
) -> bool {
    // `window` is only used on macOS (show/hide); silence the unused warning elsewhere.
    #[cfg(not(target_os = "macos"))]
    let _ = window;

    match cmd {
        WryCommand::Navigate { url, reply } => {
            eprintln!("[WRY] Navigate: {url}");
            // macOS: window starts hidden / may have been hidden on close — reveal it.
            #[cfg(target_os = "macos")]
            window.set_visible(true);
            match webview.load_url(&url) {
                Ok(()) => {
                    // Don't reply yet — wait for the page 'load' event via IPC.
                    // Fulfill any stale pending nav first (e.g. redirects).
                    if let Ok(mut nav) = pending_nav.lock() {
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
            false
        }
        WryCommand::Evaluate { id, js, reply } => {
            if let Ok(mut map) = pending.lock() {
                map.insert(id.clone(), reply.clone());
            }
            // Wrap JS to send the result via IPC. Backtick template avoids escaping issues.
            let escaped_id = id.replace('\\', "\\\\").replace('`', "\\`").replace("${", "\\${");
            let escaped_js = js.replace('\\', "\\\\").replace('`', "\\`").replace("${", "\\${");
            let wrapped = format!(
                "(function(){{try{{var __r=eval(`{escaped_js}`);window.ipc.postMessage(JSON.stringify({{id:`{escaped_id}`,result:typeof __r==='string'?__r:JSON.stringify(__r)}}))}}catch(__e){{window.ipc.postMessage(JSON.stringify({{id:`{escaped_id}`,result:'Error: '+__e.message}}))}}}})();"
            );
            if let Err(e) = webview.evaluate_script(&wrapped) {
                if let Ok(mut map) = pending.lock() {
                    if let Some(tx) = map.remove(&id) {
                        let _ = tx.send(Err(format!("wry eval inject: {e}")));
                    }
                }
            }
            false
        }
        WryCommand::Close => {
            eprintln!("[WRY] Close requested");
            // macOS: the loop owns the main thread for the process lifetime — just hide.
            // Other platforms exit (the loop is recreated on the next browse).
            #[cfg(target_os = "macos")]
            {
                window.set_visible(false);
                false
            }
            #[cfg(not(target_os = "macos"))]
            {
                true
            }
        }
    }
}

/// Reply to a command that arrived before the webview was ready (macOS startup race).
#[cfg(target_os = "macos")]
fn reply_webview_not_ready(cmd: WryCommand) {
    match cmd {
        WryCommand::Navigate { reply, .. } => {
            let _ = reply.send(Err("wry: webview not ready yet".into()));
        }
        WryCommand::Evaluate { reply, .. } => {
            let _ = reply.send(Err("wry: webview not ready yet".into()));
        }
        WryCommand::Close => {}
    }
}

/// Get or launch the wry WebView. Returns the event loop proxy for sending commands.
fn get_or_launch() -> Result<tao::event_loop::EventLoopProxy<WryCommand>, String> {
    if WRY_FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("wry: window creation previously failed (headless?)".into());
    }

    // macOS: AppKit requires the event loop on the main thread, so it's launched from
    // `main()` via `serve_browser_on_main_thread()`. Here we just wait for that loop to
    // publish its proxy (never spawn a thread to create a window).
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
}

/// Run the wry/tao event loop on the **main** thread, blocking forever (macOS only).
///
/// Must be called from `main()` on the main thread (AppKit requirement). The proxy is
/// published immediately; the window+webview are created lazily on the first `Resumed`
/// event (a WKWebView can't be created before the NSApplication run loop is active).
/// The window starts hidden and is revealed on the first navigate.
#[cfg(target_os = "macos")]
pub fn serve_browser_on_main_thread() -> ! {
    use tao::event::{Event, StartCause, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tao::window::WindowBuilder;

    eprintln!("[WRY] Running native WebView event loop on the main thread (macOS)");
    let event_loop = EventLoopBuilder::<WryCommand>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // Publish the proxy NOW — it's valid as soon as the loop exists, so tool threads in
    // `get_or_launch()` can proceed even though the webview is created later.
    if let Ok(mut slot) = MACOS_PROXY.lock() {
        *slot = Some(proxy);
    }
    MACOS_PROXY_CV.notify_all();

    let pending: PendingReplies = Arc::new(Mutex::new(HashMap::new()));
    let pending_nav: PendingNav = Arc::new(Mutex::new(None));
    // Persistent WebContext — cookies/cache/session across launches (returning-user, beats bot detection).
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("llama-chat-browser");
    let mut web_context = wry::WebContext::new(Some(data_dir));

    // Created lazily on first Resumed/Init (after the NSApp run loop starts).
    let mut window: Option<tao::window::Window> = None;
    let mut webview: Option<wry::WebView> = None;

    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(StartCause::Init) | Event::Resumed => {
                if webview.is_none() {
                    match WindowBuilder::new()
                        .with_title("LLaMA Chat — Browser")
                        .with_inner_size(tao::dpi::LogicalSize::new(1280.0, 900.0))
                        .with_visible(false) // revealed on first navigate
                        .build(target)
                    {
                        Ok(win) => {
                            match build_webview(
                                &win,
                                &mut web_context,
                                pending.clone(),
                                pending_nav.clone(),
                            ) {
                                Ok(wv) => {
                                    window = Some(win);
                                    webview = Some(wv);
                                    eprintln!("[WRY] macOS WebView ready (lazy, on main thread)");
                                }
                                Err(e) => eprintln!("[WRY] macOS WebView build failed: {e}"),
                            }
                        }
                        Err(e) => eprintln!("[WRY] macOS window build failed: {e}"),
                    }
                }
            }

            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                eprintln!("[WRY] Window closed by user (hiding)");
                if let Some(w) = &window {
                    w.set_visible(false);
                }
            }

            Event::UserEvent(cmd) => match (&webview, &window) {
                (Some(wv), Some(win)) => {
                    handle_wry_command(cmd, wv, win, &pending, &pending_nav);
                }
                _ => reply_webview_not_ready(cmd),
            },

            _ => {}
        }
    });
}

/// Run the tao event loop with a wry WebView on a spawned thread (Windows/Linux).
/// The window+webview are created eagerly before `run()`.
#[cfg(not(target_os = "macos"))]
fn run_event_loop(
    ready_tx: mpsc::Sender<Result<tao::event_loop::EventLoopProxy<WryCommand>, String>>,
) {
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tao::window::WindowBuilder;

    let mut builder = EventLoopBuilder::<WryCommand>::with_user_event();
    // Allow running on a non-main thread (Windows).
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

    let pending: PendingReplies = Arc::new(Mutex::new(HashMap::new()));
    let pending_nav: PendingNav = Arc::new(Mutex::new(None));
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("llama-chat-browser");
    let mut web_context = wry::WebContext::new(Some(data_dir));

    let webview = match build_webview(&window, &mut web_context, pending.clone(), pending_nav.clone()) {
        Ok(wv) => wv,
        Err(e) => {
            let _ = ready_tx.send(Err(e));
            return;
        }
    };

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

            Event::UserEvent(cmd) => {
                if handle_wry_command(cmd, &webview, &window, &pending, &pending_nav) {
                    *control_flow = ControlFlow::Exit;
                }
            }

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

    // Wait up to 30s for the page load event. On timeout, return Ok — the navigate
    // was initiated and the page may still be loading (slow network, heavy page, etc.).
    match rx.recv_timeout(std::time::Duration::from_secs(30)) {
        Ok(result) => result?,
        Err(_) => eprintln!("[WRY] Navigate: no load event within 30s, continuing anyway"),
    }
    Ok(())
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
    #[cfg(target_os = "macos")]
    {
        if let Ok(slot) = MACOS_PROXY.lock() {
            if let Some(ref proxy) = *slot {
                let _ = proxy.send_event(WryCommand::Close);
            }
        }
        return Ok(());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut guard = WRY.lock().map_err(|e| format!("wry lock: {e}"))?;
        if let Some(handle) = guard.take() {
            let _ = handle.proxy.send_event(WryCommand::Close);
        }
        Ok(())
    }
}
