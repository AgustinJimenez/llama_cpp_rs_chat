//! Browser panel tool handlers and REST bridge endpoints.
//!
//! Covers all `browser_*` tool dispatch cases plus the HTTP bridge routes
//! (`/bridge/browser/navigate`, `/bridge/browser/close`) that allow non-MCP
//! callers to control the embedded browser webview.

use axum::{body::Bytes, extract::State};
use serde_json::Value;
use tauri::{AppHandle, Manager};

use super::cdp::{capture_webview_screenshot, cdp_click};
use super::eval::eval_js_in;

// ─── Shared helpers ───────────────────────────────────────────────

pub async fn open_browser_view_js(app: &AppHandle, url: &str) -> Result<String, String> {
    let js = format!(
        r#"(() => {{
            if (window.__openBrowserView) {{
                window.__openBrowserView({url});
                return 'Browser view opened: ' + {url};
            }}
            return 'openBrowserView not available';
        }})()"#,
        url = serde_json::to_string(url).unwrap()
    );
    eval_js_in(app, &js, "main").await
}

pub async fn close_browser_view_js(app: &AppHandle) -> Result<String, String> {
    let js = r#"(() => {
        if (window.__closeBrowserView) {
            window.__closeBrowserView();
            return 'Browser view closed';
        }
        return 'closeBrowserView not available';
    })()"#;
    eval_js_in(app, js, "main").await
}

// ─── MCP tool dispatch cases ──────────────────────────────────────

/// Handle all `browser_*` MCP tool calls.
/// Returns `None` if the tool name is not a browser tool.
pub async fn dispatch_browser_tool(
    app: &AppHandle,
    name: &str,
    args: &Value,
) -> Option<Result<String, String>> {
    match name {
        "browser_navigate" => {
            let url = match args.get("url").and_then(|v| v.as_str()) {
                Some(u) => u,
                None => return Some(Err("'url' is required".into())),
            };
            let full_url = if url.starts_with("http://") || url.starts_with("https://") {
                url.to_string()
            } else {
                format!("https://{url}")
            };
            let parsed = match full_url.parse::<tauri::Url>() {
                Ok(p) => p,
                Err(e) => return Some(Err(format!("Invalid URL: {e}"))),
            };
            if let Some(existing) = app.webviews().get("agent-browser").cloned() {
                if let Err(e) = existing.navigate(parsed) {
                    return Some(Err(format!("Navigate failed: {e}")));
                }
            } else if let Some(window) = app.get_window("main") {
                let builder = tauri::webview::WebviewBuilder::new(
                    "agent-browser",
                    tauri::WebviewUrl::External(parsed),
                );
                if let Err(e) = window.add_child(
                    builder,
                    tauri::LogicalPosition::new(0.0, 0.0),
                    tauri::LogicalSize::new(800.0, 0.0),
                ) {
                    return Some(Err(format!("Failed to create browser: {e}")));
                }
            } else {
                return Some(Err("Main window not found".into()));
            }
            // Wait briefly for page to start loading
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            Some(Ok(format!("Navigated to {full_url}")))
        }

        "browser_read" => {
            let selector = args.get("selector").and_then(|v| v.as_str());
            let max_len = args.get("max_length").and_then(|v| v.as_u64()).unwrap_or(30000);
            let js = if let Some(sel) = selector {
                format!(
                    r#"(() => {{
                        const el = document.querySelector({sel});
                        if (!el) return 'Element not found: ' + {sel};
                        return (el.innerText || el.textContent || '').trim().slice(0, {max_len});
                    }})()"#,
                    sel = serde_json::to_string(sel).unwrap(),
                )
            } else {
                format!(
                    r#"(() => {{
                        const body = document.body;
                        if (!body) return 'No body element';
                        return body.innerText.slice(0, {max_len});
                    }})()"#,
                )
            };
            Some(eval_js_in(app, &js, "agent-browser").await)
        }

        "browser_click" => {
            let sel = match args.get("selector").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => return Some(Err("'selector' is required".into())),
            };
            Some(cdp_click(app, "agent-browser", sel).await)
        }

        "browser_type" => {
            let sel = match args.get("selector").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => return Some(Err("'selector' is required".into())),
            };
            let text = match args.get("text").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => return Some(Err("'text' is required".into())),
            };
            let submit = args.get("submit").and_then(|v| v.as_bool()).unwrap_or(false);
            let js = format!(
                r#"(() => {{
                    const el = document.querySelector({sel});
                    if (!el) return 'Element not found: ' + {sel};
                    el.focus();
                    el.value = {text};
                    el.dispatchEvent(new Event('input', {{ bubbles: true }}));
                    el.dispatchEvent(new Event('change', {{ bubbles: true }}));
                    {submit_js}
                    return 'Typed into ' + {sel};
                }})()"#,
                sel = serde_json::to_string(sel).unwrap(),
                text = serde_json::to_string(text).unwrap(),
                submit_js = if submit {
                    r#"const form = el.closest('form');
                    if (form) form.requestSubmit();
                    else {
                        const btn = document.querySelector('button[type="submit"], input[type="submit"]');
                        if (btn) setTimeout(() => btn.click(), 100);
                    }"#
                } else { "" }
            );
            Some(eval_js_in(app, &js, "agent-browser").await)
        }

        "browser_eval" => {
            let js = match args.get("js").and_then(|v| v.as_str()) {
                Some(j) => j,
                None => return Some(Err("'js' is required".into())),
            };
            Some(eval_js_in(app, js, "agent-browser").await)
        }

        "browser_get_url" => {
            let webviews = app.webviews();
            if let Some(wv) = webviews.get("agent-browser").cloned() {
                let url = wv.url().map(|u| u.to_string()).unwrap_or_default();
                Some(Ok(url))
            } else {
                Some(Err(
                    "Browser panel not open. Use browser_navigate first.".into()
                ))
            }
        }

        "browser_list_links" => {
            let filter = args.get("filter").and_then(|v| v.as_str()).unwrap_or("");
            let js = format!(
                r#"(() => {{
                    const links = Array.from(document.querySelectorAll('a[href]'));
                    const filter = {filter}.toLowerCase();
                    return JSON.stringify(links
                        .map(a => ({{
                            text: (a.textContent || '').trim().slice(0, 80),
                            href: a.href,
                        }}))
                        .filter(l => l.text && (!filter || l.text.toLowerCase().includes(filter) || l.href.toLowerCase().includes(filter)))
                        .slice(0, 100)
                    );
                }})()"#,
                filter = serde_json::to_string(filter).unwrap()
            );
            Some(eval_js_in(app, &js, "agent-browser").await)
        }

        "browser_list_elements" => {
            let filter = args.get("filter").and_then(|v| v.as_str()).unwrap_or("");
            let js = format!(
                r#"(() => {{
                    const sels = 'button, input, textarea, select, a, [role=button], [role=link], [role=tab]';
                    const els = Array.from(document.querySelectorAll(sels));
                    const filter = {filter}.toLowerCase();
                    return JSON.stringify(els
                        .map((el, i) => ({{
                            index: i,
                            tag: el.tagName.toLowerCase(),
                            type: el.type || null,
                            text: (el.textContent || el.placeholder || el.title || el.ariaLabel || '').trim().slice(0, 80),
                            href: el.href || null,
                            selector: el.id ? '#' + el.id : (el.className ? el.tagName.toLowerCase() + '.' + el.className.trim().split(/\s+/).join('.') : null),
                            visible: el.offsetParent !== null || el.offsetHeight > 0,
                        }}))
                        .filter(e => e.visible && (!filter || e.text.toLowerCase().includes(filter) || (e.selector && e.selector.toLowerCase().includes(filter))))
                        .slice(0, 100)
                    );
                }})()"#,
                filter = serde_json::to_string(filter).unwrap()
            );
            Some(eval_js_in(app, &js, "agent-browser").await)
        }

        "browser_scroll" => {
            let direction = args.get("direction").and_then(|v| v.as_str()).unwrap_or("down");
            let amount = args.get("amount").and_then(|v| v.as_u64()).unwrap_or(500);
            let js = format!(
                r#"(() => {{
                    const dy = {dir} === 'up' ? -{amount} : {amount};
                    window.scrollBy(0, dy);
                    return 'Scrolled ' + {dir} + ' by ' + Math.abs(dy) + 'px. Page at y=' + window.scrollY + '/' + document.body.scrollHeight;
                }})()"#,
                dir = serde_json::to_string(direction).unwrap(),
            );
            Some(eval_js_in(app, &js, "agent-browser").await)
        }

        "browser_screenshot" => {
            Some(capture_webview_screenshot(app, "agent-browser").await)
        }

        "browser_close" => {
            Some(close_browser_view_js(app).await)
        }

        _ => None,
    }
}

// ─── REST bridge endpoints ────────────────────────────────────────

pub async fn bridge_browser_navigate(
    State(app): State<AppHandle>,
    body: Bytes,
) -> Result<String, axum::http::StatusCode> {
    let body: Value = serde_json::from_slice(&body)
        .map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;
    let url = body.get("url")
        .and_then(|v| v.as_str())
        .ok_or(axum::http::StatusCode::BAD_REQUEST)?;
    let target = body.get("target")
        .and_then(|v| v.as_str())
        .unwrap_or("browser-panel");
    let is_default_panel = target == "browser-panel";

    // Navigate the target WebView (the default "browser-panel" is shared between
    // agent and user; other targets are per-tab child webviews used only by the
    // agent for parallel browsing). If it doesn't exist yet, create it hidden
    // (0 height) — the agent can still eval JS on it. When the user clicks the
    // globe icon, the frontend resizes the default panel to the correct position.
    let parsed = url.parse::<tauri::Url>()
        .map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;
    eprintln!("[MCP_BROWSER] navigate({target}): {url}");

    if let Some(existing) = app.webviews().get(target).cloned() {
        eprintln!("[MCP_BROWSER] navigating existing {target}");
        let _ = existing.navigate(parsed);
    } else if let Some(window) = app.get_window("main") {
        // Create the webview hidden (0 height) — agent can use it via eval,
        // user sees the default panel when they open the globe icon (frontend resizes it).
        let data_dir = app.path().app_data_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join("browser_data");
        let builder = tauri::webview::WebviewBuilder::new(
            target,
            tauri::WebviewUrl::External(parsed),
        )
        .data_directory(data_dir)
        .zoom_hotkeys_enabled(true);
        match window.add_child(
            builder,
            tauri::LogicalPosition::new(0.0, 0.0),
            tauri::LogicalSize::new(0.0, 0.0), // hidden until user opens globe
        ) {
            Ok(wv) => eprintln!(
                "[MCP_BROWSER] created hidden webview: {:?}",
                wv.label()
            ),
            Err(e) => eprintln!("[MCP_BROWSER] webview '{target}' creation FAILED: {e}"),
        }
    }

    // Tell frontend the URL so the globe icon knows what page is loaded —
    // only relevant for the user-visible default panel, not per-tab agent webviews.
    if is_default_panel {
        if let Some(main_wv) = app.webviews().get("main").cloned() {
            let js = format!(
                "if (window.__openBrowserView) {{ window.__openBrowserView('{}'); }}",
                url.replace('\'', "\\'").replace('\\', "\\\\")
            );
            let _ = main_wv.eval(&js);
        }
    }

    Ok(format!("Browser navigated: {url}"))
}

pub async fn bridge_browser_close(
    State(app): State<AppHandle>,
    body: Bytes,
) -> Result<String, axum::http::StatusCode> {
    let target = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|v| v.get("target").and_then(|t| t.as_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| "browser-panel".to_string());

    if target == "browser-panel" {
        // Legacy single-panel behavior: hide via JS hook, don't destroy the
        // webview (the user-visible panel is reused on the next navigate).
        return close_browser_view_js(&app)
            .await
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Per-tab agent webview: actually destroy it to free resources.
    if let Some(wv) = app.webviews().get(&target).cloned() {
        match wv.close() {
            Ok(()) => Ok(format!("Closed webview: {target}")),
            Err(e) => {
                eprintln!("[MCP_BROWSER] failed to close webview '{target}': {e}");
                Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    } else {
        Ok(format!("Webview '{target}' not open"))
    }
}
