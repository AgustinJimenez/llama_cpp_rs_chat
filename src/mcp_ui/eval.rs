//! JavaScript evaluation in WebView2 via raw COM vtable.
//!
//! `eval_js_in` uses WebView2's `ExecuteScript` COM API directly, which returns
//! results through a COM callback — bypassing CSP, CORS, and mixed-content
//! restrictions. The wrapping logic handles 4 JS input shapes (arrow fn,
//! IIFE, multi-statement, simple expression) and always produces a
//! JSON-encoded result.

use serde_json::Value;
use tauri::{AppHandle, Manager};
use tokio::sync::oneshot;

/// Timeout for JS eval results (ms).
pub const EVAL_TIMEOUT_MS: u64 = 10_000;

/// Execute JavaScript in the named webview and return the result.
///
/// Uses WebView2's `ExecuteScript` COM API directly (via `with_webview`),
/// which returns JS eval results through a COM callback — bypasses CSP,
/// CORS, and mixed-content restrictions that plagued the old HTTP callback approach.
pub async fn eval_js_in(app: &AppHandle, js: &str, target: &str) -> Result<String, String> {
    let (tx, rx) = oneshot::channel::<String>();

    // Wrap user JS to always return a JSON string.
    // IMPORTANT: Must be synchronous — WebView2 ExecuteScript does NOT
    // await Promises (returns `{}` for Promise objects).
    //
    // Handles 4 cases:
    // 1. Arrow function `() => {...}` → call it as IIFE
    // 2. Multi-statement with `return` → wrap in IIFE
    // 3. Multi-statement (const/let/var) → wrap in IIFE, return last expression
    // 4. Simple expression → use directly
    let trimmed = js.trim().trim_end_matches(';').trim();
    let is_multistatement = trimmed.starts_with("const ")
        || trimmed.starts_with("let ")
        || trimmed.starts_with("var ")
        || trimmed.contains(";\n")
        || trimmed.contains("; ");
    let is_iife = trimmed.starts_with('(')
        && (trimmed.ends_with(")()")  || trimmed.ends_with(")()\n"));
    let eval_expr = if trimmed.starts_with("() =>")
        || trimmed.starts_with("function(")
        || trimmed.starts_with("function (")
    {
        // Case 1: function definition — call it
        format!("({trimmed})()")
    } else if is_iife {
        // Case 1b: already an IIFE — use directly
        trimmed.to_string()
    } else if js.contains("return ") {
        // Case 2: has explicit return — wrap in IIFE
        format!("(function() {{ {js} }})()")
    } else if is_multistatement {
        // Case 3: multi-statement without return — wrap in IIFE.
        // Split on last `;`, add `return` before the final expression.
        // e.g. "const x = 1; x" → "(function() { const x = 1; return x; })()"
        let parts: Vec<&str> = trimmed.rsplitn(2, ';').collect();
        if parts.len() == 2 {
            let last_expr = parts[0].trim();
            let prefix = parts[1];
            if last_expr.is_empty() {
                format!("(function() {{ {prefix}; }})()")
            } else {
                format!("(function() {{ {prefix}; return ({last_expr}); }})()")
            }
        } else {
            format!("(function() {{ return ({trimmed}); }})()")
        }
    } else {
        // Case 4: simple expression
        js.to_string()
    };
    let wrapped_js = format!(
        r#"(function() {{
            try {{
                var __val = {eval_expr};
                return JSON.stringify(__val ?? null);
            }} catch (e) {{
                return JSON.stringify({{ __error: e.message }});
            }}
        }})()"#,
    );

    // Find the target webview
    let webviews = app.webviews();
    let webview = if target == "browser-panel" || target == "agent-browser" {
        webviews.get(target)
            .ok_or("Browser panel not open. Use browser_navigate first.")?
            .clone()
    } else if let Some(wv) = app.get_webview_window("main") {
        wv.as_ref().clone()
    } else if let Some(wv) = webviews.values().next() {
        wv.clone()
    } else {
        return Err("No webview available".into());
    };

    // Use WebView2 ExecuteScript directly — returns result via COM callback.
    // Bypasses CSP/CORS since results come through the COM API, not HTTP.
    //
    // We call ExecuteScript through the raw COM vtable because webview2-com
    // depends on windows-core 0.61 while we depend on windows 0.62, making
    // the PCWSTR types incompatible at Rust's type level (same ABI though).
    #[cfg(windows)]
    {
        let js_for_closure = wrapped_js.clone();
        webview.with_webview(move |platform_wv| {
            let controller = platform_wv.controller();
            let core_wv = unsafe { controller.CoreWebView2() };

            match core_wv {
                Ok(core) => {
                    let handler = webview2_com::ExecuteScriptCompletedHandler::create(
                        Box::new(move |_hr, result| {
                            let _ = tx.send(result);
                            Ok(())
                        }),
                    );
                    // Encode JS as null-terminated UTF-16
                    let wide: Vec<u16> = js_for_closure
                        .encode_utf16()
                        .chain(std::iter::once(0))
                        .collect();
                    // Call ExecuteScript via raw COM vtable to avoid
                    // PCWSTR version conflicts between windows 0.61/0.62.
                    // Vtable layout: IUnknown(3) + ICoreWebView2 methods.
                    // Index 29 = ExecuteScript (verified from ICoreWebView2_Vtbl).
                    unsafe {
                        let this: *mut std::ffi::c_void = std::mem::transmute_copy(&core);
                        let vtable = *(this as *const *const usize);
                        type ExecuteScriptFn = unsafe extern "system" fn(
                            this: *mut std::ffi::c_void,
                            js: *const u16,
                            handler: *mut std::ffi::c_void,
                        ) -> i32;
                        let func: ExecuteScriptFn =
                            std::mem::transmute(*vtable.add(29));
                        let handler_ptr: *mut std::ffi::c_void =
                            std::mem::transmute_copy(&handler);
                        func(this, wide.as_ptr(), handler_ptr);
                    }
                }
                Err(e) => {
                    let _ = tx.send(format!(
                        r#"{{"__error":"CoreWebView2 unavailable: {e}"}}"#
                    ));
                }
            }
        }).map_err(|e| format!("with_webview failed: {e}"))?;
    }

    // Non-Windows: fall back to old eval (fire-and-forget, won't return values)
    #[cfg(not(windows))]
    {
        webview.eval(&wrapped_js).map_err(|e| format!("eval failed: {e}"))?;
        let _ = tx.send(r#""eval sent (no return value on this platform)""#.to_string());
    }

    // Wait for the result with timeout
    match tokio::time::timeout(
        std::time::Duration::from_millis(EVAL_TIMEOUT_MS),
        rx,
    ).await {
        Ok(Ok(value)) => {
            // WebView2 returns JSON-encoded strings (extra quotes), unwrap them
            let cleaned = if value.starts_with('"') && value.ends_with('"') {
                // Parse the outer JSON string encoding added by WebView2
                serde_json::from_str::<String>(&value).unwrap_or(value)
            } else {
                value
            };
            // Check for JS errors
            if let Ok(parsed) = serde_json::from_str::<Value>(&cleaned) {
                if let Some(err) = parsed.get("__error").and_then(|e| e.as_str()) {
                    return Err(format!("JS error: {err}"));
                }
            }
            Ok(cleaned)
        }
        Ok(Err(_)) => Err("Result channel closed".into()),
        Err(_) => Err(format!("JS eval timed out ({EVAL_TIMEOUT_MS}ms)")),
    }
}
