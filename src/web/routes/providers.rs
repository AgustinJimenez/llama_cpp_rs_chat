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

/// POST /api/providers/claude/stream — streaming generation via Claude Code CLI (SSE)
pub async fn handle_claude_stream(
    req: hyper::Request<Body>,
    db: crate::web::database::SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let body = match hyper::body::to_bytes(req.into_body()).await {
        Ok(b) => b,
        Err(_) => return Ok(json_error(StatusCode::BAD_REQUEST, "Failed to read body")),
    };

    #[derive(serde::Deserialize)]
    struct StreamRequest {
        prompt: String,
        model: Option<String>,
        max_turns: Option<u32>,
        session_id: Option<String>,
        conversation_id: Option<String>,
    }

    let request: StreamRequest = match serde_json::from_slice(&body) {
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
        None,
        request.session_id.as_deref(),
    ).await {
        Ok(rx) => rx,
        Err(e) => return Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("Failed: {e}"))),
    };

    let conv_id = request.conversation_id.unwrap_or_else(|| {
        format!("chat_{}", chrono::Local::now().format("%Y-%m-%d-%H-%M-%S-%3f"))
    });
    let prompt = request.prompt.clone();

    // Stream SSE events
    let (mut sse_tx, sse_body) = Body::channel();
    let db_clone = db.clone();
    let conv_id_clone = conv_id.clone();

    tokio::spawn(async move {
        let mut full_response = String::new();

        while let Some(token_data) = rx.recv().await {
            if token_data.is_done {
                // Send done event
                let done_json = serde_json::json!({
                    "type": "done",
                    "session_id": token_data.session_id,
                    "stop_reason": token_data.stop_reason,
                    "cost_usd": token_data.cost_usd,
                    "duration_ms": token_data.duration_ms,
                    "input_tokens": token_data.input_tokens,
                    "output_tokens": token_data.output_tokens,
                    "model": token_data.model_id,
                    "conversation_id": conv_id_clone,
                });
                let _ = sse_tx.send_data(hyper::body::Bytes::from(
                    format!("data: {}\n\n", done_json)
                )).await;

                // Save to DB
                if !full_response.is_empty() {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs()).unwrap_or(0);
                    let conn = db_clone.connection();
                    let _ = conn.execute(
                        "INSERT OR IGNORE INTO conversations (id, created_at, updated_at) VALUES (?1, ?2, ?3)",
                        rusqlite::params![conv_id_clone, now as i64, now as i64],
                    );
                    let next_seq = db_clone.get_messages(&conv_id_clone)
                        .map(|msgs| msgs.len() as i32 + 1).unwrap_or(1);
                    let _ = db_clone.insert_message(&conv_id_clone, "user", &prompt, now, next_seq);
                    let _ = db_clone.insert_message(&conv_id_clone, "assistant", &full_response, now, next_seq + 1);
                    let _ = conn.execute(
                        "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
                        rusqlite::params![now as i64, conv_id_clone],
                    );
                }
                break;
            }

            full_response.push_str(&token_data.token);

            // Send token event
            let token_json = serde_json::json!({
                "type": "token",
                "token": token_data.token,
            });
            if sse_tx.send_data(hyper::body::Bytes::from(
                format!("data: {}\n\n", token_json)
            )).await.is_err() {
                break; // Client disconnected
            }
        }
    });

    let response = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .body(sse_body)
        .unwrap();

    Ok(response)
}

/// POST /api/providers/claude/generate — generate via Claude Code CLI
pub async fn handle_claude_generate(
    req: hyper::Request<Body>,
    db: crate::web::database::SharedDatabase,
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
        session_id: Option<String>,
        conversation_id: Option<String>,
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
        request.session_id.as_deref(),
    ).await {
        Ok(rx) => rx,
        Err(e) => return Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("Failed to start Claude: {e}"))),
    };

    // Collect all tokens (non-streaming for now)
    let mut full_response = String::new();
    let mut cost_usd = None;
    let mut duration_ms = None;
    let mut stop_reason = None;
    let mut actual_model_id = None;
    let mut session_id = None;
    let mut input_tokens = None;
    let mut output_tokens = None;

    while let Some(token_data) = rx.recv().await {
        if token_data.model_id.is_some() {
            actual_model_id = token_data.model_id.clone();
        }
        if token_data.session_id.is_some() {
            session_id = token_data.session_id.clone();
        }
        if token_data.is_done {
            cost_usd = token_data.cost_usd;
            duration_ms = token_data.duration_ms;
            stop_reason = token_data.stop_reason;
            input_tokens = token_data.input_tokens;
            output_tokens = token_data.output_tokens;
            break;
        }
        full_response.push_str(&token_data.token);
    }

    let display_model = actual_model_id.as_deref().unwrap_or(model.display_name());

    // Save messages to DB for conversation persistence
    let conv_id = request.conversation_id.unwrap_or_else(|| {
        format!("chat_{}", chrono::Local::now().format("%Y-%m-%d-%H-%M-%S-%3f"))
    });
    if !full_response.is_empty() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Ensure conversation exists in DB
        let conn = db.connection();
        let _ = conn.execute(
            "INSERT OR IGNORE INTO conversations (id, created_at, updated_at, system_prompt, title) VALUES (?1, ?2, ?3, NULL, NULL)",
            rusqlite::params![conv_id, now as i64, now as i64],
        );

        // Get next sequence order
        let next_seq = db.get_messages(&conv_id)
            .map(|msgs| msgs.len() as i32 + 1)
            .unwrap_or(1);

        // Save user message
        match db.insert_message(&conv_id, "user", &request.prompt, now, next_seq) {
            Ok(_) => eprintln!("[CLAUDE_SAVE] Saved user message to {}", conv_id),
            Err(e) => eprintln!("[CLAUDE_SAVE] Failed to save user message: {}", e),
        }
        // Save assistant response
        match db.insert_message(&conv_id, "assistant", &full_response, now, next_seq + 1) {
            Ok(_) => eprintln!("[CLAUDE_SAVE] Saved assistant message to {} ({} chars)", conv_id, full_response.len()),
            Err(e) => eprintln!("[CLAUDE_SAVE] Failed to save assistant message: {}", e),
        }

        // Update conversation timestamp
        let _ = conn.execute(
            "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now as i64, conv_id],
        );
    }

    let result = serde_json::json!({
        "response": full_response,
        "cost_usd": cost_usd,
        "duration_ms": duration_ms,
        "stop_reason": stop_reason,
        "provider": "claude_code",
        "model": display_model,
        "session_id": session_id,
        "conversation_id": conv_id,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
    });

    let response_json = serialize_with_fallback(&result, "{}");
    Ok(json_raw(StatusCode::OK, response_json))
}
