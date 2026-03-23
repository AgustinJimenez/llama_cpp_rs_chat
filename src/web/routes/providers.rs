// Provider route handlers

use hyper::{Body, Response, StatusCode};
use std::convert::Infallible;

use crate::web::response_helpers::{json_error, json_raw, serialize_with_fallback};

/// GET /api/providers — list available providers and their status
pub async fn handle_list_providers() -> Result<Response<Body>, Infallible> {
    let claude_available = crate::web::providers::claude_code::is_available().await;
    let claude_version = if claude_available {
        crate::web::providers::claude_code::get_version().await
    } else {
        None
    };

    let response = serde_json::json!({
        "providers": [
            {
                "id": "local",
                "name": "Local Model (llama.cpp)",
                "available": true,
                "description": "Run models locally on your GPU"
            },
            {
                "id": "claude_code",
                "name": "Claude Code",
                "available": claude_available,
                "version": claude_version,
                "description": "Use your Claude Code subscription (Max/Pro)",
                "models": ["opus", "sonnet", "haiku"]
            }
        ]
    });

    let response_json = serialize_with_fallback(&response, "{}");
    Ok(json_raw(StatusCode::OK, response_json))
}

/// POST /api/providers/claude/generate — generate via Claude Code CLI
pub async fn handle_claude_generate(
    req: hyper::Request<Body>,
) -> Result<Response<Body>, Infallible> {
    let body = match hyper::body::to_bytes(req.into_body()).await {
        Ok(b) => b,
        Err(_) => return Ok(json_error(StatusCode::BAD_REQUEST, "Failed to read body")),
    };

    #[derive(serde::Deserialize)]
    struct GenerateRequest {
        prompt: String,
        model: Option<String>,
        max_turns: Option<u32>,
        cwd: Option<String>,
    }

    let request: GenerateRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return Ok(json_error(StatusCode::BAD_REQUEST, &format!("Invalid JSON: {e}"))),
    };

    let model = match request.model.as_deref() {
        Some("opus") => crate::web::providers::claude_code::ClaudeModel::Opus,
        Some("haiku") => crate::web::providers::claude_code::ClaudeModel::Haiku,
        _ => crate::web::providers::claude_code::ClaudeModel::Sonnet,
    };

    let mut rx = match crate::web::providers::claude_code::generate(
        &request.prompt,
        &model,
        request.max_turns,
        request.cwd.as_deref(),
    ).await {
        Ok(rx) => rx,
        Err(e) => return Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("Failed to start Claude: {e}"))),
    };

    // Collect all tokens (non-streaming for now)
    let mut full_response = String::new();
    let mut cost_usd = None;
    let mut duration_ms = None;
    let mut stop_reason = None;

    while let Some(token_data) = rx.recv().await {
        if token_data.is_done {
            cost_usd = token_data.cost_usd;
            duration_ms = token_data.duration_ms;
            stop_reason = token_data.stop_reason;
            break;
        }
        full_response.push_str(&token_data.token);
    }

    let result = serde_json::json!({
        "response": full_response,
        "cost_usd": cost_usd,
        "duration_ms": duration_ms,
        "stop_reason": stop_reason,
        "provider": "claude_code",
        "model": model.display_name(),
    });

    let response_json = serialize_with_fallback(&result, "{}");
    Ok(json_raw(StatusCode::OK, response_json))
}
