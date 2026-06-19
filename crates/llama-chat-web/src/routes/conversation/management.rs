use super::*;
use crate::worker_pool::{resolve_bridge_for_conversation, WorkerPool};
use serde::Deserialize;

#[derive(Deserialize)]
struct CreateConversationRequest {
    title: Option<String>,
    worker_id: Option<String>,
}

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
        Some(s) => s,
        None => return Ok(json_error(StatusCode::BAD_REQUEST, "Invalid path")),
    };

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

pub async fn handle_compact_conversation(
    conversation_id: &str,
    pool: WorkerPool,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let bridge = match resolve_bridge_for_conversation(&pool, &db, Some(conversation_id)).await {
        Ok(bridge) => bridge,
        Err(e) => return Ok(json_error(StatusCode::SERVICE_UNAVAILABLE, &e)),
    };

    match bridge.compact_conversation(conversation_id).await {
        Ok(()) => Ok(json_raw(StatusCode::OK, r#"{"ok":true}"#.to_string())),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

pub async fn handle_update_summary(
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
    let text = match json.get("text").and_then(|t| t.as_str()) {
        Some(t) => t,
        None => return Ok(json_error(StatusCode::BAD_REQUEST, "text is required")),
    };
    match db.update_compaction_summary(conversation_id, text) {
        Ok(()) => Ok(json_raw(StatusCode::OK, r#"{"ok":true}"#.to_string())),
        Err(e) => Ok(json_error(StatusCode::NOT_FOUND, &e)),
    }
}

pub async fn handle_delete_summary(
    conversation_id: &str,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    match db.delete_compaction_summary(conversation_id) {
        Ok(()) => Ok(json_raw(StatusCode::OK, r#"{"ok":true}"#.to_string())),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

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
        rusqlite::params![title, chrono::Utc::now().timestamp(), conversation_id],
    ) {
        Ok(0) => Ok(json_error(StatusCode::NOT_FOUND, "Conversation not found")),
        Ok(_) => Ok(json_raw(
            StatusCode::OK,
            serde_json::to_string(&json!({"success": true, "title": title})).unwrap(),
        )),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("Failed: {e}"))),
    }
}

pub async fn handle_create_conversation(
    req: Request<Body>,
    pool: WorkerPool,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let create: CreateConversationRequest = match crate::request_parsing::parse_json_body(req.into_body()).await {
        Ok(body) => body,
        Err(_) => CreateConversationRequest {
            title: None,
            worker_id: None,
        },
    };
    let title = create.title.as_deref().unwrap_or("New conversation");
    let normalized_worker_id = create
        .worker_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty() && *id != "default")
        .map(str::to_string);

    if let Some(worker_id) = normalized_worker_id.as_deref() {
        if pool.get(worker_id).is_none() {
            return Ok(json_error(StatusCode::NOT_FOUND, "Worker not found"));
        }
    }

    let conv_id = format!("chat_{}", chrono::Local::now().format("%Y-%m-%d-%H-%M-%S-%3f"));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let conn = db.connection();
    match conn.execute(
        "INSERT INTO conversations (id, title, created_at, updated_at, worker_id) VALUES (?1, ?2, ?3, ?3, ?4)",
        rusqlite::params![conv_id, title, now, normalized_worker_id],
    ) {
        Ok(_) => Ok(json_raw(
            StatusCode::OK,
            serde_json::to_string(&json!({"id": conv_id, "title": title, "worker_id": create.worker_id})).unwrap(),
        )),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("Failed: {e}"))),
    }
}

pub async fn handle_delete_conversation(
    path: &str,
    #[cfg(not(feature = "mock"))] _llama_state: llama_chat_worker::worker::worker_bridge::SharedWorkerBridge,
    #[cfg(feature = "mock")] _llama_state: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let filename = &path["/api/conversations/".len()..];
    if filename.contains("..") || filename.contains("/") || filename.contains("\\") {
        return Ok(json_error(StatusCode::BAD_REQUEST, "Invalid filename"));
    }
    if !filename.starts_with("chat_") {
        return Ok(json_error(
            StatusCode::BAD_REQUEST,
            "Invalid conversation file",
        ));
    }

    let conversation_id = filename;
    match db.delete_conversation(conversation_id) {
        Ok(_) => {
            sys_info!("Deleted conversation: {}", conversation_id);
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

pub async fn handle_export_conversation(
    req: &Request<Body>,
    conversation_id: &str,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let format = crate::request_parsing::get_query_param(req.uri(), "format")
        .unwrap_or_else(|| "md".to_string());

    let conv_id = conversation_id;
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
        match db.delete_conversation(id) {
            Ok(_) => deleted += 1,
            Err(_) => failed += 1,
        }
    }

    Ok(json_raw(
        StatusCode::OK,
        serde_json::to_string(&json!({"deleted": deleted, "failed": failed, "total": ids.len()})).unwrap(),
    ))
}

pub async fn handle_conversation_token_analysis(
    conversation_id: &str,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let conv_id = conversation_id;
    let messages = match db.get_messages(conv_id) {
        Ok(m) => m,
        Err(e) => return Ok(json_error(StatusCode::NOT_FOUND, &e.to_string())),
    };

    let mut total_chars = 0usize;
    let mut user_chars = 0usize;
    let mut assistant_chars = 0usize;
    let mut system_chars = 0usize;
    let mut tool_call_count = 0usize;
    let mut tool_response_chars = 0usize;
    let mut compacted_count = 0usize;
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
                let tc_count = msg.content.matches("<tool_call>").count()
                    + msg
                        .content
                        .matches("\"name\"")
                        .count()
                        .min(msg.content.matches("<tool_call>").count().max(1));
                tool_call_count += tc_count;
            }
            "system" => system_chars += chars,
            _ => {}
        }

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
                "pct": (system_chars * 100).checked_div(total_chars).unwrap_or(0)
            },
            "user": {
                "chars": user_chars,
                "tokens": est_tokens(user_chars),
                "pct": (user_chars * 100).checked_div(total_chars).unwrap_or(0)
            },
            "assistant": {
                "chars": assistant_chars,
                "tokens": est_tokens(assistant_chars),
                "pct": (assistant_chars * 100).checked_div(total_chars).unwrap_or(0)
            },
            "tool_responses": {
                "chars": tool_response_chars,
                "tokens": est_tokens(tool_response_chars),
                "pct": (tool_response_chars * 100).checked_div(total_chars).unwrap_or(0)
            }
        },
        "tool_calls": tool_call_count
    });

    Ok(json_raw(
        StatusCode::OK,
        serde_json::to_string(&analysis).unwrap(),
    ))
}
