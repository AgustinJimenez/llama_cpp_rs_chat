//! Browser backend implementations: wry, Tauri.

pub(crate) const TAURI_UI_BRIDGE_BASE: &str = "http://127.0.0.1:18091";

/// Default tab id — maps to the original single shared `browser-panel` webview,
/// preserving behavior for callers that don't know about multi-tab.
pub const DEFAULT_TAB_ID: &str = "main";

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

/// Map a logical tab_id to the Tauri webview label used for that tab.
/// `"main"`/`"default"`/empty all map to the original single-panel label,
/// for backward compatibility with existing single-tab callers and UX
/// (the user-visible globe icon only ever shows the `"browser-panel"` webview).
pub(crate) fn tab_label(tab_id: &str) -> String {
    if tab_id.is_empty() || tab_id == DEFAULT_TAB_ID || tab_id == "default" {
        "browser-panel".to_string()
    } else {
        let safe: String = tab_id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        format!("browser-panel-{safe}")
    }
}

/// Execute JavaScript in a specific browser tab — Tauri WebView or Chrome CDP fallback.
pub fn eval_in_browser_tab(js: &str, tab_id: &str) -> Result<String, String> {
    // Try Tauri WebView first
    if is_tauri_available() {
        // When Tauri is running, ONLY use Tauri — never fall through to wry/CDP.
        // Falling through while Tauri is available causes stray Chrome windows to open.
        let target = tab_label(tab_id);
        for attempt in 0..3 {
            let resp = ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .post(&format!("{TAURI_UI_BRIDGE_BASE}/api/eval"))
                .set("Content-Type", "application/json")
                .send_string(&serde_json::json!({
                    "js": js,
                    "target": target
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
                        return Err(format!("eval_in_browser_tab({tab_id}): Tauri eval failed after retries: {}", body.trim()));
                    }
                    if body.starts_with('"') && body.ends_with('"') {
                        return Ok(serde_json::from_str::<String>(&body).unwrap_or(body));
                    }
                    return Ok(body);
                }
                Err(e) => return Err(format!("eval_in_browser_tab({tab_id}): Tauri HTTP error: {e}")),
            }
        }
        return Err(format!("eval_in_browser_tab({tab_id}): Tauri eval failed after retries"));
    }

    // Try wry native WebView (web mode only — Tauri not running). Each tab_id gets
    // its own Window + WebView, created on demand.
    #[cfg(feature = "wry-browser")]
    return match crate::wry_browser::evaluate(js, tab_id) {
        Ok(result) => Ok(result),
        Err(e) => Err(format!("eval_in_browser_tab({tab_id}): wry eval failed: {e}")),
    };

    #[allow(unreachable_code)]
    Err(format!("eval_in_browser_tab({tab_id}): no browser backend available (Tauri not running, wry not compiled)"))
}

/// Navigate a specific browser tab — Tauri WebView or Chrome CDP fallback.
/// In Tauri mode each tab_id gets its own child webview (created on demand by
/// the `/bridge/browser/navigate` handler). In wry mode each tab_id also gets
/// its own Window + WebView, created on demand.
pub fn navigate_browser_tab(url: &str, tab_id: &str) -> Result<(), String> {
    eprintln!("[BROWSER_HTTP] navigate_browser_tab({tab_id}): {url}");

    // Try Tauri WebView first
    if is_tauri_available() {
        // When Tauri is running, ONLY use Tauri — never fall through to wry/CDP.
        let target = tab_label(tab_id);
        let body = serde_json::json!({ "url": url, "target": target });
        return if ureq::post(&format!("{TAURI_UI_BRIDGE_BASE}/bridge/browser/navigate"))
            .set("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(3))
            .send_string(&body.to_string())
            .is_ok()
        {
            Ok(())
        } else {
            Err(format!("navigate_browser_tab({tab_id}): Tauri navigate request failed for {url}"))
        };
    }

    // Try wry native WebView (web mode only — Tauri not running). Each tab_id gets
    // its own Window + WebView, created on demand.
    #[cfg(feature = "wry-browser")]
    return match crate::wry_browser::navigate(url, tab_id) {
        Ok(()) => {
            eprintln!("[BROWSER_WRY] tab '{tab_id}' navigated to {url}");
            Ok(())
        }
        Err(e) => Err(format!("navigate_browser_tab({tab_id}): wry navigate failed: {e}")),
    };

    #[allow(unreachable_code)]
    Err(format!("navigate_browser_tab({tab_id}): no browser backend available (Tauri not running, wry not compiled)"))
}

/// Close a specific browser tab — destroys its Tauri child webview or wry Window.
pub fn close_browser_tab(tab_id: &str) -> Result<(), String> {
    let target = tab_label(tab_id);
    let _ = ureq::post(&format!("{TAURI_UI_BRIDGE_BASE}/bridge/browser/close"))
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(3))
        .send_string(&serde_json::json!({ "target": target }).to_string());

    #[cfg(feature = "wry-browser")]
    let _ = crate::wry_browser::close(tab_id);

    Ok(())
}

// ─── Backward-compatible single-tab wrappers ────────────────────────────────
// Existing callers (web-mode REST routes, the web-search-fallback "open_url"/
// "close_browser_view" tools) keep working unchanged against the default tab.

/// Execute JavaScript in the default browser tab.
pub fn eval_in_browser_panel(js: &str) -> Result<String, String> {
    eval_in_browser_tab(js, DEFAULT_TAB_ID)
}

/// Navigate the default browser tab.
pub fn notify_tauri_browser_navigate(url: &str) -> Result<(), String> {
    navigate_browser_tab(url, DEFAULT_TAB_ID)
}

/// Close the default browser tab.
pub fn notify_tauri_browser_close() -> Result<(), String> {
    close_browser_tab(DEFAULT_TAB_ID)
}
