// OpenAI-compatible server endpoints
// Implements /v1/models and /v1/chat/completions so third-party clients
// like openclaw can connect to this app as if it were an OpenAI API server.

#[cfg(not(feature = "mock"))]
use hyper::body::Bytes;
use hyper::{Body, Request, Response, StatusCode};
use serde::Deserialize;
use std::convert::Infallible;

use crate::request_parsing::parse_json_body;
use crate::response_helpers::json_error;

#[cfg(not(feature = "mock"))]
use llama_chat_worker::worker::worker_bridge::{GenerationResult, SharedWorkerBridge};

// ── Request types ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct OpenAiMessage {
    role: String,
    content: serde_json::Value, // string or array of content parts
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct OpenAiChatRequest {
    messages: Vec<OpenAiMessage>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    model: Option<String>,
}

// ── /v1/models ─────────────────────────────────────────────────────────────────

pub async fn handle_get_models(
    #[cfg(not(feature = "mock"))] bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
) -> Result<Response<Body>, Infallible> {
    #[cfg(not(feature = "mock"))]
    {
        let model_id = get_model_id(&bridge).await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let body = serde_json::json!({
            "object": "list",
            "data": [{
                "id": model_id,
                "object": "model",
                "created": now,
                "owned_by": "local"
            }]
        });
        Ok(json_body(StatusCode::OK, body))
    }

    #[cfg(feature = "mock")]
    {
        let body = serde_json::json!({
            "object": "list",
            "data": [{ "id": "local-model", "object": "model", "created": 0, "owned_by": "local" }]
        });
        Ok(json_body(StatusCode::OK, body))
    }
}

// ── /v1/chat/completions ───────────────────────────────────────────────────────

pub async fn handle_post_chat_completions(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
) -> Result<Response<Body>, Infallible> {
    let oai_req: OpenAiChatRequest = match parse_json_body(req.into_body()).await {
        Ok(r) => r,
        Err(e) => return Ok(e),
    };

    if oai_req.messages.is_empty() {
        return Ok(json_error(StatusCode::BAD_REQUEST, "messages must not be empty"));
    }

    // Build a single user message from the OpenAI messages array.
    // Strategy: concatenate all messages with role prefixes, then use the
    // last user message as the actual prompt (the worker handles its own history).
    // For simplicity we pass the full conversation as a single formatted prompt
    // when there are system/assistant turns mixed in.
    let user_prompt = build_user_prompt(&oai_req.messages);

    #[cfg(not(feature = "mock"))]
    {
        let model_id = oai_req.model.unwrap_or_else(|| "local-model".to_string());

        if oai_req.stream {
            stream_response(bridge, user_prompt, model_id).await
        } else {
            blocking_response(bridge, user_prompt, model_id).await
        }
    }

    #[cfg(feature = "mock")]
    {
        let _ = user_prompt;
        let body = serde_json::json!({
            "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
            "object": "chat.completion",
            "created": 0u64,
            "model": "local-model",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "mock response" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0 }
        });
        Ok(json_body(StatusCode::OK, body))
    }
}

// ── Streaming response ─────────────────────────────────────────────────────────

#[cfg(not(feature = "mock"))]
async fn stream_response(
    bridge: SharedWorkerBridge,
    user_prompt: String,
    model_id: String,
) -> Result<Response<Body>, Infallible> {
    let (mut sender, body) = Body::channel();
    let completion_id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    tokio::spawn(async move {
        let (mut token_rx, done_rx) = match bridge.generate(user_prompt, None, false, None, None).await {
            Ok(rx) => rx,
            Err(e) => {
                let chunk = error_chunk(&completion_id, &model_id, created, &e);
                let _ = sender.send_data(Bytes::from(format!("data: {chunk}\n\n"))).await;
                let _ = sender.send_data(Bytes::from("data: [DONE]\n\n")).await;
                return;
            }
        };

        while let Some(token_data) = token_rx.recv().await {
            let token = &token_data.token;
            let chunk = delta_chunk(&completion_id, &model_id, created, token);
            if sender.send_data(Bytes::from(format!("data: {chunk}\n\n"))).await.is_err() {
                return;
            }
        }

        let finish_reason = match done_rx.await {
            Ok(GenerationResult::Complete { finish_reason, .. }) => {
                finish_reason.unwrap_or_else(|| "stop".to_string())
            }
            Ok(GenerationResult::Cancelled) => "stop".to_string(),
            Ok(GenerationResult::Error(e)) => {
                let chunk = error_chunk(&completion_id, &model_id, created, &e);
                let _ = sender.send_data(Bytes::from(format!("data: {chunk}\n\n"))).await;
                let _ = sender.send_data(Bytes::from("data: [DONE]\n\n")).await;
                return;
            }
            Err(_) => "stop".to_string(),
        };

        // Final chunk with finish_reason
        let final_chunk = serde_json::json!({
            "id": completion_id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model_id,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": finish_reason
            }]
        });
        let _ = sender
            .send_data(Bytes::from(format!("data: {final_chunk}\n\n")))
            .await;
        let _ = sender.send_data(Bytes::from("data: [DONE]\n\n")).await;
    });

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("access-control-allow-origin", "*")
        .header("connection", "keep-alive")
        .header("x-accel-buffering", "no")
        .body(body)
        .unwrap())
}

// ── Non-streaming (blocking) response ─────────────────────────────────────────

#[cfg(not(feature = "mock"))]
async fn blocking_response(
    bridge: SharedWorkerBridge,
    user_prompt: String,
    model_id: String,
) -> Result<Response<Body>, Infallible> {
    let completion_id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let (mut token_rx, done_rx) = match bridge.generate(user_prompt, None, false, None, None).await {
        Ok(rx) => rx,
        Err(e) => {
            let body = serde_json::json!({
                "error": { "message": e, "type": "server_error", "code": 500 }
            });
            return Ok(json_body(StatusCode::INTERNAL_SERVER_ERROR, body));
        }
    };

    let mut full_text = String::new();
    while let Some(token_data) = token_rx.recv().await {
        full_text.push_str(&token_data.token);
    }

    let finish_reason = match done_rx.await {
        Ok(GenerationResult::Complete { finish_reason, tokens_used, gen_tokens, prompt_tokens, .. }) => {
            let body = serde_json::json!({
                "id": completion_id,
                "object": "chat.completion",
                "created": created,
                "model": model_id,
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": full_text },
                    "finish_reason": finish_reason.unwrap_or_else(|| "stop".to_string())
                }],
                "usage": {
                    "prompt_tokens": prompt_tokens.unwrap_or(0),
                    "completion_tokens": gen_tokens.unwrap_or(0),
                    "total_tokens": tokens_used
                }
            });
            return Ok(json_body(StatusCode::OK, body));
        }
        Ok(GenerationResult::Cancelled) => "stop",
        Ok(GenerationResult::Error(e)) => {
            let body = serde_json::json!({
                "error": { "message": e, "type": "server_error", "code": 500 }
            });
            return Ok(json_body(StatusCode::INTERNAL_SERVER_ERROR, body));
        }
        Err(_) => "stop",
    };

    let body = serde_json::json!({
        "id": completion_id,
        "object": "chat.completion",
        "created": created,
        "model": model_id,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": full_text },
            "finish_reason": finish_reason
        }],
        "usage": { "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0 }
    });
    Ok(json_body(StatusCode::OK, body))
}

// ── Helpers ────────────────────────────────────────────────────────────────────

/// Convert OpenAI messages array to a single user prompt string.
///
/// If the array is just one user message we pass it through directly.
/// If there are multiple turns we prefix each with the role so the local
/// model can still see the full context; the worker appends its own system
/// prompt on top.
fn build_user_prompt(messages: &[OpenAiMessage]) -> String {
    if messages.len() == 1 && messages[0].role == "user" {
        return extract_text_content(&messages[0].content);
    }

    messages
        .iter()
        .map(|m| {
            let text = extract_text_content(&m.content);
            format!("{}: {}", m.role, text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extract plain text from an OpenAI content field (string or array of parts).
fn extract_text_content(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter_map(|p| {
                if p.get("type").and_then(|t| t.as_str()) == Some("text") {
                    p.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

/// Return the loaded model's id string (filename without extension, or a generic fallback).
#[cfg(not(feature = "mock"))]
async fn get_model_id(bridge: &SharedWorkerBridge) -> String {
    if let Some(meta) = bridge.model_status().await {
        if meta.loaded {
            // Prefer general_name; fall back to file stem of model_path
            if let Some(name) = meta.general_name {
                return name;
            }
            return std::path::Path::new(&meta.model_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("local-model")
                .to_string();
        }
    }
    "local-model".to_string()
}

fn json_body(status: StatusCode, body: serde_json::Value) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("access-control-allow-origin", "*")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[cfg(not(feature = "mock"))]
fn delta_chunk(id: &str, model: &str, created: u64, token: &str) -> String {
    serde_json::json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": { "content": token },
            "finish_reason": null
        }]
    })
    .to_string()
}

#[cfg(not(feature = "mock"))]
fn error_chunk(id: &str, model: &str, created: u64, error: &str) -> String {
    serde_json::json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": { "content": format!("[Error: {}]", error) },
            "finish_reason": "stop"
        }]
    })
    .to_string()
}
