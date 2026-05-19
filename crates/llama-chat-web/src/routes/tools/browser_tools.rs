//! Browser and desktop tool handlers.

use tokio::task::spawn_blocking;

pub async fn handle_browser_navigate(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let url = tool_arguments.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if url.is_empty() {
        return serde_json::json!({ "success": false, "error": "url is required" });
    }
    match spawn_blocking(move || llama_chat_tools::browser_session::notify_tauri_browser_navigate(&url)).await {
        Ok(Ok(())) => serde_json::json!({ "success": true, "result": "Navigated" }),
        Ok(Err(e)) => serde_json::json!({ "success": false, "error": e }),
        Err(e) => serde_json::json!({ "success": false, "error": format!("Task failed: {e}") }),
    }
}

pub async fn handle_browser_search(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let q = tool_arguments.get("query").and_then(|v| v.as_str()).unwrap_or("");
    if q.is_empty() {
        return serde_json::json!({ "success": false, "error": "query is required" });
    }
    let url = format!("https://www.google.com/search?q={}", urlencoding::encode(q));
    match spawn_blocking(move || llama_chat_tools::browser_session::notify_tauri_browser_navigate(&url)).await {
        Ok(Ok(())) => serde_json::json!({ "success": true, "result": "Navigated" }),
        Ok(Err(e)) => serde_json::json!({ "success": false, "error": e }),
        Err(e) => serde_json::json!({ "success": false, "error": format!("Task failed: {e}") }),
    }
}

pub async fn handle_browser_eval(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let js = tool_arguments.get("js").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if js.is_empty() {
        return serde_json::json!({ "success": false, "error": "js is required" });
    }
    match spawn_blocking(move || llama_chat_tools::browser_session::eval_in_browser_panel(&js)).await {
        Ok(Ok(r)) => serde_json::json!({ "success": true, "result": r }),
        Ok(Err(e)) => serde_json::json!({ "success": false, "error": e }),
        Err(e) => serde_json::json!({ "success": false, "error": format!("Task failed: {e}") }),
    }
}

pub async fn handle_browser_get_text() -> serde_json::Value {
    match spawn_blocking(|| llama_chat_tools::browser_session::eval_in_browser_panel("document.body.innerText")).await {
        Ok(Ok(r)) => serde_json::json!({ "success": true, "result": r }),
        Ok(Err(e)) => serde_json::json!({ "success": false, "error": e }),
        Err(e) => serde_json::json!({ "success": false, "error": format!("Task failed: {e}") }),
    }
}

pub fn handle_browser_close() -> serde_json::Value {
    let _ = llama_chat_tools::browser_session::notify_tauri_browser_close();
    serde_json::json!({ "success": true, "result": "Closed" })
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
