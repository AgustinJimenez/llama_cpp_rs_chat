//! Multi-backend browser module for JS-rendered web page fetching.
//!
//! Supports multiple browser backends: Chrome (headless_chrome crate),
//! chrome-headless-shell (lightweight variant), agent-browser (subprocess),
//! and a no-JS HTTP-only fallback.

pub mod chrome;
pub mod chrome_headless;
pub mod agent_browser;

/// Which browser engine to use for web_fetch / web_search.
#[derive(Debug, Clone, PartialEq)]
pub enum BrowserBackend {
    /// Standard headless Chrome via `headless_chrome` crate (default).
    Chrome,
    /// Google's lightweight chrome-headless-shell binary (same CDP, less RAM).
    ChromeHeadlessShell,
    /// Vercel agent-browser: Rust CLI + Playwright daemon (subprocess).
    AgentBrowser,
    /// No browser — HTTP-only via ureq, no JS rendering.
    None,
}

impl BrowserBackend {
    /// Parse from config string. Returns Chrome as default.
    pub fn from_config(s: Option<&str>) -> Self {
        match s {
            Some("chrome-headless-shell") => Self::ChromeHeadlessShell,
            Some("agent-browser") => Self::AgentBrowser,
            Some("none") => Self::None,
            _ => Self::Chrome, // default
        }
    }

    /// Config string representation.
    pub fn as_config_str(&self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::ChromeHeadlessShell => "chrome-headless-shell",
            Self::AgentBrowser => "agent-browser",
            Self::None => "none",
        }
    }
}

/// Fetch a web page as plain text using the specified backend.
pub fn web_fetch(backend: &BrowserBackend, url: &str, max_chars: usize) -> Result<String, String> {
    let start = std::time::Instant::now();
    let result = match backend {
        BrowserBackend::Chrome => chrome::chrome_web_fetch(url, max_chars),
        BrowserBackend::ChromeHeadlessShell => chrome_headless::fetch_text(url, max_chars),
        BrowserBackend::AgentBrowser => agent_browser::fetch_text(url, max_chars),
        BrowserBackend::None => http_only_fetch(url, max_chars),
    };
    let elapsed = start.elapsed();
    let tag = backend.as_config_str();
    match &result {
        Ok(text) => eprintln!("[BROWSER:{tag}] Fetched {url} in {:.2}s ({} chars)", elapsed.as_secs_f64(), text.len()),
        Err(e) => eprintln!("[BROWSER:{tag}] Failed {url} in {:.2}s: {e}", elapsed.as_secs_f64()),
    }
    result
}

/// Fetch raw HTML from a URL using the specified backend.
pub fn web_fetch_html(backend: &BrowserBackend, url: &str) -> Result<String, String> {
    let start = std::time::Instant::now();
    let result = match backend {
        BrowserBackend::Chrome => chrome::chrome_web_fetch_html(url),
        BrowserBackend::ChromeHeadlessShell => chrome_headless::fetch_html(url),
        BrowserBackend::AgentBrowser => agent_browser::fetch_html(url),
        BrowserBackend::None => http_only_fetch_html(url),
    };
    let elapsed = start.elapsed();
    let tag = backend.as_config_str();
    match &result {
        Ok(html) => eprintln!("[BROWSER:{tag}] Fetched HTML {url} in {:.2}s ({} bytes)", elapsed.as_secs_f64(), html.len()),
        Err(e) => eprintln!("[BROWSER:{tag}] Failed HTML {url} in {:.2}s: {e}", elapsed.as_secs_f64()),
    }
    result
}

/// Shut down all browser backends to free memory.
#[allow(dead_code)]
pub fn shutdown_all() {
    chrome::shutdown_browser();
    chrome_headless::shutdown();
}

// ── HTTP-only fallback (no JS rendering) ────────────────────────────

fn http_only_fetch(url: &str, max_chars: usize) -> Result<String, String> {
    let html = http_only_fetch_html(url)?;
    let text = html2text::from_read(html.as_bytes(), 120);
    if text.len() > max_chars {
        let mut truncate_at = max_chars;
        while truncate_at > 0 && !text.is_char_boundary(truncate_at) {
            truncate_at -= 1;
        }
        Ok(format!(
            "{}...\n[Truncated: first {} of {} chars]",
            &text[..truncate_at], truncate_at, text.len()
        ))
    } else {
        Ok(text)
    }
}

fn http_only_fetch_html(url: &str) -> Result<String, String> {
    let resp = ureq::get(url)
        .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .map_err(|e| format!("HTTP request failed: {e}"))?;
    resp.into_string()
        .map_err(|e| format!("Failed to read response body: {e}"))
}
