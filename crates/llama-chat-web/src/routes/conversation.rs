// Conversation route handlers

use hyper::{Body, Request, Response, StatusCode};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;

use llama_chat_db::SharedDatabase;
use llama_chat_types::models::{ChatMessage, ConversationContentResponse, ConversationFile, ConversationsResponse, ToolTiming};
use crate::response_helpers::{json_error, json_raw, serialize_with_fallback};

#[path = "conversation/management.rs"]
mod management;
pub use management::{
    handle_batch_delete_conversations, handle_compact_conversation,
    handle_conversation_token_analysis, handle_create_conversation,
    handle_delete_conversation, handle_delete_summary, handle_export_conversation,
    handle_rename_conversation, handle_truncate_conversation, handle_update_summary,
};

/// Load tool timing events from the event log for a conversation.
/// Returns timings in chronological order (1st = 1st tool call, etc.).
fn load_tool_timings(conversation_id: &str) -> Vec<ToolTiming> {
    llama_chat_db::event_log::get_events_fresh(conversation_id)
        .into_iter()
        .filter(|e| e.event_type == "tool_timing")
        .filter_map(|e| {
            let v: serde_json::Value = serde_json::from_str(&e.message).ok()?;
            Some(ToolTiming {
                name: v["name"].as_str()?.to_string(),
                duration_ms: v["duration_ms"].as_u64()?,
            })
        })
        .collect()
}

pub async fn handle_get_conversation(
    path: &str,
    #[cfg(not(feature = "mock"))] _llama_state: llama_chat_worker::worker::worker_bridge::SharedWorkerBridge,
    #[cfg(feature = "mock")] _llama_state: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    // Extract filename from path: /api/conversation/{filename}
    let filename = &path[18..]; // Remove "/api/conversation/"

    let conversation_id = filename;
    let records_result = db.get_messages(filename);

    // Load messages directly from DB to preserve timing metadata
    match records_result {
        Ok(records) => {
            // Rebuild messages: merge consecutive assistant tool_call + tool results
            // into a single assistant message with <tool_call>/<tool_response> tags
            // so the frontend renders the same widget UI as during live streaming.
            let mut messages = Vec::new();
            let mut i = 0;
            let mut msg_idx = 0;
            while i < records.len() {
                let rec = &records[i];
                if rec.role == "assistant" && rec.content.contains("\"tool_calls\":") && rec.content.starts_with("{") {
                    // Reconstruct streamed format: content + tool_call tags + tool_response tags
                    let mut combined = String::new();
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&rec.content) {
                        if let Some(text) = parsed.get("content").and_then(|c| c.as_str()) {
                            if !text.is_empty() { combined.push_str(text); }
                        }
                        if let Some(tcs) = parsed.get("tool_calls").and_then(|t| t.as_array()) {
                            for tc in tcs {
                                let name = tc.pointer("/function/name").and_then(|n| n.as_str()).unwrap_or("unknown");
                                let args = tc.pointer("/function/arguments").and_then(|a| a.as_str()).unwrap_or("{}");
                                combined.push_str(&format!("\n<tool_call>{{\"name\": \"{name}\", \"arguments\": {args}}}</tool_call>\n"));
                            }
                        }
                    }
                    // Consume following tool result messages
                    let mut j = i + 1;
                    while j < records.len() && records[j].role == "tool" {
                        let tool_content = records[j].content.split_once("\n\n")
                            .map(|(_, c)| c)
                            .unwrap_or(&records[j].content);
                        let display = &tool_content[..tool_content.len().min(2000)];
                        combined.push_str(&format!("\n<tool_response>{display}</tool_response>\n"));
                        j += 1;
                    }
                    if !combined.trim().is_empty() {
                        messages.push(ChatMessage {
                            id: format!("msg_{msg_idx}"),
                            role: "assistant".to_string(),
                            content: combined,
                            timestamp: rec.timestamp,
                            prompt_tok_per_sec: rec.prompt_tok_per_sec,
                            gen_tok_per_sec: rec.gen_tok_per_sec,
                            gen_eval_ms: rec.gen_eval_ms,
                            gen_tokens: rec.gen_tokens,
                            prompt_eval_ms: rec.prompt_eval_ms,
                            prompt_tokens: rec.prompt_tokens,
                            compacted: rec.compacted,
                            sequence_order: Some(rec.sequence_order),
                        });
                        msg_idx += 1;
                    }
                    i = j;
                } else if rec.role == "tool" {
                    // Orphan tool message (no preceding assistant) — skip
                    i += 1;
                } else {
                    messages.push(ChatMessage {
                        id: format!("msg_{msg_idx}"),
                        role: rec.role.to_lowercase(),
                        content: rec.content.clone(),
                        timestamp: rec.timestamp,
                        prompt_tok_per_sec: rec.prompt_tok_per_sec,
                        gen_tok_per_sec: rec.gen_tok_per_sec,
                        gen_eval_ms: rec.gen_eval_ms,
                        gen_tokens: rec.gen_tokens,
                        prompt_eval_ms: rec.prompt_eval_ms,
                        prompt_tokens: rec.prompt_tokens,
                        compacted: rec.compacted,
                        sequence_order: Some(rec.sequence_order),
                    });
                    msg_idx += 1;
                    i += 1;
                }
            }
            let (provider_id, provider_session_id) = db
                .get_conversation_provider_session(conversation_id)
                .unwrap_or((None, None));
            // For remote provider conversations, don't return raw text content
            // (it contains JSON blobs that break markdown rendering).
            // The structured `messages` array has properly reconstructed tool_call widgets.
            let content = if provider_id.is_some() {
                String::new()
            } else {
                db.get_conversation_as_text(conversation_id).unwrap_or_default()
            };
            // Load tool timings from event log (persisted by both local and remote tool execution)
            let tool_timings = load_tool_timings(conversation_id);
            let response = ConversationContentResponse {
                content,
                messages,
                provider_id,
                provider_session_id,
                tool_timings,
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
    #[cfg(not(feature = "mock"))] _llama_state: llama_chat_worker::worker::worker_bridge::SharedWorkerBridge,
    #[cfg(feature = "mock")] _llama_state: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    // Parse optional search query from URL
    let query = crate::request_parsing::get_query_param(req.uri(), "q")
        .map(|v| v.to_lowercase());

    // Fetch conversations from database
    let mut conversations = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    match db.list_conversations() {
        Ok(records) => {
            for record in records {
                // Deduplicate conversation IDs
                let clean_id = record.id.to_string();

                // Skip duplicates
                if !seen_ids.insert(clean_id.clone()) {
                    continue;
                }

                // Extract timestamp from conversation ID (chat_YYYY-MM-DD-HH-mm-ss-SSS)
                let timestamp_part = clean_id
                    .strip_prefix("chat_")
                    .unwrap_or(&clean_id)
                    .to_string();

                // Use DB title for display_name when available
                let title = db.get_conversation_title(&clean_id).ok().flatten();
                let display_name = title
                    .as_deref()
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| format!("Chat {timestamp_part}"));

                conversations.push(ConversationFile {
                    name: clean_id.clone(),
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
    bridge: llama_chat_worker::worker::worker_bridge::SharedWorkerBridge,
) -> Result<Response<Body>, Infallible> {
    let stripped = match path.strip_prefix("/api/conversations/") {
        Some(s) => s,
        None => return Ok(json_error(StatusCode::BAD_REQUEST, "Invalid path")),
    };
    let conv_id = match stripped.strip_suffix("/events") {
        Some(s) => s,
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
        Some(s) => s,
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

