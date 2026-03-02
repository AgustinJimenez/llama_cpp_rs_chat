// Conversation route handlers

use hyper::{Body, Response, StatusCode};
use std::convert::Infallible;

use crate::web::{
    database::SharedDatabase,
    models::{ChatMessage, ConversationContentResponse, ConversationFile, ConversationsResponse},
    response_helpers::{json_error, json_raw, serialize_with_fallback},
};

// Import logging macros
use crate::{sys_error, sys_info};

pub async fn handle_get_conversation(
    path: &str,
    #[cfg(not(feature = "mock"))] _llama_state: crate::web::worker::worker_bridge::SharedWorkerBridge,
    #[cfg(feature = "mock")] _llama_state: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    // Extract filename from path: /api/conversation/{filename}
    let filename = &path[18..]; // Remove "/api/conversation/"

    // Remove .txt extension if present for database lookup
    let conversation_id = filename.trim_end_matches(".txt");

    // Load messages directly from DB to preserve timing metadata
    match db.get_messages(conversation_id) {
        Ok(records) => {
            let mut messages = Vec::new();
            for (i, rec) in records.iter().enumerate() {
                messages.push(ChatMessage {
                    id: format!("msg_{i}"),
                    role: rec.role.to_lowercase(),
                    content: rec.content.clone(),
                    timestamp: i as u64,
                    prompt_tok_per_sec: rec.prompt_tok_per_sec,
                    gen_tok_per_sec: rec.gen_tok_per_sec,
                    gen_eval_ms: rec.gen_eval_ms,
                    gen_tokens: rec.gen_tokens,
                    prompt_eval_ms: rec.prompt_eval_ms,
                    prompt_tokens: rec.prompt_tokens,
                });
            }
            // Also provide text content for backward compatibility
            let content = db.get_conversation_as_text(conversation_id).unwrap_or_default();
            let response = ConversationContentResponse {
                content,
                messages,
            };

            let response_json =
                serialize_with_fallback(&response, r#"{"content":"","messages":[]}"#);

            Ok(json_raw(StatusCode::OK, response_json))
        }
        Err(_) => Ok(json_error(StatusCode::NOT_FOUND, "Conversation not found")),
    }
}

pub async fn handle_get_conversations(
    #[cfg(not(feature = "mock"))] _llama_state: crate::web::worker::worker_bridge::SharedWorkerBridge,
    #[cfg(feature = "mock")] _llama_state: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    // Fetch conversations from database
    let mut conversations = Vec::new();

    match db.list_conversations() {
        Ok(records) => {
            for record in records {
                // Extract timestamp from conversation ID (chat_YYYY-MM-DD-HH-mm-ss-SSS)
                let timestamp_part = record
                    .id
                    .strip_prefix("chat_")
                    .unwrap_or(&record.id)
                    .to_string();

                conversations.push(ConversationFile {
                    name: format!("{}.txt", record.id), // Keep .txt extension for API compatibility
                    display_name: format!("Chat {timestamp_part}"),
                    timestamp: timestamp_part,
                });
            }
        }
        Err(e) => {
            sys_error!("Failed to list conversations from database: {}", e);
        }
    }

    // Conversations are already sorted by created_at DESC from database
    let response = ConversationsResponse { conversations };
    let response_json = serialize_with_fallback(&response, r#"{"conversations":[]}"#);

    Ok(json_raw(StatusCode::OK, response_json))
}

/// GET /api/conversations/:id/metrics — return generation metrics logs for a conversation
pub async fn handle_get_conversation_metrics(
    path: &str,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    // Extract conversation ID from path: /api/conversations/{id}/metrics
    let stripped = match path.strip_prefix("/api/conversations/") {
        Some(s) => s,
        None => return Ok(json_error(StatusCode::BAD_REQUEST, "Invalid path")),
    };
    let conv_id = match stripped.strip_suffix("/metrics") {
        Some(s) => s.trim_end_matches(".txt"),
        None => return Ok(json_error(StatusCode::BAD_REQUEST, "Invalid path")),
    };

    match db.get_logs_for_conversation(conv_id) {
        Ok(logs) => {
            // Filter to metrics entries only
            let metrics: Vec<_> = logs.into_iter().filter(|l| l.level == "metrics").collect();
            let response_json = serialize_with_fallback(&metrics, "[]");
            Ok(json_raw(StatusCode::OK, response_json))
        }
        Err(e) => {
            sys_error!("Failed to get metrics for {}: {}", conv_id, e);
            Ok(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to retrieve metrics",
            ))
        }
    }
}

pub async fn handle_delete_conversation(
    path: &str,
    #[cfg(not(feature = "mock"))] _llama_state: crate::web::worker::worker_bridge::SharedWorkerBridge,
    #[cfg(feature = "mock")] _llama_state: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    // Extract filename from path
    let filename = &path["/api/conversations/".len()..];

    // Validate filename to prevent path traversal
    if filename.contains("..") || filename.contains("/") || filename.contains("\\") {
        return Ok(json_error(StatusCode::BAD_REQUEST, "Invalid filename"));
    }

    // Only allow deleting conversation files that start with "chat_"
    if !filename.starts_with("chat_") {
        return Ok(json_error(
            StatusCode::BAD_REQUEST,
            "Invalid conversation file",
        ));
    }

    // Remove .txt extension if present for database lookup
    let conversation_id = filename.trim_end_matches(".txt");

    match db.delete_conversation(conversation_id) {
        Ok(_) => {
            sys_info!("Deleted conversation: {}", conversation_id);
            Ok(json_raw(StatusCode::OK, r#"{"success":true}"#.to_string()))
        }
        Err(e) => {
            sys_error!("Failed to delete conversation: {}", e);
            Ok(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to delete conversation",
            ))
        }
    }
}
