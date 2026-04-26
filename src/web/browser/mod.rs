//! Browser module — Tauri WebView-based browsing for agent tools.

pub mod session;

/// Legacy browser backend enum — kept for backward compatibility with
/// config/pipeline code. Only None is used (Tauri WebView handles everything).
#[derive(Debug, Clone, PartialEq)]
pub enum BrowserBackend {
    None,
}

impl BrowserBackend {
    pub fn from_config(_s: Option<&str>) -> Self {
        Self::None
    }
}
