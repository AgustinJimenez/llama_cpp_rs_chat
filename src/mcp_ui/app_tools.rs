//! App UI tool handlers (interact with the main LLaMA Chat webview).
//!
//! Covers all `app_*` tool dispatch cases: clicking elements, typing, reading
//! DOM content, listing interactive elements, evaluating arbitrary JS, querying
//! app state via WorkerBridge, loading models, and sending chat messages.

use serde_json::{Value, json};
use tauri::Manager;

use super::eval::eval_js_in;
use crate::web::worker::worker_bridge::SharedWorkerBridge;

/// Opaque reference to the AppHandle — passed in rather than re-exported to
/// keep the public API minimal.
use tauri::AppHandle;

/// Handle all `app_*` MCP tool calls.
/// Returns `None` if the tool name is not an app tool.
pub async fn dispatch_app_tool(
    app: &AppHandle,
    name: &str,
    args: &Value,
) -> Option<Result<String, String>> {
    match name {
        "app_click" => {
            let sel = match args.get("selector").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => return Some(Err("'selector' is required".into())),
            };
            let js = format!(
                r#"(() => {{
                    const el = document.querySelector({sel});
                    if (!el) return 'Element not found: ' + {sel};
                    el.click();
                    return 'Clicked ' + el.tagName + ': ' + (el.textContent || '').trim().slice(0, 50);
                }})()"#,
                sel = serde_json::to_string(sel).unwrap()
            );
            Some(eval_js_in(app, &js, "main").await)
        }

        "app_type" => {
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
                    const setter = Object.getOwnPropertyDescriptor(
                        el.tagName === 'TEXTAREA' ? window.HTMLTextAreaElement.prototype : window.HTMLInputElement.prototype,
                        'value'
                    )?.set;
                    if (setter) {{ setter.call(el, {text}); }}
                    else {{ el.value = {text}; }}
                    el.dispatchEvent(new Event('input', {{ bubbles: true }}));
                    el.dispatchEvent(new Event('change', {{ bubbles: true }}));
                    {submit_js}
                    return 'Typed into ' + {sel};
                }})()"#,
                sel = serde_json::to_string(sel).unwrap(),
                text = serde_json::to_string(text).unwrap(),
                submit_js = if submit {
                    r#"
                    const form = el.closest('form');
                    if (form) form.requestSubmit();
                    else {
                        const btn = document.querySelector('button[type="submit"]');
                        if (btn) setTimeout(() => btn.click(), 100);
                    }
                    "#
                } else { "" }
            );
            Some(eval_js_in(app, &js, "main").await)
        }

        "app_read" => {
            let sel = match args.get("selector").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => return Some(Err("'selector' is required".into())),
            };
            let js = format!(
                r#"(() => {{
                    const el = document.querySelector({sel});
                    if (!el) return 'Element not found: ' + {sel};
                    return (el.textContent || el.value || '').trim().slice(0, 50000);
                }})()"#,
                sel = serde_json::to_string(sel).unwrap()
            );
            Some(eval_js_in(app, &js, "main").await)
        }

        "app_list_elements" => {
            let filter = args.get("filter").and_then(|v| v.as_str()).unwrap_or("");
            let js = format!(
                r#"(() => {{
                    const sels = 'button, input, textarea, select, a, [role=button], [data-testid]';
                    const els = Array.from(document.querySelectorAll(sels));
                    const filter = {filter}.toLowerCase();
                    return JSON.stringify(els
                        .map((el, i) => ({{
                            index: i,
                            tag: el.tagName.toLowerCase(),
                            type: el.type || null,
                            text: (el.textContent || el.placeholder || el.title || el.ariaLabel || '').trim().slice(0, 80),
                            selector: el.id ? '#' + el.id : (el.dataset.testid ? '[data-testid="' + el.dataset.testid + '"]' : null),
                            visible: el.offsetParent !== null || el.offsetHeight > 0,
                        }}))
                        .filter(e => e.visible && (!filter || e.text.toLowerCase().includes(filter) || (e.selector && e.selector.toLowerCase().includes(filter))))
                        .slice(0, 100)
                    );
                }})()"#,
                filter = serde_json::to_string(filter).unwrap()
            );
            Some(eval_js_in(app, &js, "main").await)
        }

        "app_eval" => {
            let js = match args.get("js").and_then(|v| v.as_str()) {
                Some(j) => j,
                None => return Some(Err("'js' is required".into())),
            };
            let wrapped = format!("return ({js})");
            Some(eval_js_in(app, &wrapped, "main").await)
        }

        "app_get_state" => {
            let bridge: SharedWorkerBridge = match app.try_state::<SharedWorkerBridge>() {
                Some(s) => s.inner().clone(),
                None => return Some(Err("WorkerBridge not available".into())),
            };
            let meta = bridge.model_status().await;
            let generating = bridge.is_generating().await;
            let loading = bridge.is_loading();
            let state = json!({
                "model_loaded": meta.is_some(),
                "model_path": meta.as_ref().map(|m| &m.model_path),
                "generating": generating,
                "loading": loading,
            });
            Some(Ok(state.to_string()))
        }

        "app_load_model" => {
            let path = match args.get("path").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return Some(Err("'path' is required".into())),
            };
            let bridge: SharedWorkerBridge = match app.try_state::<SharedWorkerBridge>() {
                Some(s) => s.inner().clone(),
                None => return Some(Err("WorkerBridge not available".into())),
            };
            Some(match bridge.load_model(path, None, None).await {
                Ok(_) => Ok(format!("Model loaded: {path}")),
                Err(e) => Err(format!("Load failed: {e}")),
            })
        }

        "app_send_message" => {
            let text = match args.get("text").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => return Some(Err("'text' is required".into())),
            };
            let js = format!(
                r#"(() => {{
                    const ta = document.querySelector('textarea[placeholder="Ask anything"]');
                    if (!ta) return 'Message input not found';
                    const setter = Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, 'value')?.set;
                    if (setter) setter.call(ta, {text});
                    else ta.value = {text};
                    ta.dispatchEvent(new Event('input', {{ bubbles: true }}));
                    setTimeout(() => {{
                        const btn = document.querySelector('button[type="submit"], button[aria-label="Send message"]');
                        if (btn) btn.click();
                    }}, 200);
                    return 'Message sent: ' + {text}.slice(0, 50);
                }})()"#,
                text = serde_json::to_string(text).unwrap()
            );
            Some(eval_js_in(app, &js, "main").await)
        }

        "app_wait_for" => {
            let sel = match args.get("selector").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => return Some(Err("'selector' is required".into())),
            };
            let timeout = args.get("timeout_ms").and_then(|v| v.as_u64()).unwrap_or(10_000);
            let js = format!(
                r#"new Promise((resolve) => {{
                    const t0 = Date.now();
                    const check = () => {{
                        if (document.querySelector({sel})) return resolve('Found: ' + {sel});
                        if (Date.now() - t0 >= {timeout}) return resolve('Timeout waiting for: ' + {sel});
                        setTimeout(check, 200);
                    }};
                    check();
                }})"#,
                sel = serde_json::to_string(sel).unwrap(),
            );
            Some(eval_js_in(app, &js, "main").await)
        }

        "app_navigate_browser" => {
            let url = match args.get("url").and_then(|v| v.as_str()) {
                Some(u) => u,
                None => return Some(Err("'url' is required".into())),
            };
            Some(super::browser_tools::open_browser_view_js(app, url).await)
        }

        "app_screenshot" => {
            // Return DOM structure as text (no image dependency)
            let js = r#"(() => {
                const body = document.body;
                if (!body) return 'No body element';
                return body.innerText.slice(0, 30000);
            })()"#;
            Some(eval_js_in(app, js, "main").await)
        }

        _ => None,
    }
}
