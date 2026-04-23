//! Browser session abstraction — unified API for agent browser tools.
//!
//! Two backends:
//! - TauriHttpSession (default): opens Tauri native webview for the user,
//!   reads content via HTTP (ureq). No Camofox needed.
//! - CamofoxSession (fallback): headless Firefox for anti-detection browsing.

use std::io::Read;
use serde_json::Value;

use super::camofox;

/// A controllable browser session. Implementations route to different
/// backends (Camofox HTTP, Tauri WebView IPC, etc.) but expose the same
/// methods so agent tools don't need to know which backend is active.
pub trait BrowserSession: Send + Sync {
    fn navigate(&mut self, url: &str) -> Result<(), String>;
    fn click(&self, selector: &str) -> Result<(), String>;
    fn type_text(&self, selector: &str, text: &str, press_enter: bool) -> Result<(), String>;
    fn eval(&self, js: &str) -> Result<Value, String>;
    fn html(&self) -> Result<String, String>;
    fn screenshot(&self) -> Result<Vec<u8>, String>;
    fn wait_for(&self, selector: &str, timeout_ms: u64) -> Result<bool, String>;
    fn press_key(&self, key: &str) -> Result<(), String>;
    fn snapshot(&self) -> Result<String, String>;
    fn close(&mut self) -> Result<(), String>;
    fn url(&self) -> &str;
}

const TAURI_UI_BRIDGE_BASE: &str = "http://127.0.0.1:18091";

/// Camofox-backed browser session. Talks to the local Camofox HTTP server.
/// Currently unused — TauriHttpSession is the default. Kept for future
/// anti-detection browsing needs.
#[allow(dead_code)]
pub struct CamofoxSession {
    pub tab_id: String,
    pub current_url: String,
}

#[allow(dead_code)]
impl CamofoxSession {
    /// Open a new session by creating a Camofox tab.
    pub fn open(url: &str) -> Result<Self, String> {
        let tab_id = camofox::cf_create_tab(url)?;
        camofox::set_agent_tab(tab_id.clone(), url.to_string());
        Ok(Self {
            tab_id,
            current_url: url.to_string(),
        })
    }

    /// Resume the active session if one exists (e.g. after process restart).
    pub fn resume_active() -> Option<Self> {
        camofox::get_agent_tab().map(|(tab_id, url)| Self {
            tab_id,
            current_url: url,
        })
    }
}

impl BrowserSession for CamofoxSession {
    fn navigate(&mut self, url: &str) -> Result<(), String> {
        camofox::cf_navigate(&self.tab_id, url)?;
        self.current_url = url.to_string();
        camofox::set_agent_tab(self.tab_id.clone(), url.to_string());
        Ok(())
    }

    fn click(&self, selector: &str) -> Result<(), String> {
        camofox::cf_click_selector(&self.tab_id, selector)
    }

    fn type_text(&self, selector: &str, text: &str, press_enter: bool) -> Result<(), String> {
        camofox::cf_type_selector(&self.tab_id, selector, text, press_enter)
    }

    fn eval(&self, js: &str) -> Result<Value, String> {
        camofox::cf_eval(&self.tab_id, js)
    }

    fn html(&self) -> Result<String, String> {
        camofox::cf_get_html(&self.tab_id)
    }

    fn screenshot(&self) -> Result<Vec<u8>, String> {
        camofox::take_tab_screenshot_jpeg(&self.tab_id, 80)
            .ok_or_else(|| "failed to capture screenshot".to_string())
    }

    fn wait_for(&self, selector: &str, timeout_ms: u64) -> Result<bool, String> {
        camofox::cf_wait_selector(&self.tab_id, selector, timeout_ms)
    }

    fn press_key(&self, key: &str) -> Result<(), String> {
        camofox::cf_press_key(&self.tab_id, key)
    }

    fn snapshot(&self) -> Result<String, String> {
        camofox::cf_snapshot(&self.tab_id)
    }

    fn close(&mut self) -> Result<(), String> {
        camofox::clear_agent_tab();
        Ok(())
    }

    fn url(&self) -> &str {
        &self.current_url
    }
}

// ─── Tauri HTTP Session (default, no Camofox needed) ──────────────

/// Browser session that opens the Tauri native webview (user sees the page)
/// and reads content via HTTP (ureq). No external browser server needed.
pub struct TauriHttpSession {
    pub current_url: String,
}

impl TauriHttpSession {
    pub fn open(url: &str) -> Result<Self, String> {
        // Tell the Tauri app to open the browser panel
        let _ = notify_tauri_browser_navigate(url);
        Ok(Self {
            current_url: url.to_string(),
        })
    }

    /// Fetch page content via HTTP using ureq + html2text.
    fn fetch_text(&self, max_chars: usize) -> Result<String, String> {
        let resp = ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(10))
            .timeout_read(std::time::Duration::from_secs(15))
            .build()
            .get(&self.current_url)
            .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .call()
            .map_err(|e| format!("HTTP fetch failed: {e}"))?;
        // Limit body size to prevent blocking on huge pages
        let max_body: u64 = 500_000;
        let reader = resp.into_reader();
        let mut body = String::new();
        let bytes_read = reader.take(max_body).read_to_string(&mut body)
            .map_err(|e| format!("Read body failed: {e}"))?;
        let was_truncated = bytes_read as u64 >= max_body;
        // Strip HTML tags for plain text
        let mut text = html2text::from_read(body.as_bytes(), 120);
        if was_truncated {
            text.push_str("\n\n[Page was too large — only the first 500KB was read. Use browser_get_links to find specific article URLs, then browser_navigate to each one.]");
        }
        if text.len() > max_chars {
            let mut end = max_chars;
            while end > 0 && !text.is_char_boundary(end) { end -= 1; }
            Ok(format!("{}...\n[Truncated]", &text[..end]))
        } else {
            Ok(text)
        }
    }

    fn fetch_html(&self) -> Result<String, String> {
        let resp = ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(10))
            .timeout_read(std::time::Duration::from_secs(15))
            .build()
            .get(&self.current_url)
            .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .call()
            .map_err(|e| format!("HTTP fetch failed: {e}"))?;
        resp.into_string()
            .map_err(|e| format!("Read body failed: {e}"))
    }
}

impl BrowserSession for TauriHttpSession {
    fn navigate(&mut self, url: &str) -> Result<(), String> {
        self.current_url = url.to_string();
        let _ = notify_tauri_browser_navigate(url);
        Ok(())
    }

    fn click(&self, _selector: &str) -> Result<(), String> {
        Err("Click not supported in Tauri HTTP mode. Use Camofox for interactive browsing.".into())
    }

    fn type_text(&self, _selector: &str, _text: &str, _press_enter: bool) -> Result<(), String> {
        Err("Type not supported in Tauri HTTP mode. Use Camofox for interactive browsing.".into())
    }

    fn eval(&self, _js: &str) -> Result<Value, String> {
        Err("JS eval not supported in Tauri HTTP mode.".into())
    }

    fn html(&self) -> Result<String, String> {
        self.fetch_html()
    }

    fn screenshot(&self) -> Result<Vec<u8>, String> {
        Err("Screenshot not supported in Tauri HTTP mode.".into())
    }

    fn wait_for(&self, _selector: &str, _timeout_ms: u64) -> Result<bool, String> {
        // Can't wait for DOM elements via HTTP — just return true
        Ok(true)
    }

    fn press_key(&self, _key: &str) -> Result<(), String> {
        Err("Press key not supported in Tauri HTTP mode.".into())
    }

    fn snapshot(&self) -> Result<String, String> {
        self.fetch_text(20_000)
    }

    fn close(&mut self) -> Result<(), String> {
        let _ = notify_tauri_browser_close();
        Ok(())
    }

    fn url(&self) -> &str {
        &self.current_url
    }
}

// ─── Active session tracking ─────────────────────────────────────

use std::sync::Mutex;

/// The active session URL — shared between calls.
static ACTIVE_URL: Mutex<Option<String>> = Mutex::new(None);

/// Get or create the active session.
pub fn current_session() -> Result<TauriHttpSession, String> {
    let url = ACTIVE_URL.lock().ok()
        .and_then(|g| g.clone())
        .ok_or("No active browser session. Call browser_navigate(url) first.")?;
    Ok(TauriHttpSession { current_url: url })
}

/// Open a fresh session at the given URL.
pub fn open_session(url: &str) -> Result<TauriHttpSession, String> {
    let full_url = if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!("https://{url}")
    };
    let session = TauriHttpSession::open(&full_url)?;
    if let Ok(mut guard) = ACTIVE_URL.lock() {
        *guard = Some(full_url);
    }
    Ok(session)
}

/// Best-effort: ask the Tauri app process to open/navigate the visible native browser panel.
/// This bridge is only available in desktop mode; callers should ignore failures and continue.
pub fn notify_tauri_browser_navigate(url: &str) -> Result<(), String> {
    let body = serde_json::json!({ "url": url });
    ureq::post(&format!("{TAURI_UI_BRIDGE_BASE}/bridge/browser/navigate"))
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(3))
        .send_string(&body.to_string())
        .map_err(|e| format!("Tauri browser bridge navigate failed: {e}"))?;
    Ok(())
}

/// Best-effort: ask the Tauri app process to close the visible native browser panel.
pub fn notify_tauri_browser_close() -> Result<(), String> {
    ureq::post(&format!("{TAURI_UI_BRIDGE_BASE}/bridge/browser/close"))
        .timeout(std::time::Duration::from_secs(3))
        .call()
        .map_err(|e| format!("Tauri browser bridge close failed: {e}"))?;
    Ok(())
}
