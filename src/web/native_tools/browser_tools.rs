//! Browser tool handlers — browser_navigate, browser_get_text, browser_click, etc.

use serde_json::Value;
use super::NativeToolResult;

pub fn handle_browser_tool(name: &str, args: &Value) -> NativeToolResult {
    use crate::web::browser::session::{
        current_session, notify_tauri_browser_close, open_session, BrowserSession,
    };

    // navigate: reuse existing session if any, else open a fresh one
    if name == "navigate" {
        let url = match args.get("url").and_then(|v| v.as_str()) {
            Some(u) if !u.trim().is_empty() => u,
            _ => return NativeToolResult::text_only("Error: 'url' is required".to_string()),
        };
        let full_url = if url.starts_with("http://") || url.starts_with("https://") {
            url.to_string()
        } else {
            format!("https://{url}")
        };
        eprintln!("[BROWSER_TOOL] navigate: {full_url}");
        return match current_session() {
            Ok(mut s) => {
                eprintln!("[BROWSER_TOOL] existing session, calling navigate...");
                match s.navigate(&full_url) {
                    Ok(()) => {
                        eprintln!("[BROWSER_TOOL] navigate OK");
                        NativeToolResult::text_only(format!("Navigated to {full_url}."))
                    }
                    Err(e) => {
                        eprintln!("[BROWSER_TOOL] navigate failed: {e}, opening new session...");
                        match open_session(&full_url) {
                            Ok(s2) => NativeToolResult::text_only(format!("Opened new session at {}.", s2.url())),
                            Err(e) => NativeToolResult::text_only(format!("navigate failed: {e}")),
                        }
                    }
                }
            }
            Err(_) => {
                eprintln!("[BROWSER_TOOL] no existing session, calling open_session...");
                match open_session(&full_url) {
                    Ok(s) => {
                        eprintln!("[BROWSER_TOOL] open_session OK");
                        NativeToolResult::text_only(format!(
                            "Navigated to {}.",
                            s.url()
                        ))
                    }
                    Err(e) => {
                        eprintln!("[BROWSER_TOOL] open_session FAILED: {e}");
                        NativeToolResult::text_only(format!("navigate failed: {e}"))
                    }
                }
            }
        };
    }

    // All other tools require an active session
    let session = match current_session() {
        Ok(s) => s,
        Err(e) => return NativeToolResult::text_only(format!("Error: {e}")),
    };

    match name {
        "click" => {
            let sel = args.get("selector").and_then(|v| v.as_str()).unwrap_or("");
            if sel.is_empty() {
                return NativeToolResult::text_only("Error: 'selector' is required".into());
            }
            match session.click(sel) {
                Ok(()) => {
                    // Click may navigate — clear caches so next get_text reads fresh
                    crate::web::browser::session::clear_cache();
                    NativeToolResult::text_only(format!("Clicked '{sel}'"))
                }
                Err(e) => NativeToolResult::text_only(format!("click failed: {e}")),
            }
        }
        "type" => {
            let sel = args.get("selector").and_then(|v| v.as_str()).unwrap_or("");
            let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let press_enter = args.get("press_enter").and_then(|v| v.as_bool()).unwrap_or(false);
            if sel.is_empty() || text.is_empty() {
                return NativeToolResult::text_only(
                    "Error: 'selector' and 'text' are required".into(),
                );
            }
            match session.type_text(sel, text, press_enter) {
                Ok(()) => NativeToolResult::text_only(format!(
                    "Typed into '{sel}'{}",
                    if press_enter { " and pressed Enter" } else { "" }
                )),
                Err(e) => NativeToolResult::text_only(format!("type failed: {e}")),
            }
        }
        "query" => {
            let selector = args.get("selector").and_then(|v| v.as_str()).unwrap_or("");
            if selector.is_empty() {
                return NativeToolResult::text_only("Error: 'selector' is required".into());
            }
            let attributes = args.get("attributes").and_then(|v| v.as_str()).unwrap_or("text");
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

            // Build JS that extracts requested attributes from matched elements
            let attr_list: Vec<&str> = attributes.split(',').map(|s| s.trim()).collect();
            let extract_js: Vec<String> = attr_list.iter().map(|attr| {
                match *attr {
                    "text" => "text: el.innerText?.trim() || ''".to_string(),
                    "html" => "html: el.outerHTML".to_string(),
                    "href" => "href: el.href || el.getAttribute('href') || ''".to_string(),
                    "src" => "src: el.src || el.getAttribute('src') || ''".to_string(),
                    other => format!("{k}: el.getAttribute('{k}') || ''", k = other),
                }
            }).collect();
            let extract_obj = extract_js.join(", ");

            let js = format!(
                "Array.from(document.querySelectorAll({sel})).slice(0, {limit}).map(el => ({{ {extract} }}))",
                sel = serde_json::to_string(selector).unwrap_or_default(),
                extract = extract_obj,
            );
            match session.eval(&js) {
                Ok(Value::Array(arr)) if arr.is_empty() => {
                    NativeToolResult::text_only(format!("No elements found matching '{selector}'"))
                }
                Ok(v) => NativeToolResult::text_only(v.to_string()),
                Err(e) => NativeToolResult::text_only(format!("query failed: {e}")),
            }
        }
        "eval" => {
            let js = args.get("js").and_then(|v| v.as_str()).unwrap_or("");
            if js.is_empty() {
                return NativeToolResult::text_only("Error: 'js' is required".into());
            }
            match session.eval(js) {
                Ok(Value::String(s)) => NativeToolResult::text_only(s),
                Ok(v) => NativeToolResult::text_only(v.to_string()),
                Err(e) => NativeToolResult::text_only(format!("eval failed: {e}")),
            }
        }
        "get_html" => match session.html() {
            Ok(html) => {
                const MAX: usize = 50_000;
                let mut s = html;
                if s.len() > MAX {
                    let mut end = MAX;
                    while end > 0 && !s.is_char_boundary(end) {
                        end -= 1;
                    }
                    s.truncate(end);
                    s.push_str("\n... [truncated]");
                }
                NativeToolResult::text_only(s)
            }
            Err(e) => NativeToolResult::text_only(format!("get_html failed: {e}")),
        },
        "screenshot" => {
            // Try Camofox screenshot first
            if let Ok(bytes) = session.screenshot() {
                NativeToolResult::with_image("Screenshot captured.".into(), bytes)
            } else {
                // No visual screenshot — return page info + suggestion
                match crate::web::browser::session::eval_in_browser_panel("document.title + ' — ' + window.location.href") {
                    Ok(info) => NativeToolResult::text_only(format!(
                        "Visual screenshot not available. Page: {info}\nUse browser_get_text to read content or browser_query to extract structured data."
                    )),
                    Err(_) => NativeToolResult::text_only(
                        "Screenshot not available. Use browser_get_text or browser_query instead.".into()
                    ),
                }
            }
        },
        "wait" => {
            let sel = args.get("selector").and_then(|v| v.as_str()).unwrap_or("");
            let timeout = args
                .get("timeout_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(5000);
            if sel.is_empty() {
                return NativeToolResult::text_only("Error: 'selector' is required".into());
            }
            match session.wait_for(sel, timeout) {
                Ok(true) => NativeToolResult::text_only(format!("Element '{sel}' appeared")),
                Ok(false) => {
                    NativeToolResult::text_only(format!("Timeout waiting for '{sel}'"))
                }
                Err(e) => NativeToolResult::text_only(format!("wait failed: {e}")),
            }
        }
        "get_text" => match session.snapshot() {
            Ok(text) => {
                const MAX: usize = 30_000;
                let mut s = text;
                if s.len() > MAX {
                    let mut end = MAX;
                    while end > 0 && !s.is_char_boundary(end) { end -= 1; }
                    s.truncate(end);
                    s.push_str("\n... [truncated]");
                }
                NativeToolResult::text_only(s)
            }
            Err(e) => NativeToolResult::text_only(format!("get_text failed: {e}")),
        },
        "get_links" => match session.html() {
            Ok(html) => {
                // Extract links from HTML using simple regex
                let mut links = Vec::new();
                for cap in regex::Regex::new(r#"<a[^>]+href="([^"]*)"[^>]*>(.*?)</a>"#)
                    .unwrap()
                    .captures_iter(&html)
                {
                    if links.len() >= 200 { break; }
                    let href = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                    let text = cap.get(2).map(|m| m.as_str()).unwrap_or("");
                    // Strip HTML tags from link text
                    let clean = regex::Regex::new(r"<[^>]+>").unwrap()
                        .replace_all(text, "").trim().chars().take(80).collect::<String>();
                    if !href.is_empty() && !clean.is_empty() {
                        links.push(serde_json::json!({"text": clean, "href": href}));
                    }
                }
                NativeToolResult::text_only(serde_json::to_string(&links).unwrap_or("[]".into()))
            }
            Err(e) => NativeToolResult::text_only(format!("get_links failed: {e}")),
        }
        "snapshot" => match session.snapshot() {
            Ok(s) => {
                const MAX: usize = 20_000;
                let mut text = s;
                if text.len() > MAX {
                    let mut end = MAX;
                    while end > 0 && !text.is_char_boundary(end) { end -= 1; }
                    text.truncate(end);
                    text.push_str("\n... [truncated]");
                }
                NativeToolResult::text_only(text)
            }
            Err(e) => NativeToolResult::text_only(format!("snapshot failed: {e}")),
        },
        "scroll" => {
            let sel = args.get("selector").and_then(|v| v.as_str()).unwrap_or("");
            let amount = args.get("amount").and_then(|v| v.as_i64()).unwrap_or(0);
            // Try eval first (Camofox), fall back to no-op (HTTP mode)
            let js = if !sel.is_empty() {
                format!(
                    "(() => {{ const el = document.querySelector({sel_lit}); if (el) {{ el.scrollIntoView({{behavior:'smooth', block:'center'}}); return 'scrolled to '+{sel_lit}; }} return 'element not found'; }})()",
                    sel_lit = serde_json::to_string(sel).unwrap_or_else(|_| "''".into())
                )
            } else {
                format!("(() => {{ window.scrollBy(0, {amount}); return 'scrolled '+{amount}+' px'; }})()")
            };
            match session.eval(&js) {
                Ok(v) => {
                    let msg = v.get("result").and_then(|r| r.as_str()).unwrap_or("done");
                    NativeToolResult::text_only(msg.to_string())
                }
                Err(_) => NativeToolResult::text_only("Scroll not available in HTTP mode (content is fetched statically).".into()),
            }
        }
        "press_key" => {
            let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("");
            if key.is_empty() {
                return NativeToolResult::text_only("Error: 'key' is required".into());
            }
            match session.press_key(key) {
                Ok(()) => NativeToolResult::text_only(format!("Pressed '{key}'")),
                Err(e) => NativeToolResult::text_only(format!("press_key failed: {e}")),
            }
        }
        "close" => {
            let mut s = session;
            let _ = notify_tauri_browser_close();
            match s.close() {
                Ok(()) => NativeToolResult::text_only("Browser session closed.".into()),
                Err(e) => NativeToolResult::text_only(format!("close failed: {e}")),
            }
        }
        other => NativeToolResult::text_only(format!("Unknown browser tool: browser_{other}")),
    }
}

