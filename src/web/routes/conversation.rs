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

/// Parse conversation content (from database) to ChatMessage array
fn parse_conversation_to_messages(content: &str) -> Vec<ChatMessage> {
    let mut messages = Vec::new();
    let mut current_role = String::new();
    let mut current_content = String::new();
    let mut sequence = 0u64;

    for line in content.lines() {
        // Check for role headers (uppercase followed by colon)
        if line == "SYSTEM:" || line == "USER:" || line == "ASSISTANT:" {
            // Save previous message if any
            if !current_role.is_empty() && !current_content.trim().is_empty() {
                messages.push(ChatMessage {
                    id: format!("msg_{}", sequence),
                    role: current_role.to_lowercase(),
                    content: current_content.trim().to_string(),
                    timestamp: sequence,
                });
                sequence += 1;
            }
            current_role = line.trim_end_matches(':').to_string();
            current_content.clear();
        } else {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    // Don't forget the last message
    if !current_role.is_empty() && !current_content.trim().is_empty() {
        messages.push(ChatMessage {
            id: format!("msg_{}", sequence),
            role: current_role.to_lowercase(),
            content: current_content.trim().to_string(),
            timestamp: sequence,
        });
    }

    messages
}

pub async fn handle_get_conversation(
    path: &str,
    #[cfg(not(feature = "mock"))] _llama_state: crate::web::models::SharedLlamaState,
    #[cfg(feature = "mock")] _llama_state: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    // Extract filename from path: /api/conversation/{filename}
    let filename = &path[18..]; // Remove "/api/conversation/"

    // Remove .txt extension if present for database lookup
    let conversation_id = filename.trim_end_matches(".txt");

    match db.get_conversation_as_text(conversation_id) {
        Ok(content) => {
            let messages = parse_conversation_to_messages(&content);
            let response = ConversationContentResponse {
                content: content.clone(),
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
    #[cfg(not(feature = "mock"))] _llama_state: crate::web::models::SharedLlamaState,
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
                    display_name: format!("Chat {}", timestamp_part),
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

pub async fn handle_delete_conversation(
    path: &str,
    #[cfg(not(feature = "mock"))] _llama_state: crate::web::models::SharedLlamaState,
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
