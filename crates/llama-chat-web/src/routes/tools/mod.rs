// Tool execution route handler

use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;

use crate::request_parsing::parse_json_body;
use crate::response_helpers::{json_error, json_raw};

mod helpers;
mod file_tools;
mod exec_tools;
mod browser_tools;

use helpers::canonicalize_allowed;
pub use helpers::{handle_get_web_fetch, handle_post_extract_text};
#[allow(unused_imports)]
pub use helpers::fetch_url_as_text;

#[cfg(not(feature = "mock"))]
use llama_chat_worker::worker::worker_bridge::SharedWorkerBridge;

/// GET /api/tools/available — list all available tools with their schemas
pub async fn handle_get_available_tools(
    #[cfg(not(feature = "mock"))] _bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
) -> Result<Response<Body>, Infallible> {
    let core_tools = llama_chat_engine::jinja_templates::get_available_tools();
    let body = serde_json::json!({
        "core_tools": core_tools.len(),
        "tools": core_tools,
    });
    Ok(json_raw(StatusCode::OK, serde_json::to_string(&body).unwrap()))
}

#[derive(serde::Deserialize)]
struct ToolExecuteRequest {
    tool_name: String,
    arguments: serde_json::Value,
}

pub async fn handle_post_tools_execute(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] _bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
) -> Result<Response<Body>, Infallible> {
    let request: ToolExecuteRequest = match parse_json_body(req.into_body()).await {
        Ok(req) => req,
        Err(error_response) => return Ok(error_response),
    };

    #[cfg(not(feature = "mock"))]
    let (tool_name, tool_arguments) = {
        let meta = _bridge.model_status().await;
        let chat_template = meta
            .as_ref()
            .and_then(|m| m.chat_template_type.as_deref())
            .unwrap_or("Unknown");
        let capabilities = llama_chat_types::models::get_model_capabilities(chat_template);
        llama_chat_types::models::translate_tool_for_model(
            &request.tool_name,
            &request.arguments,
            &capabilities,
        )
    };

    #[cfg(feature = "mock")]
    let (tool_name, tool_arguments) = (request.tool_name.clone(), request.arguments.clone());

    sys_debug!(
        "[TOOL EXECUTE] Original: {} → Actual: {}",
        request.tool_name,
        tool_name
    );

    let result = match tool_name.as_str() {
        "read_file" => {
            // Validate path before delegating (for proper HTTP error response)
            let path = tool_arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if path.is_empty() {
                return Ok(json_error(StatusCode::BAD_REQUEST, "File path is required"));
            }
            if let Err(e) = canonicalize_allowed(path).await {
                return Ok(json_error(StatusCode::FORBIDDEN, &e));
            }
            file_tools::handle_read_file(&tool_arguments).await
        }
        "write_file" => {
            let path = tool_arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if path.is_empty() {
                return Ok(json_error(StatusCode::BAD_REQUEST, "File path is required"));
            }
            if let Err(e) = canonicalize_allowed(path).await {
                return Ok(json_error(StatusCode::FORBIDDEN, &e));
            }
            file_tools::handle_write_file(&tool_arguments).await
        }
        "edit_file" => {
            let path = tool_arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let old_string = tool_arguments.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
            if path.is_empty() {
                return Ok(json_error(StatusCode::BAD_REQUEST, "File path is required"));
            }
            if old_string.is_empty() {
                return Ok(json_error(StatusCode::BAD_REQUEST, "old_string is required"));
            }
            if let Err(e) = canonicalize_allowed(path).await {
                return Ok(json_error(StatusCode::FORBIDDEN, &e));
            }
            file_tools::handle_edit_file(&tool_arguments).await
        }
        "undo_edit" => {
            let path = tool_arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if path.is_empty() {
                return Ok(json_error(StatusCode::BAD_REQUEST, "File path is required"));
            }
            if let Err(e) = canonicalize_allowed(path).await {
                return Ok(json_error(StatusCode::FORBIDDEN, &e));
            }
            file_tools::handle_undo_edit(&tool_arguments).await
        }
        "insert_text" => {
            let path = tool_arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let line = tool_arguments.get("line").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            if path.is_empty() {
                return Ok(json_error(StatusCode::BAD_REQUEST, "File path is required"));
            }
            if line == 0 {
                return Ok(json_error(StatusCode::BAD_REQUEST, "Line number is required (1-based)"));
            }
            if let Err(e) = canonicalize_allowed(path).await {
                return Ok(json_error(StatusCode::FORBIDDEN, &e));
            }
            file_tools::handle_insert_text(&tool_arguments).await
        }
        "search_files" => {
            let pattern = tool_arguments.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
            let path = tool_arguments.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            if pattern.is_empty() {
                return Ok(json_error(StatusCode::BAD_REQUEST, "Search pattern is required"));
            }
            if let Err(e) = canonicalize_allowed(path).await {
                return Ok(json_error(StatusCode::FORBIDDEN, &e));
            }
            file_tools::handle_search_files(&tool_arguments).await
        }
        "find_files" => {
            let pattern = tool_arguments.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
            let path = tool_arguments.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            if pattern.is_empty() {
                return Ok(json_error(StatusCode::BAD_REQUEST, "File pattern is required"));
            }
            if let Err(e) = canonicalize_allowed(path).await {
                return Ok(json_error(StatusCode::FORBIDDEN, &e));
            }
            file_tools::handle_find_files(&tool_arguments).await
        }
        "list_directory" => {
            let path = tool_arguments.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            if let Err(e) = canonicalize_allowed(path).await {
                return Ok(json_error(StatusCode::FORBIDDEN, &e));
            }
            file_tools::handle_list_directory(&tool_arguments).await
        }
        "bash" | "shell" | "command" => {
            let command = tool_arguments.get("command").and_then(|v| v.as_str()).unwrap_or("");
            if command.is_empty() {
                return Ok(json_error(StatusCode::BAD_REQUEST, "Command is required"));
            }
            exec_tools::handle_bash(&tool_arguments).await
        }
        "web_fetch" => {
            let url = tool_arguments.get("url").and_then(|v| v.as_str()).unwrap_or("");
            if url.is_empty() {
                return Ok(json_error(StatusCode::BAD_REQUEST, "URL is required"));
            }
            exec_tools::handle_web_fetch(&tool_arguments).await
        }
        "browser_navigate" => browser_tools::handle_browser_navigate(&tool_arguments).await,
        "browser_go_back" => browser_tools::handle_browser_go_back(&tool_arguments),
        "browser_search" => browser_tools::handle_browser_search(&tool_arguments).await,
        "browser_eval" => {
            let js = tool_arguments.get("js").and_then(|v| v.as_str()).unwrap_or("");
            if js.is_empty() {
                return Ok(json_error(StatusCode::BAD_REQUEST, "js is required"));
            }
            browser_tools::handle_browser_eval(&tool_arguments).await
        }
        "browser_get_text" => browser_tools::handle_browser_get_text(&tool_arguments).await,
        "browser_get_html" => browser_tools::handle_browser_get_html(&tool_arguments).await,
        "browser_fetch_text" => browser_tools::handle_browser_fetch_text(&tool_arguments).await,
        "browser_close" => browser_tools::handle_browser_close(&tool_arguments),
        name if llama_chat_desktop_tools::is_desktop_tool(name) => {
            browser_tools::handle_desktop_tool(name, &tool_arguments)
        }
        _ => {
            serde_json::json!({
                "success": false,
                "error": format!("Unknown tool: {}", request.tool_name)
            })
        }
    };

    Ok(json_raw(StatusCode::OK, result.to_string()))
}
