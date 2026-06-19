//! TauriHttpSession — browser session via Tauri native webview + HTTP read.

use serde_json::Value;

use super::backends::{
    close_browser_tab, eval_in_browser_tab, is_tauri_available, navigate_browser_tab, tab_label,
    TAURI_UI_BRIDGE_BASE,
};
use super::session_state::{remove_session, store_tab_state};
use super::BrowserSession;

/// Browser session that opens the Tauri native webview (user sees the page)
/// and reads content via HTTP (ureq). No external browser server needed.
/// Page content is cached on navigate — reads are instant.
pub struct TauriHttpSession {
    pub tab_id: String,
    pub current_url: String,
    pub(crate) cached_html: Option<String>,
    pub(crate) cached_text: Option<String>,
}

impl TauriHttpSession {
    pub fn open(tab_id: &str, url: &str) -> Result<Self, String> {
        let _ = navigate_browser_tab(url, tab_id);
        Ok(Self {
            tab_id: tab_id.to_string(),
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
    pub(crate) fn strip_html(html: &str) -> String {
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
        let _expected_path = expected
            .find("//")
            .map(|i| &expected[i + 2..])
            .unwrap_or(&expected);

        while start.elapsed() < max_wait {
            // Phase 1: ensure browser has navigated to the right URL.
            // Skip this check for Tauri (Tauri bridge handles this synchronously).
            #[cfg(feature = "wry-browser")]
            if !is_tauri_available() {
                match eval_in_browser_tab("window.location.href", &self.tab_id) {
                    Ok(href) => {
                        let href_norm = href.trim().trim_end_matches('/').to_lowercase();
                        let href_path = href_norm
                            .find("//")
                            .map(|i| &href_norm[i + 2..])
                            .unwrap_or(&href_norm);
                        // Accept if path portion matches or if browser is on about:blank (initial)
                        if href_path != _expected_path && !href_norm.starts_with("about:") {
                            eprintln!("[BROWSER_HTTP] waiting for URL: expected={_expected_path}, got={href_path}");
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
            match eval_in_browser_tab("document.readyState", &self.tab_id) {
                Ok(state) if state == "complete" || state == "interactive" => {
                    // Also check we have actual content (not just empty shell)
                    if let Ok(len) = eval_in_browser_tab("document.body?.innerText?.length || 0", &self.tab_id) {
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
        let _ = eval_in_browser_tab(cookie_js, &self.tab_id);
        // Second pass after 1.5s — CMPs often render after initial page load
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let _ = eval_in_browser_tab(cookie_js, &self.tab_id);

        // Read the page HTML via eval_in_browser_tab (Tauri WebView or wry window)
        match eval_in_browser_tab("document.documentElement ? document.documentElement.outerHTML : ''", &self.tab_id) {
            Ok(html) if html.len() >= 50 || html.contains('<') => {
                eprintln!("[BROWSER_HTTP] eval OK: {} bytes ({:.1}s)",
                    html.len(), start.elapsed().as_secs_f64());
                let max = 500_000;
                if html.len() > max {
                    return Ok(html[..max].to_string());
                }
                Ok(html)
            }
            Ok(html) => {
                eprintln!("[BROWSER_HTTP] eval returned short/non-HTML ({} bytes)", html.len());
                Err(format!("Browser eval returned no content ({} bytes) — page may not have loaded yet", html.len()))
            }
            Err(e) => {
                eprintln!("[BROWSER_HTTP] eval failed: {e}");
                Err(format!("Browser eval failed: {e}"))
            }
        }
    }

    /// Get text — from cache (instant) or fetch.
    pub(crate) fn get_text(&self, max_chars: usize) -> Result<String, String> {
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
        eprintln!("[BROWSER_HTTP] navigate({}): {url}", self.tab_id);
        self.current_url = url.to_string();
        self.cached_html = None;
        self.cached_text = None;
        let _ = navigate_browser_tab(url, &self.tab_id);
        // Fetch and cache the new page
        if let Ok(html) = self.do_fetch() {
            let text = Self::strip_html(&html);
            eprintln!("[BROWSER_HTTP] navigate fetched {} bytes, text {} bytes", html.len(), text.len());
            self.cached_html = Some(html.clone());
            self.cached_text = Some(text.clone());
            store_tab_state(&self.tab_id, url, Some(html), Some(text));
        } else {
            store_tab_state(&self.tab_id, url, None, None);
        }
        Ok(())
    }

    fn click(&self, selector: &str) -> Result<(), String> {
        // Try Tauri CDP Input.dispatchMouseEvent first (works on React SPAs)
        if is_tauri_available() {
            let url = format!("{TAURI_UI_BRIDGE_BASE}/api/browser/click");
            let body = serde_json::json!({
                "selector": selector,
                "target": tab_label(&self.tab_id)
            });
            if let Ok(resp) = ureq::post(&url)
                .set("Content-Type", "application/json")
                .timeout(std::time::Duration::from_secs(10))
                .send_string(&body.to_string())
            {
                let text = resp.into_string().unwrap_or_default();
                if !text.contains("not found") && !text.contains("not open") {
                    return Ok(());
                }
            }
        }

        // JS click fallback via the existing browser panel (wry/CDP — whatever is open)
        {
            let sel_json = serde_json::to_string(selector).unwrap_or_default();
            let js = format!(
                "(() => {{ const el = document.querySelector({sel_json}); if (!el) return 'not found'; el.click(); return 'clicked'; }})()"
            );
            match eval_in_browser_tab(&js, &self.tab_id) {
                Ok(r) if !r.contains("not found") => Ok(()),
                Ok(r) => Err(format!("Element not found: {r}")),
                Err(e) => Err(format!("click failed: {e}")),
            }
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
        match eval_in_browser_tab(&js, &self.tab_id) {
            Ok(r) if r.contains("not found") => Err(r),
            Ok(_) => Ok(()),
            Err(e) => Err(format!("type failed: {e}")),
        }
    }

    fn eval(&self, js: &str) -> Result<Value, String> {
        let result = eval_in_browser_tab(js, &self.tab_id)?;
        // Parse as JSON; if it fails, return as a plain string (not an error).
        // eval_in_browser_tab double-unwraps JSON encoding, so string results
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
            if let Ok(r) = eval_in_browser_tab(&js, &self.tab_id) {
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
        eval_in_browser_tab(&js, &self.tab_id).map(|_| ())
    }

    fn snapshot(&self) -> Result<String, String> {
        // Use cached text if available (populated by navigate/do_fetch)
        if let Some(ref text) = self.cached_text {
            if text.len() > 50 {
                return self.get_text(20_000);
            }
        }
        // No cache — read directly from webview (after click navigation, etc.)
        match eval_in_browser_tab("document.body.innerText", &self.tab_id) {
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
            match eval_in_browser_tab("document.body.innerText", &self.tab_id) {
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
        let _ = close_browser_tab(&self.tab_id);
        remove_session(&self.tab_id);
        Ok(())
    }

    fn url(&self) -> &str {
        &self.current_url
    }
}
