//! Browser session abstraction — unified API for agent browser tools.
//!
//! Two backends:
//! - TauriHttpSession (default): opens Tauri native webview for the user,
//!   reads content via HTTP (ureq). No Camofox needed.
//! - CamofoxSession (fallback): headless Firefox for anti-detection browsing.

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

        // Wait for page to load in the webview (navigated by caller)
        std::thread::sleep(std::time::Duration::from_millis(2000));

        // Read the page HTML from the browser panel webview via eval REST endpoint
        // Timeout is short — if eval fails, we fall back to curl quickly
        let eval_result = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(12))
            .build()
            .post(&format!("{TAURI_UI_BRIDGE_BASE}/api/eval"))
            .set("Content-Type", "application/json")
            .send_string(&serde_json::json!({
                "js": "document.documentElement ? document.documentElement.outerHTML : ''",
                "target": "agent-browser"
            }).to_string());

        let body = match eval_result {
            Ok(resp) => resp.into_string().unwrap_or_default(),
            Err(e) => {
                eprintln!("[BROWSER_HTTP] webview eval HTTP error: {e}, falling back to curl");
                return Self::curl_fetch(&self.current_url);
            }
        };

        // Check for eval failures (timeout, panel not open, etc.)
        if body.contains("timed out") || body.contains("not open") || body.contains("not found") || body.contains("eval failed") {
            eprintln!("[BROWSER_HTTP] webview eval returned error: {body}, falling back to curl");
            return Self::curl_fetch(&self.current_url);
        }

        // Unwrap the JSON string wrapper from eval result
        let html = if body.starts_with('"') && body.ends_with('"') {
            serde_json::from_str::<String>(&body).unwrap_or(body)
        } else {
            body
        };

        // Sanity check: result should look like HTML (not a short error message)
        if html.len() < 50 && !html.contains('<') {
            eprintln!("[BROWSER_HTTP] webview eval returned non-HTML: {html}, falling back to curl");
            return Self::curl_fetch(&self.current_url);
        }

        eprintln!("[BROWSER_HTTP] webview eval OK: {}bytes ({}ms)",
            html.len(), start.elapsed().as_millis());
        let max = 500_000;
        if html.len() > max {
            Ok(html[..max].to_string())
        } else {
            Ok(html)
        }
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
        let js = format!(
            r#"(() => {{
                const el = document.querySelector({sel});
                if (!el) return 'Element not found: ' + {sel};
                el.click();
                return 'clicked';
            }})()"#,
            sel = serde_json::to_string(selector).unwrap_or_default()
        );
        match eval_in_browser_panel(&js) {
            Ok(r) if r.contains("not found") => Err(r),
            Ok(_) => Ok(()),
            Err(e) => Err(format!("click failed: {e}")),
        }
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

/// Execute JavaScript in the browser-panel webview via the MCP bridge REST endpoint.
pub fn eval_in_browser_panel(js: &str) -> Result<String, String> {
    // Retry up to 3 times — COM callback can fail during heavy generation
    for attempt in 0..3 {
        let resp = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .post(&format!("{TAURI_UI_BRIDGE_BASE}/api/eval"))
            .set("Content-Type", "application/json")
            .send_string(&serde_json::json!({
                "js": js,
                "target": "agent-browser"
            }).to_string())
            .map_err(|e| format!("eval bridge failed: {e}"))?;
        let body = resp.into_string().unwrap_or_default();

        // Retry on COM channel failures (webview busy during generation)
        if body.contains("Result channel closed") || body.contains("eval timed out") {
            if attempt < 2 {
                eprintln!("[BROWSER_EVAL] attempt {}: {}, retrying...", attempt + 1, body.trim());
                std::thread::sleep(std::time::Duration::from_millis(1000));
                continue;
            }
            return Err(body);
        }

        // Unwrap JSON string wrapper
        if body.starts_with('"') && body.ends_with('"') {
            return Ok(serde_json::from_str::<String>(&body).unwrap_or(body));
        }
        return Ok(body);
    }
    Err("eval_in_browser_panel: all retries failed".into())
}

/// Best-effort: ask the Tauri app process to open/navigate the visible native browser panel.
/// This bridge is only available in desktop mode; callers should ignore failures and continue.
pub fn notify_tauri_browser_navigate(url: &str) -> Result<(), String> {
    eprintln!("[BROWSER_HTTP] notify_tauri_browser_navigate: {url}");
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
