// Conversation route handlers

use hyper::{Body, Response, StatusCode};
use std::convert::Infallible;

use crate::web::{
    models::{ConversationFile, ConversationsResponse, ConversationContentResponse, ChatMessage},
    database::SharedDatabase,
};

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
    #[cfg(not(feature = "mock"))]
    _llama_state: crate::web::models::SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
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

            let response_json = match serde_json::to_string(&response) {
                Ok(json) => json,
                Err(_) => r#"{"content":"","messages":[]}"#.to_string(),
            };

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(response_json))
                .unwrap())
        }
        Err(_) => {
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"error":"Conversation not found"}"#))
                .unwrap())
        }
    }
}

pub async fn handle_get_conversations(
    #[cfg(not(feature = "mock"))]
    _llama_state: crate::web::models::SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    // Fetch conversations from database
    let mut conversations = Vec::new();

    match db.list_conversations() {
        Ok(records) => {
            for record in records {
                // Extract timestamp from conversation ID (chat_YYYY-MM-DD-HH-mm-ss-SSS)
                let timestamp_part = record.id
                    .strip_prefix("chat_")
                    .unwrap_or(&record.id)
                    .to_string();

                conversations.push(ConversationFile {
                    name: format!("{}.txt", record.id),  // Keep .txt extension for API compatibility
                    display_name: format!("Chat {}", timestamp_part),
                    timestamp: timestamp_part,
                });
            }
        }
        Err(e) => {
            eprintln!("Failed to list conversations from database: {}", e);
        }
    }

    // Conversations are already sorted by created_at DESC from database
    let response = ConversationsResponse { conversations };
    let response_json = match serde_json::to_string(&response) {
        Ok(json) => json,
        Err(_) => r#"{"conversations":[]}"#.to_string(),
    };

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .header("access-control-allow-origin", "*")
        .body(Body::from(response_json))
        .unwrap())
}

pub async fn handle_delete_conversation(
    path: &str,
    #[cfg(not(feature = "mock"))]
    _llama_state: crate::web::models::SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    // Extract filename from path
    let filename = &path["/api/conversations/".len()..];

    // Validate filename to prevent path traversal
    if filename.contains("..") || filename.contains("/") || filename.contains("\\") {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("content-type", "application/json")
            .header("access-control-allow-origin", "*")
            .body(Body::from(r#"{"error":"Invalid filename"}"#))
            .unwrap());
    }

    // Only allow deleting conversation files that start with "chat_"
    if !filename.starts_with("chat_") {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("content-type", "application/json")
            .header("access-control-allow-origin", "*")
            .body(Body::from(r#"{"error":"Invalid conversation file"}"#))
            .unwrap());
    }

    // Remove .txt extension if present for database lookup
    let conversation_id = filename.trim_end_matches(".txt");

    match db.delete_conversation(conversation_id) {
        Ok(_) => {
            println!("Deleted conversation: {}", conversation_id);
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"success":true}"#))
                .unwrap())
        }
        Err(e) => {
            eprintln!("Failed to delete conversation: {}", e);
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"error":"Failed to delete conversation"}"#))
                .unwrap())
        }
    }
}
