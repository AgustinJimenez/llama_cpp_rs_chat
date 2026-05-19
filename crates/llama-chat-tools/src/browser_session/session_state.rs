//! Active browser session state — shared between calls.

use std::sync::Mutex;

use super::tauri_session::TauriHttpSession;

pub(crate) static ACTIVE_URL: Mutex<Option<String>> = Mutex::new(None);
pub(crate) static CACHED_HTML: Mutex<Option<String>> = Mutex::new(None);
pub(crate) static CACHED_TEXT: Mutex<Option<String>> = Mutex::new(None);

/// Clear cached HTML/text (e.g. after a click that may navigate).
pub fn clear_cache() {
    if let Ok(mut g) = CACHED_HTML.lock() { *g = None; }
    if let Ok(mut g) = CACHED_TEXT.lock() { *g = None; }
}

/// Get or create the active session.
pub fn current_session() -> Result<TauriHttpSession, String> {
    let url = ACTIVE_URL.lock().ok()
        .and_then(|g| g.clone())
        .ok_or("No active browser session. Call browser_navigate(url) first.")?;
    let cached_html = CACHED_HTML.lock().ok().and_then(|g| g.clone());
    let cached_text = CACHED_TEXT.lock().ok().and_then(|g| g.clone());
    Ok(TauriHttpSession { current_url: url, cached_html, cached_text })
}

/// Open a fresh session at the given URL.
pub fn open_session(url: &str) -> Result<TauriHttpSession, String> {
    let full_url = if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!("https://{url}")
    };
    let mut session = TauriHttpSession::open(&full_url)?;
    eprintln!("[BROWSER_HTTP] open_session: fetching {full_url}");
    let fetch_start = std::time::Instant::now();
    // Fetch and cache immediately so subsequent reads are instant
    if let Ok(html) = session.do_fetch() {
        eprintln!("[BROWSER_HTTP] fetch done ({}ms), stripping HTML ({} bytes)...", fetch_start.elapsed().as_millis(), html.len());
        let strip_start = std::time::Instant::now();
        let text = TauriHttpSession::strip_html(&html);
        eprintln!("[BROWSER_HTTP] strip_html done ({}ms), text={} bytes", strip_start.elapsed().as_millis(), text.len());
        session.cached_html = Some(html.clone());
        session.cached_text = Some(text.clone());
        eprintln!("[BROWSER_HTTP] storing cache...");
        if let Ok(mut g) = CACHED_HTML.lock() { *g = Some(html); }
        if let Ok(mut g) = CACHED_TEXT.lock() { *g = Some(text); }
        eprintln!("[BROWSER_HTTP] cache stored");
    }
    if let Ok(mut guard) = ACTIVE_URL.lock() {
        *guard = Some(full_url.clone());
    }
    eprintln!("[BROWSER_HTTP] open_session COMPLETE: {full_url}");
    Ok(session)
}
