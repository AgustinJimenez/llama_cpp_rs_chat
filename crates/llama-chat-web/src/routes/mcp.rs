// MCP (Model Context Protocol) server management route handlers

use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;

use llama_chat_db::SharedDatabase;
use llama_chat_worker::{McpServerConfig, McpTransport};
use crate::request_parsing::parse_json_body;
use crate::response_helpers::{json_error, json_success, json_raw};
use llama_chat_worker::worker::worker_bridge::SharedWorkerBridge;
use llama_chat_types::ipc_types::WorkerPayload;

/// GET /api/mcp/servers — List all configured MCP servers
pub async fn handle_list_mcp_servers(
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let servers = llama_chat_db::mcp::load_mcp_servers(&db);
    match serde_json::to_string(&servers) {
        Ok(json) => Ok(json_raw(StatusCode::OK, json)),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("Serialize error: {e}"))),
    }
}

/// POST /api/mcp/servers — Add or update an MCP server configuration
pub async fn handle_save_mcp_server(
    req: Request<Body>,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let config: McpServerConfig = match parse_json_body(req.into_body()).await {
        Ok(c) => c,
        Err(err_resp) => return Ok(err_resp),
    };

    // Validate
    if config.id.is_empty() || config.name.is_empty() {
        return Ok(json_error(StatusCode::BAD_REQUEST, "id and name are required"));
    }
    match &config.transport {
        McpTransport::Stdio { command, .. } if command.is_empty() => {
            return Ok(json_error(StatusCode::BAD_REQUEST, "command is required for stdio transport"));
        }
        McpTransport::Http { url } if url.is_empty() => {
            return Ok(json_error(StatusCode::BAD_REQUEST, "url is required for http transport"));
        }
        _ => {}
    }

    match llama_chat_db::mcp::save_mcp_server(&db, &config) {
        Ok(()) => Ok(json_success("MCP server saved")),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

/// DELETE /api/mcp/servers/:id — Delete an MCP server configuration
pub async fn handle_delete_mcp_server(
    id: &str,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    match llama_chat_db::mcp::delete_mcp_server(&db, id) {
        Ok(()) => Ok(json_success("MCP server deleted")),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

/// POST /api/mcp/servers/:id/toggle — Enable/disable an MCP server
pub async fn handle_toggle_mcp_server(
    req: Request<Body>,
    id: &str,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    #[derive(serde::Deserialize)]
    struct ToggleBody {
        enabled: bool,
    }

    let body: ToggleBody = match parse_json_body(req.into_body()).await {
        Ok(b) => b,
        Err(err_resp) => return Ok(err_resp),
    };

    match llama_chat_db::mcp::toggle_mcp_server(&db, id, body.enabled) {
        Ok(()) => Ok(json_success(&format!("MCP server {} {}", id, if body.enabled { "enabled" } else { "disabled" }))),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

/// POST /api/mcp/refresh — Reconnect to MCP servers and rediscover tools
pub async fn handle_refresh_mcp(
    bridge: SharedWorkerBridge,
) -> Result<Response<Body>, Infallible> {
    match bridge.refresh_mcp_servers().await {
        Ok(WorkerPayload::McpServersRefreshed { connected_servers, total_tools }) => {
            let result = serde_json::json!({
                "success": true,
                "connected_servers": connected_servers,
                "total_tools": total_tools,
            });
            Ok(json_raw(StatusCode::OK, result.to_string()))
        }
        Ok(WorkerPayload::Error { message }) => {
            Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &message))
        }
        Ok(_) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, "Unexpected response from worker")),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

/// GET /api/mcp/tools — List all discovered MCP tools
pub async fn handle_list_mcp_tools(
    bridge: SharedWorkerBridge,
) -> Result<Response<Body>, Infallible> {
    match bridge.get_mcp_status().await {
        Ok(WorkerPayload::McpStatus { servers }) => {
            let result = serde_json::json!({
                "servers": servers,
            });
            Ok(json_raw(StatusCode::OK, result.to_string()))
        }
        Ok(WorkerPayload::Error { message }) => {
            Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &message))
        }
        Ok(_) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, "Unexpected response from worker")),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}
