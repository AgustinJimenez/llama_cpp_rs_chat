//! Browser and desktop tool handlers.

use tokio::task::spawn_blocking;
use super::helpers::{fetch_url_as_text, FETCH_TIMEOUT_SECS};
use tokio::time::timeout;

const NAV_TIMEOUT_SECS: u64 = 30;

fn get_tab_id(args: &serde_json::Value) -> String {
    args.get("tab_id")
        .and_then(|v| v.as_str())
        .unwrap_or("main")
        .to_string()
}

pub async fn handle_browser_navigate(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let url = tool_arguments.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if url.is_empty() {
        return serde_json::json!({ "success": false, "error": "url is required" });
    }
    let tab_id = get_tab_id(tool_arguments);
    match timeout(
        std::time::Duration::from_secs(NAV_TIMEOUT_SECS),
        spawn_blocking(move || llama_chat_tools::browser_session::navigate_browser_tab(&url, &tab_id)),
    ).await {
        Ok(Ok(Ok(()))) => serde_json::json!({ "success": true, "result": "Navigated" }),
        Ok(Ok(Err(e))) => serde_json::json!({ "success": false, "error": e }),
        Ok(Err(e)) => serde_json::json!({ "success": false, "error": format!("Task failed: {e}") }),
        Err(_) => serde_json::json!({ "success": false, "error": format!("Navigation timed out after {NAV_TIMEOUT_SECS}s") }),
    }
}

pub async fn handle_browser_search(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let q = tool_arguments.get("query").and_then(|v| v.as_str()).unwrap_or("");
    if q.is_empty() {
        return serde_json::json!({ "success": false, "error": "query is required" });
    }
    let url = format!("https://www.google.com/search?q={}", urlencoding::encode(q));
    let tab_id = get_tab_id(tool_arguments);
    match timeout(
        std::time::Duration::from_secs(NAV_TIMEOUT_SECS),
        spawn_blocking(move || llama_chat_tools::browser_session::navigate_browser_tab(&url, &tab_id)),
    ).await {
        Ok(Ok(Ok(()))) => serde_json::json!({ "success": true, "result": "Navigated" }),
        Ok(Ok(Err(e))) => serde_json::json!({ "success": false, "error": e }),
        Ok(Err(e)) => serde_json::json!({ "success": false, "error": format!("Task failed: {e}") }),
        Err(_) => serde_json::json!({ "success": false, "error": format!("Search timed out after {NAV_TIMEOUT_SECS}s") }),
    }
}

pub async fn handle_browser_eval(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let js = tool_arguments.get("js").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if js.is_empty() {
        return serde_json::json!({ "success": false, "error": "js is required" });
    }
    let tab_id = get_tab_id(tool_arguments);
    match spawn_blocking(move || llama_chat_tools::browser_session::eval_in_browser_tab(&js, &tab_id)).await {
        Ok(Ok(r)) => serde_json::json!({ "success": true, "result": r }),
        Ok(Err(e)) => serde_json::json!({ "success": false, "error": e }),
        Err(e) => serde_json::json!({ "success": false, "error": format!("Task failed: {e}") }),
    }
}

pub async fn handle_browser_get_text(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let tab_id = get_tab_id(tool_arguments);
    let offset = tool_arguments.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let max_chars = tool_arguments.get("max_chars").and_then(|v| v.as_u64()).unwrap_or(8000) as usize;
    match spawn_blocking(move || llama_chat_tools::browser_session::eval_in_browser_tab("document.body.innerText", &tab_id)).await {
        Ok(Ok(r)) => {
            let total = r.len();
            let slice = &r[offset.min(total)..];
            let chunk = if slice.len() > max_chars { &slice[..max_chars] } else { slice };
            let remaining = total.saturating_sub(offset + max_chars);
            let result = if remaining > 0 {
                format!("{chunk}\n[{remaining} chars remaining — call browser_get_text(offset={}) to continue]", offset + max_chars)
            } else {
                chunk.to_string()
            };
            // Hint when content is suspiciously short — likely a JS-rendered SPA
            let mut json = serde_json::json!({ "success": true, "result": result });
            if offset == 0 && total < 500 {
                json["partial"] = serde_json::json!(true);
                json["partial_reason"] = serde_json::json!(
                    "Page text is very short — this is likely a JS-rendered app (canvas/WebGL/SPA) with minimal DOM text. Suggestions: use browser_get_html to check for inline script data, browser_snapshot to see interactive elements, or browser_screenshot for a visual capture."
                );
            }
            json
        }
        Ok(Err(e)) => serde_json::json!({ "success": false, "error": e }),
        Err(e) => serde_json::json!({ "success": false, "error": format!("Task failed: {e}") }),
    }
}

pub async fn handle_browser_get_html(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let tab_id = get_tab_id(tool_arguments);
    let max_chars = tool_arguments.get("max_chars").and_then(|v| v.as_u64()).unwrap_or(8000) as usize;
    match spawn_blocking(move || llama_chat_tools::browser_session::eval_in_browser_tab("document.documentElement.outerHTML", &tab_id)).await {
        Ok(Ok(r)) => {
            let chunk = if r.len() > max_chars {
                format!("{}\n[truncated — {} chars total]", &r[..max_chars], r.len())
            } else {
                r
            };
            serde_json::json!({ "success": true, "result": chunk })
        }
        Ok(Err(e)) => serde_json::json!({ "success": false, "error": e }),
        Err(e) => serde_json::json!({ "success": false, "error": format!("Task failed: {e}") }),
    }
}

/// Fetch article content from a URL via HTTP — without navigating the browser.
/// Uses the same html2text extraction as web_fetch but is explicitly scoped for
/// "read this article without leaving the current browser tab."
pub async fn handle_browser_fetch_text(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let url = tool_arguments.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if url.is_empty() {
        return serde_json::json!({ "success": false, "error": "url is required" });
    }
    let max_chars = tool_arguments.get("max_chars").and_then(|v| v.as_u64()).unwrap_or(8000) as usize;
    match timeout(
        std::time::Duration::from_secs(FETCH_TIMEOUT_SECS + 5),
        spawn_blocking(move || fetch_url_as_text(&url, max_chars)),
    ).await {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => serde_json::json!({ "success": false, "error": format!("fetch task failed: {e}") }),
        Err(_) => serde_json::json!({ "success": false, "error": "fetch timed out" }),
    }
}

pub fn handle_browser_go_back(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let tab_id = get_tab_id(tool_arguments);
    match llama_chat_tools::browser_session::eval_in_browser_tab("history.go(-1); 'ok'", &tab_id) {
        Ok(_) => serde_json::json!({ "success": true, "result": "Navigated back" }),
        Err(e) => serde_json::json!({ "success": false, "error": e }),
    }
}

pub fn handle_browser_close(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let tab_id = get_tab_id(tool_arguments);
    if tab_id == "main" {
        // Close everything (legacy behavior)
        let _ = llama_chat_tools::browser_session::notify_tauri_browser_close();
    } else {
        let _ = llama_chat_tools::browser_session::close_browser_tab(&tab_id);
    }
    serde_json::json!({ "success": true, "result": format!("Closed tab '{tab_id}'") })
}

pub fn handle_desktop_tool(name: &str, tool_arguments: &serde_json::Value) -> serde_json::Value {
    let result = if name == "take_screenshot" {
        llama_chat_desktop_tools::tool_take_screenshot_with_image(tool_arguments)
    } else {
        llama_chat_desktop_tools::dispatch_desktop_tool(name, tool_arguments)
            .unwrap_or_else(|| llama_chat_types::NativeToolResult::text_only(
                format!("Desktop tool '{name}' not found")
            ))
    };
    serde_json::json!({"success": true, "result": result.text})
}
