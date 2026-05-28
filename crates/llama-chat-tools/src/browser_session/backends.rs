//! Browser backend implementations: CDP, wry, Tauri.

// ─── Chrome CDP fallback (visible browser for web mode) ────────

#[cfg(feature = "cdp")]
pub(crate) mod cdp {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use headless_chrome::{Browser, LaunchOptions, Tab};
    use std::sync::Arc;

    /// Shared Chrome instance (one process, many tabs).
    static BROWSER: Mutex<Option<Browser>> = Mutex::new(None);
    /// Named tabs — each agent/session gets its own isolated tab.
    static TABS: Mutex<Option<HashMap<String, Arc<Tab>>>> = Mutex::new(None);

    fn ensure_browser() -> Result<(), String> {
        let mut guard = BROWSER.lock().map_err(|e| format!("CDP browser lock: {e}"))?;
        if guard.is_none() {
            eprintln!("[BROWSER_CDP] Launching visible Chrome window...");
            let options = LaunchOptions {
                headless: false,
                window_size: Some((1280, 900)),
                sandbox: false,
                enable_logging: false,
                ..LaunchOptions::default()
            };
            *guard = Some(Browser::new(options)
                .map_err(|e| format!("Chrome launch failed (is Chrome/Edge installed?): {e}"))?);
            eprintln!("[BROWSER_CDP] Chrome launched successfully");
        }
        Ok(())
    }

    /// Get or create a named tab. Tab `"main"` is the default/legacy tab.
    pub fn get_or_create_tab(tab_id: &str) -> Result<Arc<Tab>, String> {
        ensure_browser()?;
        let mut tabs = TABS.lock().map_err(|e| format!("CDP tabs lock: {e}"))?;
        let map = tabs.get_or_insert_with(HashMap::new);

        // Check if existing tab is still alive
        if let Some(tab) = map.get(tab_id) {
            if tab.evaluate("1", false).is_ok() {
                return Ok(Arc::clone(tab));
            }
            eprintln!("[BROWSER_CDP] Tab '{tab_id}' is dead, recreating...");
            map.remove(tab_id);
        }

        // Create new tab in the shared browser
        let browser_guard = BROWSER.lock().map_err(|e| format!("CDP browser lock: {e}"))?;
        let browser = browser_guard.as_ref().ok_or("Browser not initialized")?;
        let tab = browser.new_tab()
            .map_err(|e| format!("Chrome new tab '{tab_id}': {e}"))?;
        let tab_arc = Arc::clone(&tab);
        map.insert(tab_id.to_string(), tab);
        eprintln!("[BROWSER_CDP] Created tab '{tab_id}'");
        Ok(tab_arc)
    }

    pub fn navigate_tab(url: &str, tab_id: &str) -> Result<(), String> {
        let tab = get_or_create_tab(tab_id)?;
        tab.navigate_to(url)
            .map_err(|e| format!("CDP navigate: {e}"))?;
        tab.wait_until_navigated()
            .map_err(|e| format!("CDP wait: {e}"))?;
        Ok(())
    }

    pub fn navigate(url: &str) -> Result<(), String> {
        navigate_tab(url, "main")
    }

    pub fn evaluate_tab(js: &str, tab_id: &str) -> Result<String, String> {
        let tab = get_or_create_tab(tab_id)?;
        let result = tab.evaluate(js, false)
            .map_err(|e| format!("CDP eval: {e}"))?;
        match result.value {
            Some(serde_json::Value::String(s)) => Ok(s),
            Some(v) => Ok(v.to_string()),
            None => Ok(String::new()),
        }
    }

    pub fn evaluate(js: &str) -> Result<String, String> {
        evaluate_tab(js, "main")
    }

    /// Close a single named tab and remove it from the map.
    pub fn close_tab(tab_id: &str) -> Result<(), String> {
        let mut tabs = TABS.lock().map_err(|e| format!("CDP tabs lock: {e}"))?;
        if let Some(map) = tabs.as_mut() {
            if map.remove(tab_id).is_some() {
                eprintln!("[BROWSER_CDP] Closed tab '{tab_id}'");
            }
        }
        Ok(())
    }

    /// Close all tabs and kill Chrome.
    pub fn close() -> Result<(), String> {
        if let Ok(mut tabs) = TABS.lock() {
            if let Some(map) = tabs.as_mut() {
                eprintln!("[BROWSER_CDP] Closing {} tab(s)", map.len());
                map.clear();
            }
            *tabs = None;
        }
        if let Ok(mut guard) = BROWSER.lock() {
            *guard = None; // Drop kills Chrome process
        }
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

// ─── Tab-aware public API ───────────────────────────────────────────────────

/// Navigate a specific named browser tab. In CDP mode each tab_id is isolated.
/// In Tauri/wry mode tab_id is ignored (single panel).
pub fn navigate_browser_tab(url: &str, tab_id: &str) -> Result<(), String> {
    // Tauri and wry don't support multi-tab — delegate to single-tab navigate
    if is_tauri_available() {
        return notify_tauri_browser_navigate(url);
    }
    #[cfg(feature = "wry-browser")]
    {
        if crate::wry_browser::navigate(url).is_ok() { return Ok(()); }
    }
    #[cfg(feature = "cdp")]
    {
        return cdp::navigate_tab(url, tab_id);
    }
    #[allow(unreachable_code)]
    Err("No browser backend available".into())
}

/// Evaluate JS in a specific named browser tab.
/// In Tauri/wry mode tab_id is ignored (single panel).
pub fn eval_in_browser_tab(js: &str, tab_id: &str) -> Result<String, String> {
    if is_tauri_available() {
        return eval_in_browser_panel(js);
    }
    #[cfg(feature = "wry-browser")]
    {
        if let Ok(r) = crate::wry_browser::evaluate(js) { return Ok(r); }
    }
    #[cfg(feature = "cdp")]
    {
        return cdp::evaluate_tab(js, tab_id);
    }
    #[allow(unreachable_code)]
    Err("No browser backend available".into())
}

/// Close a specific named browser tab (CDP only; no-op for Tauri/wry).
pub fn close_browser_tab(tab_id: &str) -> Result<(), String> {
    #[cfg(feature = "cdp")]
    {
        if !is_tauri_available() {
            return cdp::close_tab(tab_id);
        }
    }
    Ok(())
}
