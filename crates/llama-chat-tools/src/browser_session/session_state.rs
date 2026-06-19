//! Active browser session state — shared between calls, keyed by tab_id.

use std::collections::HashMap;
use std::sync::Mutex;

use super::tauri_session::TauriHttpSession;

#[derive(Clone, Default)]
struct TabState {
    url: Option<String>,
    cached_html: Option<String>,
    cached_text: Option<String>,
}

static TABS: Mutex<Option<HashMap<String, TabState>>> = Mutex::new(None);

fn with_tabs<R>(f: impl FnOnce(&mut HashMap<String, TabState>) -> R) -> R {
    let mut guard = TABS.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    f(map)
}

/// Clear cached HTML/text for one tab (e.g. after a click that may navigate).
pub fn clear_cache(tab_id: &str) {
    with_tabs(|tabs| {
        if let Some(state) = tabs.get_mut(tab_id) {
            state.cached_html = None;
            state.cached_text = None;
        }
    });
}

/// Get or create the active session for a tab.
pub fn current_session(tab_id: &str) -> Result<TauriHttpSession, String> {
    with_tabs(|tabs| {
        let state = tabs
            .get(tab_id)
            .ok_or("No active browser session. Call browser_navigate(url) first.")?;
        let url = state
            .url
            .clone()
            .ok_or("No active browser session. Call browser_navigate(url) first.")?;
        Ok(TauriHttpSession {
            tab_id: tab_id.to_string(),
            current_url: url,
            cached_html: state.cached_html.clone(),
            cached_text: state.cached_text.clone(),
        })
    })
}

/// Open a fresh session at the given URL for a tab.
pub fn open_session(url: &str, tab_id: &str) -> Result<TauriHttpSession, String> {
    let full_url = if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!("https://{url}")
    };
    let mut session = TauriHttpSession::open(tab_id, &full_url)?;
    eprintln!("[BROWSER_HTTP] open_session({tab_id}): fetching {full_url}");
    let fetch_start = std::time::Instant::now();
    // Fetch and cache immediately so subsequent reads are instant
    if let Ok(html) = session.do_fetch() {
        eprintln!(
            "[BROWSER_HTTP] fetch done ({}ms), stripping HTML ({} bytes)...",
            fetch_start.elapsed().as_millis(),
            html.len()
        );
        let strip_start = std::time::Instant::now();
        let text = TauriHttpSession::strip_html(&html);
        eprintln!(
            "[BROWSER_HTTP] strip_html done ({}ms), text={} bytes",
            strip_start.elapsed().as_millis(),
            text.len()
        );
        session.cached_html = Some(html.clone());
        session.cached_text = Some(text.clone());
        eprintln!("[BROWSER_HTTP] storing cache...");
        store_tab_state(tab_id, &full_url, Some(html), Some(text));
        eprintln!("[BROWSER_HTTP] cache stored");
    } else {
        store_tab_state(tab_id, &full_url, None, None);
    }
    eprintln!("[BROWSER_HTTP] open_session({tab_id}) COMPLETE: {full_url}");
    Ok(session)
}

/// Overwrite a tab's stored state (url + cache) — used after navigate/fetch.
pub(crate) fn store_tab_state(tab_id: &str, url: &str, html: Option<String>, text: Option<String>) {
    with_tabs(|tabs| {
        tabs.insert(
            tab_id.to_string(),
            TabState {
                url: Some(url.to_string()),
                cached_html: html,
                cached_text: text,
            },
        );
    });
}

/// Remove a tab's session entirely (e.g. after browser_close).
pub fn remove_session(tab_id: &str) {
    with_tabs(|tabs| {
        tabs.remove(tab_id);
    });
}
