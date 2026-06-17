//! Browser session abstraction — Tauri WebView with Chrome CDP fallback.

use serde_json::Value;

mod backends;
mod session_state;
mod tauri_session;

// Re-export public API so callers don't need to change.
pub use backends::{
    eval_in_browser_panel, notify_tauri_browser_close, notify_tauri_browser_navigate,
    eval_in_browser_tab, navigate_browser_tab, close_browser_tab, DEFAULT_TAB_ID,
};
pub use session_state::{clear_cache, current_session, open_session, remove_session};
pub use tauri_session::TauriHttpSession;

/// A controllable browser session.
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
    /// Get a slice of the page text starting at `offset` chars, up to `max_chars`.
    /// Returns the slice + a "[Page N of M]" footer when there's more content.
    fn get_full_text(&self, offset: usize, max_chars: usize) -> Result<String, String>;
    fn close(&mut self) -> Result<(), String>;
    fn url(&self) -> &str;
}
