//! Browser backend implementations: CDP, wry, Tauri.

// ─── Chrome CDP fallback (visible browser for web mode) ────────

#[cfg(feature = "cdp")]
pub(crate) mod cdp {
    use std::sync::Mutex;
    use headless_chrome::{Browser, LaunchOptions, Tab};
    use std::sync::Arc;

    static CDP: Mutex<Option<(Browser, Arc<Tab>)>> = Mutex::new(None);

    /// Get or launch the visible Chrome browser. Returns the active tab.
    pub fn get_or_launch() -> Result<Arc<Tab>, String> {
        let mut guard = CDP.lock().map_err(|e| format!("CDP lock: {e}"))?;
        if let Some((_, ref tab)) = *guard {
            // Check tab is still alive by trying a trivial eval
            if tab.evaluate("1", false).is_ok() {
                return Ok(Arc::clone(tab));
            }
            eprintln!("[BROWSER_CDP] Existing tab is dead, relaunching...");
        }
        eprintln!("[BROWSER_CDP] Launching visible Chrome window...");
        let options = LaunchOptions {
            headless: false,
            window_size: Some((1280, 900)),
            sandbox: false,
            enable_logging: false,
            ..LaunchOptions::default()
        };
        let browser = Browser::new(options)
            .map_err(|e| format!("Chrome launch failed (is Chrome/Edge installed?): {e}"))?;
        let tab = browser.new_tab()
            .map_err(|e| format!("Chrome new tab: {e}"))?;
        let tab_clone = Arc::clone(&tab);
        *guard = Some((browser, tab));
        eprintln!("[BROWSER_CDP] Chrome launched successfully");
        Ok(tab_clone)
    }

    pub fn navigate(url: &str) -> Result<(), String> {
        let tab = get_or_launch()?;
        tab.navigate_to(url)
            .map_err(|e| format!("CDP navigate: {e}"))?;
        tab.wait_until_navigated()
            .map_err(|e| format!("CDP wait: {e}"))?;
        Ok(())
    }

    pub fn evaluate(js: &str) -> Result<String, String> {
        let tab = get_or_launch()?;
        let result = tab.evaluate(js, false)
            .map_err(|e| format!("CDP eval: {e}"))?;
        match result.value {
            Some(serde_json::Value::String(s)) => Ok(s),
            Some(v) => Ok(v.to_string()),
            None => Ok(String::new()),
        }
    }

    pub fn close() -> Result<(), String> {
        let mut guard = CDP.lock().map_err(|e| format!("CDP lock: {e}"))?;
        if guard.is_some() {
            eprintln!("[BROWSER_CDP] Closing Chrome");
        }
        *guard = None; // Drop kills Chrome process
        Ok(())
    }
}

pub(crate) const TAURI_UI_BRIDGE_BASE: &str = "http://127.0.0.1:18091";

/// Check if the Tauri UI bridge is available (desktop mode).
/// Caches result for 30 seconds to avoid constant probing.
pub(crate) fn is_tauri_available() -> bool {
    use std::sync::Mutex;
    static LAST_CHECK: Mutex<Option<(std::time::Instant, bool)>> = Mutex::new(None);
    if let Ok(guard) = LAST_CHECK.lock() {
        if let Some((when, result)) = *guard {
            if when.elapsed() < std::time::Duration::from_secs(30) {
                return result;
            }
        }
    }
    let available = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_millis(500))
        .build()
        .post(&format!("{TAURI_UI_BRIDGE_BASE}/api/eval"))
        .set("Content-Type", "application/json")
        .send_string(&serde_json::json!({"js": "1", "target": "browser-panel"}).to_string())
        .is_ok();
    if let Ok(mut guard) = LAST_CHECK.lock() {
        *guard = Some((std::time::Instant::now(), available));
    }
    available
}

/// Execute JavaScript in the browser panel — Tauri WebView or Chrome CDP fallback.
pub fn eval_in_browser_panel(js: &str) -> Result<String, String> {
    // Try Tauri WebView first
    if is_tauri_available() {
        // When Tauri is running, ONLY use Tauri — never fall through to wry/CDP.
        // Falling through while Tauri is available causes stray Chrome windows to open.
        for attempt in 0..3 {
            let resp = ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .post(&format!("{TAURI_UI_BRIDGE_BASE}/api/eval"))
                .set("Content-Type", "application/json")
                .send_string(&serde_json::json!({
                    "js": js,
                    "target": "browser-panel"
                }).to_string());

            match resp {
                Ok(resp) => {
                    let body = resp.into_string().unwrap_or_default();
                    if body.contains("Result channel closed") || body.contains("eval timed out") {
                        if attempt < 2 {
                            eprintln!("[BROWSER_EVAL] attempt {}: {}, retrying...", attempt + 1, body.trim());
                            std::thread::sleep(std::time::Duration::from_millis(1000));
                            continue;
                        }
                        return Err(format!("eval_in_browser_panel: Tauri eval failed after retries: {}", body.trim()));
                    }
                    if body.starts_with('"') && body.ends_with('"') {
                        return Ok(serde_json::from_str::<String>(&body).unwrap_or(body));
                    }
                    return Ok(body);
                }
                Err(e) => return Err(format!("eval_in_browser_panel: Tauri HTTP error: {e}")),
            }
        }
        return Err("eval_in_browser_panel: Tauri eval failed after retries".into());
    }

    // Try wry native WebView (web mode only — Tauri not running)
    #[cfg(feature = "wry-browser")]
    {
        match crate::wry_browser::evaluate(js) {
            Ok(result) => return Ok(result),
            Err(e) => eprintln!("[BROWSER_WRY] eval failed: {e}"),
        }
    }

    // Try Chrome CDP (web mode only — Tauri not running)
    #[cfg(feature = "cdp")]
    {
        match cdp::evaluate(js) {
            Ok(result) => return Ok(result),
            Err(e) => eprintln!("[BROWSER_CDP] eval failed: {e}"),
        }
    }

    Err("eval_in_browser_panel: no backend available".into())
}

/// Navigate the visible browser panel — Tauri WebView or Chrome CDP fallback.
pub fn notify_tauri_browser_navigate(url: &str) -> Result<(), String> {
    eprintln!("[BROWSER_HTTP] notify_tauri_browser_navigate: {url}");

    // Try Tauri WebView first
    if is_tauri_available() {
        // When Tauri is running, ONLY use Tauri — never fall through to wry/CDP.
        let body = serde_json::json!({ "url": url });
        return if ureq::post(&format!("{TAURI_UI_BRIDGE_BASE}/bridge/browser/navigate"))
            .set("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(3))
            .send_string(&body.to_string())
            .is_ok()
        {
            Ok(())
        } else {
            Err(format!("notify_tauri_browser_navigate: Tauri navigate request failed for {url}"))
        };
    }

    // Try wry native WebView (web mode only — Tauri not running)
    #[cfg(feature = "wry-browser")]
    {
        match crate::wry_browser::navigate(url) {
            Ok(()) => {
                eprintln!("[BROWSER_WRY] navigated to {url}");
                return Ok(());
            }
            Err(e) => eprintln!("[BROWSER_WRY] navigate failed: {e}"),
        }
    }

    // Try Chrome CDP (web mode only — Tauri not running)
    #[cfg(feature = "cdp")]
    {
        match cdp::navigate(url) {
            Ok(()) => {
                eprintln!("[BROWSER_CDP] navigated to {url}");
                return Ok(());
            }
            Err(e) => eprintln!("[BROWSER_CDP] navigate failed: {e}"),
        }
    }

    Err("No browser backend available (Tauri WebView not running, Chrome not found)".into())
}

/// Close the visible browser panel — Tauri WebView and/or Chrome CDP.
pub fn notify_tauri_browser_close() -> Result<(), String> {
    // Try Tauri
    let _ = ureq::post(&format!("{TAURI_UI_BRIDGE_BASE}/bridge/browser/close"))
        .timeout(std::time::Duration::from_secs(3))
        .call();

    // Also close wry if active
    #[cfg(feature = "wry-browser")]
    { let _ = crate::wry_browser::close(); }

    // Also close CDP if active
    #[cfg(feature = "cdp")]
    { let _ = cdp::close(); }

    Ok(())
}
