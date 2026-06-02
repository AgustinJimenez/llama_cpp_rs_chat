// ─── Chat Commands ────────────────────────────────────────────────────

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use crate::event_payloads::{ChatDoneEvent, ChatTokenEvent};
use crate::web::config::load_config;
use crate::web::database::SharedDatabase;
use crate::web::models::ChatRequest;
use crate::web::worker::worker_bridge::{GenerationResult, SharedWorkerBridge};

#[tauri::command]
pub async fn generate_stream(
    app: AppHandle,
    request: ChatRequest,
    bridge: tauri::State<'_, SharedWorkerBridge>,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    use std::sync::Mutex;
    use crate::web::chat::{get_tool_tags_for_model, get_universal_system_prompt_with_tags};
    use crate::web::database::conversation::ConversationLogger;

    if request
        .worker_id
        .as_deref()
        .map(str::trim)
        .is_some_and(|id| !id.is_empty() && id != "default")
    {
        return Err(
            "Per-conversation workers are not implemented in Tauri mode yet; only the default worker is available".to_string(),
        );
    }

    // Resolve system prompt
    let general_name = bridge
        .model_status()
        .await
        .and_then(|m| m.general_name.clone());
    let config = load_config(&db);
    let system_prompt = match config.system_prompt.as_deref() {
        Some("__AGENTIC__") => {
            let tags = get_tool_tags_for_model(general_name.as_deref());
            Some(get_universal_system_prompt_with_tags(&tags))
        }
        Some(custom) => Some(custom.to_string()),
        None => None,
    };

    // Create or load conversation logger
    let conversation_logger = if let Some(ref conv_id) = request.conversation_id {
        ConversationLogger::from_existing(db.inner().clone(), conv_id)
            .map_err(|e| format!("Failed to load conversation: {e}"))?
    } else {
        ConversationLogger::new(db.inner().clone(), system_prompt.as_deref())
            .map_err(|e| format!("Failed to create conversation: {e}"))?
    };
    let shared_logger = Arc::new(Mutex::new(conversation_logger));

    // Log user message unless this is an auto-continue/regenerate request.
    if !request.auto_continue {
        let mut logger = shared_logger.lock().unwrap();
        logger.log_message("USER", &request.message);
    }

    let conversation_id = {
        let logger = shared_logger.lock().unwrap();
        logger.get_conversation_id()
    };

    // Notify frontend that a new conversation was created (so sidebar updates immediately)
    let _ = app.emit("conversation-title-updated", serde_json::json!({
        "conversation_id": &conversation_id,
    }));

    // Prevent OS sleep/screen-off during inference. Dropped automatically when the
    // spawned task below completes (i.e. when generation finishes or is cancelled).
    let _wake_guard = keepawake::Builder::default()
        .display(false)
        .idle(true)
        .sleep(true)
        .create()
        .map_err(|e| log::warn!("keepawake: {e}"))
        .ok();

    // Start generation (skip_user_logging since we logged above)
    let (mut token_rx, done_rx) = bridge
        .generate(
            request.message.clone(),
            Some(conversation_id.clone()),
            true,
            request.image_data.clone(),
        )
        .await?;

    // Spawn task to forward tokens as Tauri events
    let conv_id = conversation_id.clone();
    tokio::spawn(async move {
        // Keep OS awake for the duration of inference; drops when this task ends.
        let _wake = _wake_guard;
        while let Some(token_data) = token_rx.recv().await {
            let _ = app.emit(
                "chat-token",
                ChatTokenEvent {
                    token: token_data.token,
                    tokens_used: token_data.tokens_used,
                    max_tokens: token_data.max_tokens,
                },
            );
        }

        match done_rx.await {
            Ok(GenerationResult::Complete {
                conversation_id,
                tokens_used,
                max_tokens,
                prompt_tok_per_sec,
                gen_tok_per_sec,
                gen_eval_ms,
                gen_tokens,
                prompt_eval_ms,
                prompt_tokens,
                finish_reason,
                ..
            }) => {
                let _ = app.emit(
                    "chat-done",
                    ChatDoneEvent {
                        event_type: "done".into(),
                        conversation_id: Some(conversation_id.clone()),
                        tokens_used: Some(tokens_used),
                        max_tokens: Some(max_tokens),
                        error: None,
                        prompt_tok_per_sec,
                        gen_tok_per_sec,
                        gen_eval_ms,
                        gen_tokens,
                        prompt_eval_ms,
                        prompt_tokens,
                        finish_reason,
                    },
                );

                // Auto-generate/update title in background
                let bridge_bg: SharedWorkerBridge = app.state::<SharedWorkerBridge>().inner().clone();
                let db_bg: SharedDatabase = app.state::<SharedDatabase>().inner().clone();
                let conv_id_for_title = conversation_id.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let conv_clean = conv_id_for_title.as_str();
                    let messages = match db_bg.get_messages(conv_clean) {
                        Ok(m) => m,
                        Err(_) => return,
                    };
                    let first_user = messages.iter().find(|m| m.role == "user");
                    let first_asst = messages.iter().find(|m| m.role == "assistant");
                    if first_user.is_none() || first_asst.is_none() { return; }

                    // Check if title already exists
                    let existing_title = db_bg.list_conversations().ok()
                        .and_then(|convs| convs.into_iter().find(|c| c.id == conv_clean))
                        .map(|c| c.title)
                        .unwrap_or_default();

                    let fu: String = first_user.unwrap().content.chars().take(200).collect();
                    let fa: String = first_asst.unwrap().content.chars().take(200).collect();

                    if !existing_title.is_empty() {
                        // Title exists — only update if topic changed
                        // (last user message is substantially different from first)
                        let last_user = messages.iter().rev().find(|m| m.role == "user");
                        if let Some(lu) = last_user {
                            let fu_content = &first_user.unwrap().content;
                            // Same user message or auto-continue ("Continue working on")
                            if lu.content == *fu_content
                                || lu.content.starts_with("Continue working on")
                                || lu.content == "Continue"
                            {
                                eprintln!("[TAURI_TITLE] Same topic, keeping: '{existing_title}'");
                                return;
                            }
                        }
                        eprintln!("[TAURI_TITLE] Topic may have changed, regenerating");
                    }

                    // Generate new title
                    let prompt = format!("User: {fu}\nAssistant: {fa}");
                    match bridge_bg.generate_title(conv_clean, &prompt).await {
                        Ok(raw) => {
                            let title = crate::web::websocket::sanitize_title(&raw);
                            eprintln!("[TAURI_TITLE] Generated: '{title}'");
                            if !title.is_empty() {
                                let _ = db_bg.update_conversation_title(conv_clean, &title);
                                let _ = app.emit("conversation-title-updated", serde_json::json!({
                                    "conversation_id": conv_clean,
                                    "title": title,
                                }));
                            }
                        }
                        Err(e) => eprintln!("[TAURI_TITLE] Failed: {e}"),
                    }
                });
            }
            Ok(GenerationResult::Cancelled) => {
                let _ = app.emit(
                    "chat-done",
                    ChatDoneEvent {
                        event_type: "cancelled".into(),
                        conversation_id: Some(conv_id.clone()),
                        tokens_used: None,
                        max_tokens: None,
                        error: None,
                        prompt_tok_per_sec: None,
                        gen_tok_per_sec: None,
                        gen_eval_ms: None,
                        gen_tokens: None,
                        prompt_eval_ms: None,
                        prompt_tokens: None,
                        finish_reason: Some("cancelled".into()),
                    },
                );
            }
            Ok(GenerationResult::Error(e)) => {
                let _ = app.emit(
                    "chat-done",
                    ChatDoneEvent {
                        event_type: "error".into(),
                        conversation_id: Some(conv_id.clone()),
                        tokens_used: None,
                        max_tokens: None,
                        error: Some(e),
                        prompt_tok_per_sec: None,
                        gen_tok_per_sec: None,
                        gen_eval_ms: None,
                        gen_tokens: None,
                        prompt_eval_ms: None,
                        prompt_tokens: None,
                        finish_reason: Some("error".into()),
                    },
                );
            }
            Err(_) => {
                let _ = app.emit(
                    "chat-done",
                    ChatDoneEvent {
                        event_type: "error".into(),
                        conversation_id: Some(conv_id.clone()),
                        tokens_used: None,
                        max_tokens: None,
                        error: Some("Worker response channel closed".into()),
                        prompt_tok_per_sec: None,
                        gen_tok_per_sec: None,
                        gen_eval_ms: None,
                        gen_tokens: None,
                        prompt_eval_ms: None,
                        prompt_tokens: None,
                        finish_reason: Some("error".into()),
                    },
                );
            }
        }
    });

    Ok(serde_json::json!({ "conversation_id": conversation_id }))
}

#[tauri::command]
pub async fn cancel_generation(
    bridge: tauri::State<'_, SharedWorkerBridge>,
) -> Result<serde_json::Value, String> {
    bridge.cancel_generation().await;
    Ok(serde_json::json!({"success": true, "message": "Cancellation requested"}))
}
