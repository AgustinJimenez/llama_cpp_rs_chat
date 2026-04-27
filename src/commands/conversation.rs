//! Conversation Tauri commands — list, load, delete, truncate, rename.

use crate::web::database::SharedDatabase;
use crate::web::models::*;

// ─── Conversation Commands ────────────────────────────────────────────

#[tauri::command]
pub async fn get_conversations(
    db: tauri::State<'_, SharedDatabase>,
) -> Result<ConversationsResponse, String> {
    let records = db.list_conversations().unwrap_or_default();
    let conversations = records
        .into_iter()
        .map(|r| {
            let timestamp_part = r.id.strip_prefix("chat_").unwrap_or(&r.id).to_string();
            ConversationFile {
                name: format!("{}.txt", r.id),
                display_name: format!("Chat {timestamp_part}"),
                timestamp: timestamp_part,
                title: if r.title.is_empty() { None } else { Some(r.title) },
                provider_id: None,
            }
        })
        .collect();
    Ok(ConversationsResponse { conversations })
}

#[tauri::command]
pub async fn get_conversation(
    filename: String,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<ConversationContentResponse, String> {
    let conversation_id = filename.trim_end_matches(".txt");
    let content = db.get_conversation_as_text(conversation_id)?;
    let messages = parse_conversation_to_messages(&content);
    Ok(ConversationContentResponse { content, messages, provider_id: None, provider_session_id: None })
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

