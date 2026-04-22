//! Browser session abstraction — unified API for agent browser tools.
//!
//! Currently has one backend (Camofox), but designed so a Tauri-native
//! WebView backend can be added without changing the agent tools.

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
pub struct CamofoxSession {
    pub tab_id: String,
    pub current_url: String,
}

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

/// Get or create the active session. Used by agent tools — if no session
/// exists, this returns an error so the agent knows it needs to call
/// `browser_navigate` first.
pub fn current_session() -> Result<CamofoxSession, String> {
    CamofoxSession::resume_active()
        .ok_or_else(|| "No active browser session. Call browser_navigate(url) first.".to_string())
}

/// Open a fresh session at the given URL (closes any existing one).
pub fn open_session(url: &str) -> Result<CamofoxSession, String> {
    // Ensure the URL has a scheme
    let full_url = if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!("https://{url}")
    };
    CamofoxSession::open(&full_url)
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
