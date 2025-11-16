// Conversation route handlers

use hyper::{Body, Response, StatusCode};
use std::convert::Infallible;
use std::fs;

use crate::web::{
    models::{ConversationFile, ConversationsResponse, ConversationContentResponse},
    conversation::parse_conversation_to_messages,
};

pub async fn handle_get_conversation(
    path: &str,
    #[cfg(not(feature = "mock"))]
    _llama_state: crate::web::models::SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    // Extract filename from path: /api/conversation/{filename}
    let filename = &path[18..]; // Remove "/api/conversation/"
    let conversations_dir = "assets/conversations";
    let file_path = format!("{}/{}", conversations_dir, filename);

    match fs::read_to_string(&file_path) {
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
) -> Result<Response<Body>, Infallible> {
    // Fetch conversation files from assets/conversations directory
    let conversations_dir = "assets/conversations";
    let mut conversations = Vec::new();

    match fs::read_dir(conversations_dir) {
        Ok(entries) => {
            for entry in entries {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if path.is_file() && path.extension().map_or(false, |ext| ext == "txt") {
                        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                            // Extract timestamp from filename (chat_YYYY-MM-DD-HH-mm-ss-SSS.txt)
                            if filename.starts_with("chat_") && filename.ends_with(".txt") {
                                let timestamp_part = &filename[5..filename.len()-4]; // Remove "chat_" and ".txt"

                                conversations.push(ConversationFile {
                                    name: filename.to_string(),
                                    display_name: format!("Chat {}", timestamp_part),
                                    timestamp: timestamp_part.to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to read conversations directory: {}", e);
        }
    }

    // Sort conversations by timestamp (newest first)
    conversations.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

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

    // Only allow deleting .txt files that start with "chat_"
    if !filename.starts_with("chat_") || !filename.ends_with(".txt") {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("content-type", "application/json")
            .header("access-control-allow-origin", "*")
            .body(Body::from(r#"{"error":"Invalid conversation file"}"#))
            .unwrap());
    }

    let file_path = format!("assets/conversations/{}", filename);

    match fs::remove_file(&file_path) {
        Ok(_) => {
            println!("Deleted conversation file: {}", filename);
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"success":true}"#))
                .unwrap())
        }
        Err(e) => {
            eprintln!("Failed to delete conversation file: {}", e);
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"error":"Failed to delete conversation"}"#))
                .unwrap())
        }
    }
}
