//! Conversation Tauri commands — list, load, delete, truncate, rename.

use crate::web;
use crate::web::database::SharedDatabase;
use crate::web::models::*;

// ─── Conversation Commands ────────────────────────────────────────────

#[tauri::command]
pub async fn get_conversations(
    db: tauri::State<'_, SharedDatabase>,
) -> Result<ConversationsResponse, String> {
    let records = db.list_conversations().unwrap_or_default();
    let mut seen_ids = std::collections::HashSet::new();
    let mut conversations = Vec::new();
    for r in records {
        let clean_id = r.id.trim_end_matches(".txt").to_string();
        if !seen_ids.insert(clean_id.clone()) {
            continue;
        }
        let timestamp_part = clean_id.strip_prefix("chat_").unwrap_or(&clean_id).to_string();
        let title_opt = db.get_conversation_title(&clean_id).ok().flatten();
        let display_name = title_opt.clone()
            .unwrap_or_else(|| format!("Chat {timestamp_part}"));
        conversations.push(ConversationFile {
            name: format!("{clean_id}.txt"),
            display_name,
            timestamp: timestamp_part,
            title: title_opt,
            provider_id: None,
        });
    }
    Ok(ConversationsResponse { conversations })
}

#[tauri::command]
pub async fn get_conversation(
    filename: String,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<ConversationContentResponse, String> {
    // Try both with and without .txt suffix — remote provider conversations
    // may store the ID with .txt, local ones without.
    let trimmed = filename.trim_end_matches(".txt");
    let mut db_messages = db.get_messages(trimmed).unwrap_or_default();
    if db_messages.is_empty() {
        db_messages = db.get_messages(&filename).unwrap_or_default();
    }
    let conversation_id = trimmed;
    // Rebuild messages: merge consecutive assistant tool_call + tool results
    // into a single message with <tool_call>/<tool_response> tags for widget rendering
    let mut messages: Vec<crate::web::models::ChatMessage> = Vec::new();
    let mut idx = 0;
    let mut msg_idx = 0u64;
    while idx < db_messages.len() {
        let m = &db_messages[idx];
        if m.role == "assistant" && m.content.contains("\"tool_calls\":") && m.content.starts_with("{") {
            let mut combined = String::new();
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&m.content) {
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
            let mut j = idx + 1;
            while j < db_messages.len() && db_messages[j].role == "tool" {
                let tool_content = db_messages[j].content.split_once("\n\n")
                    .map(|(_, c)| c).unwrap_or(&db_messages[j].content);
                let display = &tool_content[..tool_content.len().min(2000)];
                combined.push_str(&format!("\n<tool_response>{display}</tool_response>\n"));
                j += 1;
            }
            if !combined.trim().is_empty() {
                messages.push(crate::web::models::ChatMessage {
                    id: format!("msg_{msg_idx}"), role: "assistant".to_string(), content: combined,
                    timestamp: m.timestamp, prompt_tok_per_sec: m.prompt_tok_per_sec,
                    gen_tok_per_sec: m.gen_tok_per_sec, gen_eval_ms: m.gen_eval_ms,
                    gen_tokens: m.gen_tokens, prompt_eval_ms: m.prompt_eval_ms, prompt_tokens: m.prompt_tokens,
                });
                msg_idx += 1;
            }
            idx = j;
        } else if m.role == "tool" {
            idx += 1; // orphan tool message, skip
        } else {
            messages.push(crate::web::models::ChatMessage {
                id: format!("msg_{msg_idx}"), role: m.role.clone(), content: m.content.clone(),
                timestamp: m.timestamp, prompt_tok_per_sec: m.prompt_tok_per_sec,
                gen_tok_per_sec: m.gen_tok_per_sec, gen_eval_ms: m.gen_eval_ms,
                gen_tokens: m.gen_tokens, prompt_eval_ms: m.prompt_eval_ms, prompt_tokens: m.prompt_tokens,
            });
            msg_idx += 1;
            idx += 1;
        }
    }
    let content = db.get_conversation_as_text(conversation_id).unwrap_or_default();

    // Check streaming buffer for partial response from a crashed session
    let partial = db.connection().query_row(
        "SELECT partial_content FROM streaming_buffer WHERE conversation_id = ?1",
        rusqlite::params![conversation_id],
        |row| row.get::<_, String>(0),
    ).ok();
    // If there's a partial response not yet saved as a message, append it
    let mut final_messages = messages;
    if let Some(partial_content) = partial {
        if !partial_content.trim().is_empty() {
            final_messages.push(crate::web::models::ChatMessage {
                id: "msg_partial_recovery".to_string(),
                role: "assistant".to_string(),
                content: format!("{partial_content}\n\n[Response was interrupted — partial recovery from crash]"),
                timestamp: 0,
                prompt_tok_per_sec: None,
                gen_tok_per_sec: None,
                gen_eval_ms: None,
                gen_tokens: None,
                prompt_eval_ms: None,
                prompt_tokens: None,
            });
            // Save recovered content as a real message and clear buffer
            if let Ok(mut logger) = web::database::conversation::ConversationLogger::from_existing(
                db.inner().clone(), conversation_id,
            ) {
                logger.log_message("ASSISTANT", &format!("{partial_content}\n\n[Response interrupted — recovered from crash]"));
            }
            let _ = db.connection().execute(
                "DELETE FROM streaming_buffer WHERE conversation_id = ?1",
                rusqlite::params![conversation_id],
            );
        }
    }

    Ok(ConversationContentResponse { content, messages: final_messages, provider_id: None, provider_session_id: None })
}

#[tauri::command]
pub async fn delete_conversation(
    filename: String,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return Err("Invalid filename".into());
    }
    if !filename.starts_with("chat_") {
        return Err("Invalid conversation file".into());
    }
    let conversation_id = filename.trim_end_matches(".txt");
    db.delete_conversation(conversation_id)?;
    Ok(serde_json::json!({"success": true}))
}

#[tauri::command]
pub async fn truncate_conversation(
    conversation_id: String,
    from_sequence: i32,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    let id = conversation_id.trim_end_matches(".txt");
    let deleted = db.truncate_messages(id, from_sequence)?;
    Ok(serde_json::json!({"success": true, "deleted": deleted}))
}

#[tauri::command]
pub async fn get_conversation_metrics(
    conversation_id: String,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    let conv_id = conversation_id.trim_end_matches(".txt");
    let logs = db.get_logs_for_conversation(conv_id)?;
    let metrics: Vec<_> = logs.into_iter().filter(|l| l.level == "metrics").collect();
    Ok(serde_json::to_value(&metrics).unwrap_or_default())
}
#[allow(dead_code)]
pub fn parse_conversation_to_messages(content: &str) -> Vec<crate::web::models::ChatMessage> {
    let mut messages = Vec::new();
    let mut current_role = String::new();
    let mut current_content = String::new();
    let mut sequence = 0u64;

    for line in content.lines() {
        if line == "SYSTEM:" || line == "USER:" || line == "ASSISTANT:" {
            if !current_role.is_empty() && !current_content.trim().is_empty() {
                messages.push(crate::web::models::ChatMessage {
                    id: format!("msg_{sequence}"),
                    role: current_role.to_lowercase(),
                    content: current_content.trim().to_string(),
                    timestamp: sequence,
                    prompt_tok_per_sec: None,
                    gen_tok_per_sec: None,
                    gen_eval_ms: None,
                    gen_tokens: None,
                    prompt_eval_ms: None,
                    prompt_tokens: None,
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

    if !current_role.is_empty() && !current_content.trim().is_empty() {
        messages.push(crate::web::models::ChatMessage {
            id: format!("msg_{sequence}"),
            role: current_role.to_lowercase(),
            content: current_content.trim().to_string(),
            timestamp: sequence,
            prompt_tok_per_sec: None,
            gen_tok_per_sec: None,
            gen_eval_ms: None,
            gen_tokens: None,
            prompt_eval_ms: None,
            prompt_tokens: None,
        });
    }

    messages
}

// ─── Helper trait for piping ──────────────────────────────────────────

