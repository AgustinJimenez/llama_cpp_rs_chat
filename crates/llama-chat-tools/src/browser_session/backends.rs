//! Browser backend implementations: wry, Tauri.

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
    return match crate::wry_browser::evaluate(js) {
        Ok(result) => Ok(result),
        Err(e) => Err(format!("eval_in_browser_panel: wry eval failed: {e}")),
    };

    #[allow(unreachable_code)]
    Err("eval_in_browser_panel: no browser backend available (Tauri not running, wry not compiled)".into())
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
    return match crate::wry_browser::navigate(url) {
        Ok(()) => {
            eprintln!("[BROWSER_WRY] navigated to {url}");
            Ok(())
        }
        Err(e) => Err(format!("notify_tauri_browser_navigate: wry navigate failed: {e}")),
    };

    #[allow(unreachable_code)]
    Err("No browser backend available (Tauri not running, wry not compiled)".into())
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

    Ok(())
}

// ─── Tab-aware public API ───────────────────────────────────────────────────

/// Navigate a specific named browser tab. In CDP mode each tab_id is isolated.
/// In Tauri/wry mode tab_id is ignored (single panel).
pub fn navigate_browser_tab(url: &str, _tab_id: &str) -> Result<(), String> {
    // Tauri and wry don't support multi-tab — delegate to single-tab navigate
    notify_tauri_browser_navigate(url)
}

/// Evaluate JS in a specific named browser tab.
/// In Tauri/wry mode tab_id is ignored (single panel).
pub fn eval_in_browser_tab(js: &str, _tab_id: &str) -> Result<String, String> {
    // Tauri and wry don't support multi-tab — delegate to single-panel eval
    eval_in_browser_panel(js)
}

/// No-op: tab isolation is CDP-only; wry/Tauri use a single panel.
pub fn close_browser_tab(_tab_id: &str) -> Result<(), String> {
    Ok(())
}
