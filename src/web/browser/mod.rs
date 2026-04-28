//! Browser module — re-exports from workspace crates.

/// Browser session management (Tauri WebView-based).
#[allow(unused_imports)]
pub mod session {
    pub use llama_chat_tools::browser_session::{
        BrowserSession, TauriHttpSession,
        clear_cache, current_session, open_session,
        eval_in_browser_panel, notify_tauri_browser_navigate, notify_tauri_browser_close,
    };
}

/// Re-export BrowserBackend from the engine crate.
#[allow(unused_imports)]
pub use llama_chat_engine::browser::BrowserBackend;
