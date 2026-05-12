//! Browser session abstraction — Tauri WebView with Chrome CDP fallback.

use serde_json::Value;
use std::sync::Mutex;

// ─── Chrome CDP fallback (visible browser for web mode) ────────

#[cfg(feature = "cdp")]
mod cdp {
    use std::sync::Mutex;
    use headless_chrome::{Browser, LaunchOptions, Tab};
    use std::sync::Arc;

    static CDP: Mutex<Option<(Browser, Arc<Tab>)>> = Mutex::new(None);

    /// Get or launch the visible Chrome browser. Returns the active tab.
    pub fn get_or_launch() -> Result<Arc<Tab>, String> {
        let mut guard = CDP.lock().map_err(|e| format!("CDP lock: {e}"))?;
        if let Some((_, ref tab)) = *guard {
            // Check tab is still alive by trying a trivial eval
            if tab.evaluate("1", false).is_ok() {
                return Ok(Arc::clone(tab));
            }
            eprintln!("[BROWSER_CDP] Existing tab is dead, relaunching...");
        }
        eprintln!("[BROWSER_CDP] Launching visible Chrome window...");
        let options = LaunchOptions {
            headless: false,
            window_size: Some((1280, 900)),
            sandbox: false,
            enable_logging: false,
            ..LaunchOptions::default()
        };
        let browser = Browser::new(options)
            .map_err(|e| format!("Chrome launch failed (is Chrome/Edge installed?): {e}"))?;
        let tab = browser.new_tab()
            .map_err(|e| format!("Chrome new tab: {e}"))?;
        let tab_clone = Arc::clone(&tab);
        *guard = Some((browser, tab));
        eprintln!("[BROWSER_CDP] Chrome launched successfully");
        Ok(tab_clone)
    }

    pub fn navigate(url: &str) -> Result<(), String> {
        let tab = get_or_launch()?;
        tab.navigate_to(url)
            .map_err(|e| format!("CDP navigate: {e}"))?;
        tab.wait_until_navigated()
            .map_err(|e| format!("CDP wait: {e}"))?;
        Ok(())
    }

    pub fn evaluate(js: &str) -> Result<String, String> {
        let tab = get_or_launch()?;
        let result = tab.evaluate(js, false)
            .map_err(|e| format!("CDP eval: {e}"))?;
        match result.value {
            Some(serde_json::Value::String(s)) => Ok(s),
            Some(v) => Ok(v.to_string()),
            None => Ok(String::new()),
        }
    }

    pub fn close() -> Result<(), String> {
        let mut guard = CDP.lock().map_err(|e| format!("CDP lock: {e}"))?;
        if guard.is_some() {
            eprintln!("[BROWSER_CDP] Closing Chrome");
        }
        *guard = None; // Drop kills Chrome process
        Ok(())
    }
}

/// Check if the Tauri UI bridge is available (desktop mode).
/// Caches result for 30 seconds to avoid constant probing.
fn is_tauri_available() -> bool {
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

const TAURI_UI_BRIDGE_BASE: &str = "http://127.0.0.1:18091";

// ─── Tauri HTTP Session ─────────────────────────────────────────

/// Browser session that opens the Tauri native webview (user sees the page)
/// and reads content via HTTP (ureq). No external browser server needed.
/// Page content is cached on navigate — reads are instant.
pub struct TauriHttpSession {
    pub current_url: String,
    cached_html: Option<String>,
    cached_text: Option<String>,
}

impl TauriHttpSession {
    pub fn open(url: &str) -> Result<Self, String> {
        let _ = notify_tauri_browser_navigate(url);
        Ok(Self {
            current_url: url.to_string(),
            cached_html: None,
            cached_text: None,
        })
    }

    /// Fetch page and cache both HTML and text.
    #[allow(dead_code)]
    fn prefetch(&mut self) -> Result<(), String> {
        let html = self.do_fetch()?;
        let text = Self::strip_html(&html);
        self.cached_html = Some(html);
        self.cached_text = Some(text);
        Ok(())
    }

    /// Fast HTML tag stripper — no regex, pure iteration. Handles script/style removal.
    fn strip_html(html: &str) -> String {
        let mut result = String::with_capacity(html.len() / 3);
        let mut in_tag = false;
        let mut in_script = false;
        let lower = html.to_lowercase();
        let bytes = html.as_bytes();
        let lower_bytes = lower.as_bytes();
        let len = bytes.len();
        let mut i = 0;

        while i < len {
            if in_script {
                // Look for </script> or </style>
                if i + 8 < len && lower_bytes[i] == b'<' && lower_bytes[i + 1] == b'/' {
                    if lower[i..].starts_with("</script>") {
                        i += 9;
                        in_script = false;
                        continue;
                    }
                    if lower[i..].starts_with("</style>") {
                        i += 8;
                        in_script = false;
                        continue;
                    }
                }
                i += 1;
                continue;
            }
            if bytes[i] == b'<' {
                // Check for <script or <style
                if i + 7 < len
                    && (lower[i..].starts_with("<script") || lower[i..].starts_with("<style"))
                {
                    in_script = true;
                }
                in_tag = true;
                i += 1;
                continue;
            }
            if bytes[i] == b'>' && in_tag {
                in_tag = false;
                result.push(' ');
                i += 1;
                continue;
            }
            if !in_tag {
                result.push(bytes[i] as char);
            }
            i += 1;
        }

        // Collapse whitespace + decode entities
        let collapsed: String = result.split_whitespace().collect::<Vec<_>>().join(" ");
        collapsed
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&nbsp;", " ")
    }

    /// Fetch page HTML via the Tauri webview (reads from real browser, bypasses bot detection).
    /// Falls back to curl if the webview eval endpoint isn't available.
    pub fn do_fetch(&self) -> Result<String, String> {
        eprintln!("[BROWSER_HTTP] do_fetch START: {}", self.current_url);
        let start = std::time::Instant::now();

        // Wait for page to fully load by polling document.readyState.
        // IMPORTANT: wry's load_url() is fire-and-forget — it returns before the browser
        // has started navigating. We must first wait for window.location.href to reflect
        // the new URL, otherwise we'd read content from the *previous* page still cached
        // in the WebView (which has readyState="complete" and content > 50 chars).
        let max_wait = std::time::Duration::from_secs(15);
        let poll_interval = std::time::Duration::from_millis(400);
        let mut ready = false;

        // Normalize expected URL for comparison (strip trailing slash, lowercase scheme+host)
        let expected = self.current_url.trim_end_matches('/').to_lowercase();
        // Extract just the host+path portion for flexible matching (handles http↔https, www differences)
        let expected_path = expected
            .find("//")
            .map(|i| &expected[i + 2..])
            .unwrap_or(&expected);

        while start.elapsed() < max_wait {
            // Phase 1: ensure browser has navigated to the right URL.
            // Skip this check for Tauri (Tauri bridge handles this synchronously).
            #[cfg(feature = "wry-browser")]
            if !is_tauri_available() {
                match eval_in_browser_panel("window.location.href") {
                    Ok(href) => {
                        let href_norm = href.trim().trim_end_matches('/').to_lowercase();
                        let href_path = href_norm
                            .find("//")
                            .map(|i| &href_norm[i + 2..])
                            .unwrap_or(&href_norm);
                        // Accept if path portion matches or if browser is on about:blank (initial)
                        if href_path != expected_path && !href_norm.starts_with("about:") {
                            eprintln!("[BROWSER_HTTP] waiting for URL: expected={expected_path}, got={href_path}");
                            std::thread::sleep(poll_interval);
                            continue;
                        }
                    }
                    Err(_) => {
                        std::thread::sleep(poll_interval);
                        continue;
                    }
                }
            }

            // Phase 2: wait for readyState + content
            match eval_in_browser_panel("document.readyState") {
                Ok(state) if state == "complete" || state == "interactive" => {
                    // Also check we have actual content (not just empty shell)
                    if let Ok(len) = eval_in_browser_panel("document.body?.innerText?.length || 0") {
                        if let Ok(n) = len.parse::<usize>() {
                            if n > 50 {
                                ready = true;
                                eprintln!("[BROWSER_HTTP] page ready: readyState={state}, text={n} chars ({:.1}s)",
                                    start.elapsed().as_secs_f64());
                                break;
                            }
                        }
                    }
                }
                _ => {}
            }
            std::thread::sleep(poll_interval);
        }
        if !ready {
            eprintln!("[BROWSER_HTTP] page not ready after {:.1}s, proceeding anyway",
                start.elapsed().as_secs_f64());
        }

        // Auto-dismiss common cookie/consent banners so they don't pollute page text.
        // Run twice with a delay — CMPs (OneTrust, Cookiebot, etc.) load asynchronously
        // after readyState=complete, so the first pass may fire before the button exists.
        let cookie_js = r#"
            (() => {
                const patterns = [
                    // OneTrust (used by insidehighered, many news sites)
                    '#onetrust-accept-btn-handler', '.onetrust-accept-btn-handler',
                    '.ot-sdk-btn-handler', '#accept-recommended-btn-handler',
                    // Cookiebot
                    '#CybotCookiebotDialogBodyLevelButtonLevelOptinAllowAll',
                    '#CybotCookiebotDialogBodyButtonAccept',
                    // TrustArc / Evidon / other CMPs
                    '.truste_popframe', '#truste-consent-button', '.evidon-accept-button',
                    '#gdpr-consent-tool-wrapper button',
                    // By ID/class containing accept/agree/consent
                    'button[id*="accept" i]', 'button[class*="accept" i]',
                    'button[id*="agree" i]', 'button[class*="agree" i]',
                    'button[id*="consent" i]', 'button[class*="consent" i]',
                    // Data attributes
                    'button[data-testid*="accept" i]', 'button[data-testid*="agree" i]',
                    '[data-gdpr*="accept" i]', '[data-consent*="accept" i]',
                    // Aria labels
                    '[aria-label*="Accept" i]', '[aria-label*="Agree" i]',
                    '[aria-label*="Allow all" i]', '[aria-label*="Allow cookies" i]',
                    // Common class names
                    '#accept-cookies', '#cookie-accept', '.cookie-accept',
                    '.js-accept-cookies', '.accept-cookies', '.accept-all', '#acceptAll',
                    // Text-based: buttons whose visible text matches common patterns
                    ...Array.from(document.querySelectorAll('button, [role="button"], a.btn'))
                        .filter(el => /^(accept|agree|allow|got it|ok|i agree|accept all|allow all|accept cookies|accept & continue|accept and continue)/i.test((el.innerText||'').trim()))
                        .slice(0, 5)
                ];
                for (const el of patterns) {
                    try {
                        const target = typeof el === 'string' ? document.querySelector(el) : el;
                        if (target && target.offsetParent !== null) {
                            target.click();
                            return 'dismissed: ' + (typeof el === 'string' ? el : target.innerText?.trim());
                        }
                    } catch(_) {}
                }
                return 'no banner found';
            })()
        "#;
        let _ = eval_in_browser_panel(cookie_js);
        // Second pass after 1.5s — CMPs often render after initial page load
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let _ = eval_in_browser_panel(cookie_js);

        // Read the page HTML via eval_in_browser_panel (uses Tauri → wry → CDP fallback chain)
        match eval_in_browser_panel("document.documentElement ? document.documentElement.outerHTML : ''") {
            Ok(html) if html.len() >= 50 || html.contains('<') => {
                eprintln!("[BROWSER_HTTP] eval OK: {} bytes ({:.1}s)",
                    html.len(), start.elapsed().as_secs_f64());
                let max = 500_000;
                if html.len() > max {
                    return Ok(html[..max].to_string());
                }
                return Ok(html);
            }
            Ok(html) => {
                eprintln!("[BROWSER_HTTP] eval returned non-HTML ({} bytes), falling back to curl", html.len());
            }
            Err(e) => {
                eprintln!("[BROWSER_HTTP] eval failed: {e}, falling back to curl");
            }
        }

        Self::curl_fetch(&self.current_url)
    }

    /// Fallback: fetch via curl (for web mode where Tauri webview isn't available).
    fn curl_fetch(url: &str) -> Result<String, String> {
        let mut cmd = std::process::Command::new("curl");
        cmd.args(["-sL", "--max-time", "15",
                "-A", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
                url])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        let output = cmd.output().map_err(|e| format!("curl failed: {e}"))?;
        if !output.status.success() {
            return Err(format!("curl status {}", output.status));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Get text — from cache (instant) or fetch.
    fn get_text(&self, max_chars: usize) -> Result<String, String> {
        let text = match &self.cached_text {
            Some(t) => t.clone(),
            None => {
                let html = self.do_fetch()?;
                Self::strip_html(&html)
            }
        };
        if text.len() > max_chars {
            let mut end = max_chars;
            while end > 0 && !text.is_char_boundary(end) { end -= 1; }
            Ok(format!("{}...\n[Truncated]", &text[..end]))
        } else {
            Ok(text)
        }
    }

    /// Get HTML — from cache (instant) or fetch.
    fn get_html(&self) -> Result<String, String> {
        match &self.cached_html {
            Some(h) => Ok(h.clone()),
            None => self.do_fetch(),
        }
    }
}

impl BrowserSession for TauriHttpSession {
    fn navigate(&mut self, url: &str) -> Result<(), String> {
        eprintln!("[BROWSER_HTTP] navigate: {url}");
        self.current_url = url.to_string();
        self.cached_html = None;
        self.cached_text = None;
        // Clear static cache too
        if let Ok(mut g) = CACHED_HTML.lock() { *g = None; }
        if let Ok(mut g) = CACHED_TEXT.lock() { *g = None; }
        if let Ok(mut g) = ACTIVE_URL.lock() { *g = Some(url.to_string()); }
        let _ = notify_tauri_browser_navigate(url);
        // Fetch and cache the new page
        if let Ok(html) = self.do_fetch() {
            let text = Self::strip_html(&html);
            eprintln!("[BROWSER_HTTP] navigate fetched {} bytes, text {} bytes", html.len(), text.len());
            self.cached_html = Some(html.clone());
            self.cached_text = Some(text.clone());
            if let Ok(mut g) = CACHED_HTML.lock() { *g = Some(html); }
            if let Ok(mut g) = CACHED_TEXT.lock() { *g = Some(text); }
        }
        Ok(())
    }

    fn click(&self, selector: &str) -> Result<(), String> {
        // Try Tauri CDP Input.dispatchMouseEvent first (works on React SPAs)
        if is_tauri_available() {
            let url = format!("{TAURI_UI_BRIDGE_BASE}/api/browser/click");
            let body = serde_json::json!({
                "selector": selector,
                "target": "browser-panel"
            });
            match ureq::post(&url)
                .set("Content-Type", "application/json")
                .timeout(std::time::Duration::from_secs(10))
                .send_string(&body.to_string())
            {
                Ok(resp) => {
                    let text = resp.into_string().unwrap_or_default();
                    if !text.contains("not found") && !text.contains("not open") {
                        return Ok(());
                    }
                }
                Err(_) => {} // Fall through to CDP
            }
        }

        // Chrome CDP fallback: use JS click
        #[cfg(feature = "cdp")]
        {
            let sel_json = serde_json::to_string(selector).unwrap_or_default();
            let js = format!(
                "(() => {{ const el = document.querySelector({sel_json}); if (!el) return 'not found'; el.click(); return 'clicked'; }})()"
            );
            match cdp::evaluate(&js) {
                Ok(r) if !r.contains("not found") => return Ok(()),
                Ok(r) => return Err(format!("Element not found: {r}")),
                Err(e) => return Err(format!("CDP click failed: {e}")),
            }
        }

        #[cfg(not(feature = "cdp"))]
        Err("No browser backend available for click".into())
    }

    fn type_text(&self, selector: &str, text: &str, press_enter: bool) -> Result<(), String> {
        let enter_js = if press_enter {
            r#"el.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',code:'Enter',keyCode:13,bubbles:true}));"#
        } else {
            ""
        };
        let js = format!(
            r#"(() => {{
                const el = document.querySelector({sel});
                if (!el) return 'Element not found: ' + {sel};
                el.focus();
                el.value = {val};
                el.dispatchEvent(new Event('input', {{bubbles:true}}));
                el.dispatchEvent(new Event('change', {{bubbles:true}}));
                {enter}
                return 'typed';
            }})()"#,
            sel = serde_json::to_string(selector).unwrap_or_default(),
            val = serde_json::to_string(text).unwrap_or_default(),
            enter = enter_js
        );
        match eval_in_browser_panel(&js) {
            Ok(r) if r.contains("not found") => Err(r),
            Ok(_) => Ok(()),
            Err(e) => Err(format!("type failed: {e}")),
        }
    }

    fn eval(&self, js: &str) -> Result<Value, String> {
        let result = eval_in_browser_panel(js)?;
        // Parse as JSON; if it fails, return as a plain string (not an error).
        // eval_in_browser_panel double-unwraps JSON encoding, so string results
        // arrive as plain text which isn't valid JSON — that's fine.
        Ok(serde_json::from_str(&result).unwrap_or(Value::String(result)))
    }

    fn html(&self) -> Result<String, String> {
        self.get_html()
    }

    fn screenshot(&self) -> Result<Vec<u8>, String> {
        Err("Screenshot not supported in Tauri HTTP mode.".into())
    }

    fn wait_for(&self, selector: &str, timeout_ms: u64) -> Result<bool, String> {
        let js = format!(
            r#"!!document.querySelector({sel})"#,
            sel = serde_json::to_string(selector).unwrap_or_default()
        );
        let max_polls = (timeout_ms / 500).max(1);
        for _ in 0..max_polls {
            if let Ok(r) = eval_in_browser_panel(&js) {
                if r.contains("true") { return Ok(true); }
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        Ok(false)
    }

    fn press_key(&self, key: &str) -> Result<(), String> {
        let js = format!(
            r#"(() => {{
                const el = document.activeElement || document.body;
                el.dispatchEvent(new KeyboardEvent('keydown', {{key:{k}, bubbles:true}}));
                el.dispatchEvent(new KeyboardEvent('keyup', {{key:{k}, bubbles:true}}));
                return 'pressed';
            }})()"#,
            k = serde_json::to_string(key).unwrap_or_default()
        );
        eval_in_browser_panel(&js).map(|_| ())
    }

    fn snapshot(&self) -> Result<String, String> {
        // Use cached text if available (populated by navigate/do_fetch)
        if let Some(ref text) = self.cached_text {
            if text.len() > 50 {
                return self.get_text(20_000);
            }
        }
        // No cache — read directly from webview (after click navigation, etc.)
        match eval_in_browser_panel("document.body.innerText") {
            Ok(text) if text.len() > 50 => {
                let max = 20_000;
                if text.len() > max {
                    let mut end = max;
                    while end > 0 && !text.is_char_boundary(end) { end -= 1; }
                    Ok(format!("{}...\n[Truncated]", &text[..end]))
                } else {
                    Ok(text)
                }
            }
            _ => self.get_text(20_000),
        }
    }

    fn get_full_text(&self, offset: usize, max_chars: usize) -> Result<String, String> {
        // Get full text (from cache or live webview eval — no pre-truncation)
        let full = if let Some(ref t) = self.cached_text {
            t.clone()
        } else {
            match eval_in_browser_panel("document.body.innerText") {
                Ok(t) if t.len() > 50 => t,
                _ => {
                    let html = self.do_fetch()?;
                    Self::strip_html(&html)
                }
            }
        };

        let total = full.len();
        if offset >= total {
            return Ok(format!("[offset {offset} is past end of page ({total} chars total)]"));
        }

        // Align to char boundary
        let mut start = offset;
        while start > 0 && !full.is_char_boundary(start) { start -= 1; }

        let mut end = (start + max_chars).min(total);
        while end < total && !full.is_char_boundary(end) { end += 1; }

        let slice = &full[start..end];
        if end < total {
            let remaining = total - end;
            let next_offset = end;
            Ok(format!("{slice}\n\n[{remaining} chars remaining — call browser_get_text(offset={next_offset}) to continue]"))
        } else {
            Ok(slice.to_string())
        }
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

/// The active session state — shared between calls.
static ACTIVE_URL: Mutex<Option<String>> = Mutex::new(None);
static CACHED_HTML: Mutex<Option<String>> = Mutex::new(None);
static CACHED_TEXT: Mutex<Option<String>> = Mutex::new(None);

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

/// Execute JavaScript in the browser panel — Tauri WebView or Chrome CDP fallback.
pub fn eval_in_browser_panel(js: &str) -> Result<String, String> {
    // Try Tauri WebView first
    if is_tauri_available() {
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
                        break; // Fall through to CDP
                    }
                    if body.starts_with('"') && body.ends_with('"') {
                        return Ok(serde_json::from_str::<String>(&body).unwrap_or(body));
                    }
                    return Ok(body);
                }
                Err(_) => break, // Fall through to CDP
            }
        }
    }

    // Try wry native WebView
    #[cfg(feature = "wry-browser")]
    {
        match crate::wry_browser::evaluate(js) {
            Ok(result) => return Ok(result),
            Err(e) => eprintln!("[BROWSER_WRY] eval failed: {e}"),
        }
    }

    // Try Chrome CDP
    #[cfg(feature = "cdp")]
    {
        match cdp::evaluate(js) {
            Ok(result) => return Ok(result),
            Err(e) => eprintln!("[BROWSER_CDP] eval failed: {e}"),
        }
    }

    Err("eval_in_browser_panel: no backend available".into())
}

/// Navigate the visible browser panel — Tauri WebView or Chrome CDP fallback.
pub fn notify_tauri_browser_navigate(url: &str) -> Result<(), String> {
    eprintln!("[BROWSER_HTTP] notify_tauri_browser_navigate: {url}");

    // Try Tauri WebView first
    if is_tauri_available() {
        let body = serde_json::json!({ "url": url });
        if ureq::post(&format!("{TAURI_UI_BRIDGE_BASE}/bridge/browser/navigate"))
            .set("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(3))
            .send_string(&body.to_string())
            .is_ok()
        {
            return Ok(());
        }
    }

    // Try wry native WebView
    #[cfg(feature = "wry-browser")]
    {
        match crate::wry_browser::navigate(url) {
            Ok(()) => {
                eprintln!("[BROWSER_WRY] navigated to {url}");
                return Ok(());
            }
            Err(e) => eprintln!("[BROWSER_WRY] navigate failed: {e}"),
        }
    }

    // Try Chrome CDP
    #[cfg(feature = "cdp")]
    {
        match cdp::navigate(url) {
            Ok(()) => {
                eprintln!("[BROWSER_CDP] navigated to {url}");
                return Ok(());
            }
            Err(e) => eprintln!("[BROWSER_CDP] navigate failed: {e}"),
        }
    }

    Err("No browser backend available (Tauri WebView not running, Chrome not found)".into())
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

    // Also close CDP if active
    #[cfg(feature = "cdp")]
    { let _ = cdp::close(); }

    Ok(())
}
