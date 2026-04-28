// Conversation route handlers

use hyper::{Body, Request, Response, StatusCode};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;

use crate::web::{
    database::SharedDatabase,
    models::{ChatMessage, ConversationContentResponse, ConversationFile, ConversationsResponse},
    response_helpers::{json_error, json_raw, serialize_with_fallback},
};

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
                    timestamp: rec.timestamp * 1000,
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
            let (provider_id, provider_session_id) = db
                .get_conversation_provider_session(conversation_id)
                .unwrap_or((None, None));
            let response = ConversationContentResponse {
                content,
                messages,
                provider_id,
                provider_session_id,
            };

            let response_json =
                serialize_with_fallback(&response, r#"{"content":"","messages":[]}"#);

            Ok(json_raw(StatusCode::OK, response_json))
        }
        Err(_) => Ok(json_error(StatusCode::NOT_FOUND, "Conversation not found")),
    }
}

pub async fn handle_get_conversations(
    req: &Request<Body>,
    #[cfg(not(feature = "mock"))] _llama_state: crate::web::worker::worker_bridge::SharedWorkerBridge,
    #[cfg(feature = "mock")] _llama_state: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    // Parse optional search query from URL
    let query = crate::web::request_parsing::get_query_param(req.uri(), "q")
        .map(|v| v.to_lowercase());

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

                // Use DB title for display_name when available
                let title = db.get_conversation_title(&record.id).ok().flatten();
                let display_name = title
                    .as_deref()
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| format!("Chat {timestamp_part}"));

                conversations.push(ConversationFile {
                    name: format!("{}.txt", record.id),
                    display_name,
                    timestamp: timestamp_part,
                    title,
                    provider_id: record.provider_id.clone(),
                });
            }
        }
        Err(e) => {
            sys_error!("Failed to list conversations from database: {}", e);
        }
    }

    // If search query provided, filter by title, display_name, or ID containing the query
    if let Some(ref q) = query {
        conversations.retain(|c| {
            c.display_name.to_lowercase().contains(q)
                || c.name.to_lowercase().contains(q)
                || c.title.as_ref().map(|t| t.to_lowercase().contains(q)).unwrap_or(false)
        });
    }

    // Conversations are already sorted by created_at DESC from database
    let response = ConversationsResponse { conversations };
    let response_json = serialize_with_fallback(&response, r#"{"conversations":[]}"#);

    Ok(json_raw(StatusCode::OK, response_json))
}

/// GET /api/conversations/:id/events — return in-memory event log for a conversation
pub async fn handle_get_conversation_events(
    path: &str,
    bridge: crate::web::worker::worker_bridge::SharedWorkerBridge,
) -> Result<Response<Body>, Infallible> {
    let stripped = match path.strip_prefix("/api/conversations/") {
        Some(s) => s,
        None => return Ok(json_error(StatusCode::BAD_REQUEST, "Invalid path")),
    };
    let conv_id = match stripped.strip_suffix("/events") {
        Some(s) => s.trim_end_matches(".txt"),
        None => return Ok(json_error(StatusCode::BAD_REQUEST, "Invalid path")),
    };

    match bridge.get_conversation_events(conv_id).await {
        Ok(events) => {
            let response_json = serialize_with_fallback(&events, "[]");
            Ok(json_raw(StatusCode::OK, response_json))
        }
        Err(e) => {
            sys_error!("Failed to get events for {}: {}", conv_id, e);
            Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to retrieve events"))
        }
    }
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

/// POST /api/conversations/:id/truncate — delete messages from a given sequence_order onward
pub async fn handle_truncate_conversation(
    req: hyper::Request<Body>,
    path: &str,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let stripped = match path.strip_prefix("/api/conversations/") {
        Some(s) => s,
        None => return Ok(json_error(StatusCode::BAD_REQUEST, "Invalid path")),
    };
    let conv_id = match stripped.strip_suffix("/truncate") {
        Some(s) => s.trim_end_matches(".txt"),
        None => return Ok(json_error(StatusCode::BAD_REQUEST, "Invalid path")),
    };

    // Parse request body
    let body_bytes = match hyper::body::to_bytes(req.into_body()).await {
        Ok(b) => b,
        Err(_) => return Ok(json_error(StatusCode::BAD_REQUEST, "Failed to read body")),
    };
    let from_sequence: i32 = match serde_json::from_slice::<serde_json::Value>(&body_bytes)
        .ok()
        .and_then(|v| v.get("from_sequence")?.as_i64())
    {
        Some(n) => n as i32,
        None => {
            return Ok(json_error(
                StatusCode::BAD_REQUEST,
                "Missing or invalid from_sequence",
            ))
        }
    };

    match db.truncate_messages(conv_id, from_sequence) {
        Ok(deleted) => {
            let _ = db.set_conversation_provider_session_id(conv_id, None, None);
            sys_info!(
                "Truncated {} messages from conversation {} at seq {}",
                deleted,
                conv_id,
                from_sequence
            );
            Ok(json_raw(
                StatusCode::OK,
                format!(r#"{{"success":true,"deleted":{deleted}}}"#),
            ))
        }
        Err(e) => {
            sys_error!("Failed to truncate conversation: {}", e);
            Ok(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to truncate conversation",
            ))
        }
    }
}

/// PATCH /api/conversations/{id}/title — rename a conversation
pub async fn handle_rename_conversation(
    req: Request<Body>,
    conversation_id: &str,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let body = match hyper::body::to_bytes(req.into_body()).await {
        Ok(b) => b,
        Err(_) => return Ok(json_error(StatusCode::BAD_REQUEST, "Failed to read body")),
    };
    let json: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return Ok(json_error(StatusCode::BAD_REQUEST, "Invalid JSON")),
    };
    let title = match json.get("title").and_then(|t| t.as_str()) {
        Some(t) if !t.is_empty() => t,
        _ => return Ok(json_error(StatusCode::BAD_REQUEST, "title is required")),
    };

    let conn = db.connection();
    match conn.execute(
        "UPDATE conversations SET title = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![title, chrono::Utc::now().timestamp(), conversation_id.trim_end_matches(".txt")],
    ) {
        Ok(0) => Ok(json_error(StatusCode::NOT_FOUND, "Conversation not found")),
        Ok(_) => Ok(json_raw(
            StatusCode::OK,
            serde_json::to_string(&json!({"success": true, "title": title})).unwrap(),
        )),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("Failed: {e}"))),
    }
}

/// POST /api/conversations — create a new empty conversation
pub async fn handle_create_conversation(
    req: Request<Body>,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let body = hyper::body::to_bytes(req.into_body()).await.unwrap_or_default();
    let json: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
    let title = json.get("title").and_then(|t| t.as_str()).unwrap_or("New conversation");

    let conv_id = format!("chat_{}", chrono::Local::now().format("%Y-%m-%d-%H-%M-%S-%3f"));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let conn = db.connection();
    match conn.execute(
        "INSERT INTO conversations (id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        rusqlite::params![conv_id, title, now],
    ) {
        Ok(_) => Ok(json_raw(
            StatusCode::OK,
            serde_json::to_string(&json!({"id": conv_id, "title": title})).unwrap(),
        )),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("Failed: {e}"))),
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
            // Clean up persisted screenshot images for this conversation
            let images_dir = std::path::PathBuf::from("assets/images").join(conversation_id);
            if images_dir.exists() {
                if let Err(e) = std::fs::remove_dir_all(&images_dir) {
                    sys_error!("Failed to clean up images for {}: {}", conversation_id, e);
                } else {
                    sys_info!("Cleaned up images for {}", conversation_id);
                }
            }
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

/// GET /api/conversation/{id}/export — export conversation as markdown or JSON
pub async fn handle_export_conversation(
    req: &Request<Body>,
    conversation_id: &str,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let format = crate::web::request_parsing::get_query_param(req.uri(), "format")
        .unwrap_or_else(|| "md".to_string());

    let conv_id = conversation_id.trim_end_matches(".txt");
    let messages = match db.get_messages(conv_id) {
        Ok(msgs) => msgs,
        Err(e) => return Ok(json_error(StatusCode::NOT_FOUND, &format!("Conversation not found: {e}"))),
    };

    match format.as_str() {
        "json" => {
            let json_msgs: Vec<serde_json::Value> = messages.iter()
                .filter(|m| !m.compacted)
                .map(|m| json!({
                    "role": m.role,
                    "content": m.content,
                    "timestamp": m.timestamp,
                }))
                .collect();
            let body = json!({ "conversation_id": conv_id, "messages": json_msgs });
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .header("Content-Disposition", format!("attachment; filename=\"{conv_id}.json\""))
                .body(Body::from(serde_json::to_string_pretty(&body).unwrap()))
                .unwrap())
        }
        _ => {
            // Markdown format
            let mut md = format!("# Conversation: {conv_id}\n\n");
            for m in &messages {
                if m.compacted { continue; }
                let role_label = match m.role.as_str() {
                    "user" => "**User**",
                    "assistant" => "**Assistant**",
                    "system" => "**System**",
                    _ => &m.role,
                };
                md.push_str(&format!("### {role_label}\n\n{}\n\n---\n\n", m.content));
            }
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/markdown; charset=utf-8")
                .header("Content-Disposition", format!("attachment; filename=\"{conv_id}.md\""))
                .body(Body::from(md))
                .unwrap())
        }
    }
}

/// DELETE /api/conversations/batch — delete multiple conversations
pub async fn handle_batch_delete_conversations(
    req: Request<Body>,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let body = match hyper::body::to_bytes(req.into_body()).await {
        Ok(b) => b,
        Err(_) => return Ok(json_error(StatusCode::BAD_REQUEST, "Failed to read body")),
    };
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return Ok(json_error(StatusCode::BAD_REQUEST, "Invalid JSON")),
    };

    let ids: Vec<String> = match parsed.get("ids").and_then(|i| i.as_array()) {
        Some(arr) => arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect(),
        None => return Ok(json_error(StatusCode::BAD_REQUEST, "ids array is required")),
    };

    let mut deleted = 0;
    let mut failed = 0;
    for id in &ids {
        match db.delete_conversation(id.trim_end_matches(".txt")) {
            Ok(_) => deleted += 1,
            Err(_) => failed += 1,
        }
    }

    Ok(json_raw(
        StatusCode::OK,
        serde_json::to_string(&json!({"deleted": deleted, "failed": failed, "total": ids.len()})).unwrap(),
    ))
}

/// GET /api/conversations/:id/token-analysis — estimate token usage breakdown
pub async fn handle_conversation_token_analysis(
    conversation_id: &str,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let conv_id = conversation_id.trim_end_matches(".txt");
    let messages = match db.get_messages(conv_id) {
        Ok(m) => m,
        Err(e) => return Ok(json_error(StatusCode::NOT_FOUND, &format!("{e}"))),
    };

    let mut total_chars = 0usize;
    let mut user_chars = 0usize;
    let mut assistant_chars = 0usize;
    let mut system_chars = 0usize;
    let mut tool_call_count = 0usize;
    let mut tool_response_chars = 0usize;
    let mut compacted_count = 0usize;

    // Track per-tool token usage
    let mut _tool_usage: HashMap<String, (usize, usize)> = HashMap::new();

    for msg in &messages {
        let chars = msg.content.len();
        total_chars += chars;

        if msg.compacted {
            compacted_count += 1;
            continue;
        }

        match msg.role.as_str() {
            "user" => user_chars += chars,
            "assistant" => {
                assistant_chars += chars;
                // Count tool calls in assistant messages
                let tc_count = msg.content.matches("<tool_call>").count()
                    + msg
                        .content
                        .matches("\"name\"")
                        .count()
                        .min(msg.content.matches("<tool_call>").count().max(1));
                tool_call_count += tc_count;
            }
            "system" => {
                system_chars += chars;
            }
            _ => {}
        }

        // Detect tool responses
        if msg.content.contains("<tool_response>") || msg.content.starts_with("[TOOL_RESULTS]") {
            tool_response_chars += chars;
        }
    }

    let est_tokens = |c: usize| c / 4;

    let analysis = json!({
        "total_messages": messages.len(),
        "compacted_messages": compacted_count,
        "total_chars": total_chars,
        "total_tokens_estimate": est_tokens(total_chars),
        "breakdown": {
            "system": {
                "chars": system_chars,
                "tokens": est_tokens(system_chars),
                "pct": if total_chars > 0 { system_chars * 100 / total_chars } else { 0 }
            },
            "user": {
                "chars": user_chars,
                "tokens": est_tokens(user_chars),
                "pct": if total_chars > 0 { user_chars * 100 / total_chars } else { 0 }
            },
            "assistant": {
                "chars": assistant_chars,
                "tokens": est_tokens(assistant_chars),
                "pct": if total_chars > 0 { assistant_chars * 100 / total_chars } else { 0 }
            },
            "tool_responses": {
                "chars": tool_response_chars,
                "tokens": est_tokens(tool_response_chars),
                "pct": if total_chars > 0 { tool_response_chars * 100 / total_chars } else { 0 }
            }
        },
        "tool_calls": tool_call_count
    });

    Ok(json_raw(
        StatusCode::OK,
        serde_json::to_string(&analysis).unwrap(),
    ))
}
