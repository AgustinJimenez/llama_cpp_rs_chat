use std::sync::{Arc, Mutex};
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;
use hyper::upgrade::Upgraded;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;
use futures_util::{StreamExt, SinkExt};
use llama_cpp_2::model::AddBos;

use super::models::*;
use super::database::{SharedDatabase, conversation::ConversationLogger};
use super::chat_handler::{generate_llama_response, apply_model_chat_template, get_universal_system_prompt};
use super::config::load_config;

// Import the global counter
use std::sync::atomic::AtomicU32;
pub static ACTIVE_WS_CONNECTIONS: AtomicU32 = AtomicU32::new(0);

// WebSocket handler for real-time token streaming
pub async fn handle_websocket(
    upgraded: Upgraded,
    llama_state: Option<SharedLlamaState>,
    db: SharedDatabase,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Convert the upgraded connection to a WebSocket stream
    let ws_stream = WebSocketStream::from_raw_socket(
        upgraded,
        tokio_tungstenite::tungstenite::protocol::Role::Server,
        None,
    ).await;

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    let _conn_count = ACTIVE_WS_CONNECTIONS.fetch_add(1, Ordering::SeqCst) + 1;
    eprintln!("[WS_CHAT] New WebSocket connection established");

    // Wait for the first message from the client (should be the chat request)
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(WsMessage::Text(text)) => {
                eprintln!("[WS_CHAT] Received message: {}", text.chars().take(100).collect::<String>());

                // Parse the chat request
                let chat_request: ChatRequest = match serde_json::from_str(&text) {
                    Ok(req) => req,
                    Err(_e) => {
                        let error_msg = serde_json::json!({
                            "type": "error",
                            "error": "Invalid JSON format"
                        });
                        let _ = ws_sender.send(WsMessage::Text(error_msg.to_string())).await;
                        break;
                    }
                };

                // Determine system prompt based on config
                let config = load_config();
                let system_prompt: Option<String> = match config.system_prompt.as_deref() {
                    // "__AGENTIC__" marker = use universal agentic prompt with command execution
                    Some("__AGENTIC__") => Some(get_universal_system_prompt()),
                    // Custom prompt = use as-is
                    Some(custom) => Some(custom.to_string()),
                    // None = use model's default system prompt from GGUF
                    None => {
                        if let Some(ref state) = llama_state {
                            if let Ok(state_guard) = state.lock() {
                                state_guard.as_ref()
                                    .and_then(|s| s.model_default_system_prompt.clone())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                };

                // Create or load conversation logger based on conversation_id
                let conversation_logger = match &chat_request.conversation_id {
                    Some(conv_id) => {
                        eprintln!("[WS_CHAT] Loading existing conversation: {}", conv_id);
                        // Load existing conversation
                        match ConversationLogger::from_existing(db.clone(), conv_id) {
                            Ok(logger) => {
                                eprintln!("[WS_CHAT] Successfully loaded conversation: {}", conv_id);
                                Arc::new(Mutex::new(logger))
                            },
                            Err(e) => {
                                eprintln!("[WS_CHAT] Failed to load conversation: {}", e);
                                let error_msg = serde_json::json!({
                                    "type": "error",
                                    "error": format!("Failed to load conversation: {}", e)
                                });
                                let _ = ws_sender.send(WsMessage::Text(error_msg.to_string())).await;
                                break;
                            }
                        }
                    }
                    None => {
                        eprintln!("[WS_CHAT] Creating new conversation with system prompt mode");
                        // Create a new conversation with config-based system prompt
                        match ConversationLogger::new(db.clone(), system_prompt.as_deref()) {
                            Ok(logger) => {
                                eprintln!("[WS_CHAT] Successfully created new conversation");
                                Arc::new(Mutex::new(logger))
                            },
                            Err(e) => {
                                eprintln!("[WS_CHAT] Failed to create conversation: {}", e);
                                let error_msg = serde_json::json!({
                                    "type": "error",
                                    "error": format!("Failed to create conversation logger: {}", e)
                                });
                                let _ = ws_sender.send(WsMessage::Text(error_msg.to_string())).await;
                                break;
                            }
                        }
                    }
                };

                // Get conversation ID to send back to client
                let conversation_id = {
                    let logger = conversation_logger.lock()
                        .expect("Conversation logger mutex poisoned");
                    logger.get_conversation_id()
                };
                eprintln!("[WS_CHAT] Conversation ID: {}", conversation_id);
                eprintln!("[WS_CHAT] User message: {}", chat_request.message);

                // Create channel for streaming tokens
                let (tx, mut rx) = mpsc::unbounded_channel::<TokenData>();

                // Spawn generation task
                let message = chat_request.message.clone();

                let state_clone = llama_state.clone();
                eprintln!("[WS_CHAT] Spawning generation task");
                tokio::spawn(async move {
                    eprintln!("[WS_CHAT] Generation task started");
                    if let Some(state) = state_clone {
                        match generate_llama_response(&message, state, conversation_logger, Some(tx), false).await {
                            Ok((_content, _tokens, _max)) => {
                            }
                            Err(_e) => {
                            }
                        }
                    }
                });

                // Stream tokens through WebSocket
                loop {
                    tokio::select! {
                        // Receive tokens from the generation task
                        token_result = rx.recv() => {
                            match token_result {
                                Some(token_data) => {
                                    // Send token as JSON
                                    let json = serde_json::json!({
                                        "type": "token",
                                        "token": token_data.token,
                                        "tokens_used": token_data.tokens_used,
                                        "max_tokens": token_data.max_tokens
                                    });

                                    if let Err(_e) = ws_sender.send(WsMessage::Text(json.to_string())).await {
                                        break;
                                    }
                                }
                                None => {
                                    // Channel closed, generation complete
                                    eprintln!("[WS_CHAT] Generation complete, sending done message");
                                    let done_msg = serde_json::json!({
                                        "type": "done",
                                        "conversation_id": conversation_id
                                    });
                                    let _ = ws_sender.send(WsMessage::Text(done_msg.to_string())).await;
                                    eprintln!("[WS_CHAT] Done message sent");
                                    break;
                                }
                            }
                        }
                        // Handle client disconnection or close messages
                        ws_msg = ws_receiver.next() => {
                            match ws_msg {
                                Some(Ok(WsMessage::Close(_))) | None => {
                                    break;
                                }
                                Some(Ok(WsMessage::Ping(data))) => {
                                    let _ = ws_sender.send(WsMessage::Pong(data)).await;
                                }
                                Some(Err(_e)) => {
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                eprintln!("[WS_CHAT] Message processing complete, waiting for next message");
            }
            Ok(WsMessage::Close(_)) => {
                eprintln!("[WS_CHAT] Received Close message");
                break;
            }
            Ok(WsMessage::Ping(data)) => {
                let _ = ws_sender.send(WsMessage::Pong(data)).await;
            }
            Ok(_) => {
                // Ignore other message types
            }
            Err(_e) => {
                break;
            }
        }
    }

    let _conn_count = ACTIVE_WS_CONNECTIONS.fetch_sub(1, Ordering::SeqCst) - 1;
    eprintln!("[WS_CHAT] WebSocket connection closed");
    Ok(())
}

// WebSocket handler for watching conversation updates via broadcast channel
pub async fn handle_conversation_watch(
    upgraded: Upgraded,
    conversation_id: String,
    llama_state: Option<SharedLlamaState>,
    db: SharedDatabase,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws_stream = WebSocketStream::from_raw_socket(
        upgraded,
        tokio_tungstenite::tungstenite::protocol::Role::Server,
        None,
    ).await;

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    let _ = ACTIVE_WS_CONNECTIONS.fetch_add(1, Ordering::SeqCst);

    // Remove .txt extension if present for database lookup
    let conv_id = conversation_id.trim_end_matches(".txt").to_string();

    eprintln!("[WS_WATCH] Watching conversation: {}", conv_id);

    // Subscribe to streaming updates FIRST (before reading initial content)
    // This prevents race conditions where generation completes before we subscribe
    let mut streaming_rx = db.subscribe_streaming();
    eprintln!("[WS_WATCH] Subscribed to streaming updates via broadcast channel");

    // Read initial content from database (now safe - we won't miss broadcasts)
    let initial_content = db.get_conversation_as_text(&conv_id).unwrap_or_default();
    eprintln!("[WS_WATCH] Initial content length: {}", initial_content.len());

    // Calculate token counts for initial content
    let (tokens_used, max_tokens) = if let Some(ref state) = llama_state {
        if let Ok(state_lock) = state.lock() {
            if let Some(ref llama_state_inner) = *state_lock {
                if let Some(ref model) = llama_state_inner.model {
                    if let Some(context_size) = llama_state_inner.model_context_length {
                        // Apply chat template to get the prompt
                        let template_type = llama_state_inner.chat_template_type.as_deref();
                        match apply_model_chat_template(&initial_content, template_type) {
                            Ok(prompt) => {
                                match model.str_to_token(&prompt, AddBos::Always) {
                                    Ok(tokens) => (Some(tokens.len() as i32), Some(context_size as i32)),
                                    Err(_) => (None, Some(context_size as i32))
                                }
                            },
                            Err(_) => (None, Some(context_size as i32))
                        }
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            }
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    // Send initial content with token info
    let initial_msg = serde_json::json!({
        "type": "update",
        "content": initial_content,
        "tokens_used": tokens_used,
        "max_tokens": max_tokens
    });

    let _ = ws_sender.send(WsMessage::Text(initial_msg.to_string())).await;

    loop {
        tokio::select! {
            // Receive streaming updates from broadcast channel
            update_result = streaming_rx.recv() => {
                match update_result {
                    Ok(update) => {
                        // Only process updates for this conversation
                        if update.conversation_id == conv_id {
                            eprintln!("[WS_WATCH] Received update for conversation: {} (complete: {})",
                                conv_id, update.is_complete);

                            // Get full conversation content including new tokens
                            let current_content = db.get_conversation_as_text(&conv_id).unwrap_or_default();

                            // Calculate token counts
                            let (tokens_used, max_tokens) = if let Some(ref state) = llama_state {
                                if let Ok(state_lock) = state.lock() {
                                    if let Some(ref llama_state_inner) = *state_lock {
                                        if let Some(ref model) = llama_state_inner.model {
                                            if let Some(context_size) = llama_state_inner.model_context_length {
                                                let template_type = llama_state_inner.chat_template_type.as_deref();
                                                match apply_model_chat_template(&current_content, template_type) {
                                                    Ok(prompt) => {
                                                        match model.str_to_token(&prompt, AddBos::Always) {
                                                            Ok(tokens) => (Some(tokens.len() as i32), Some(context_size as i32)),
                                                            Err(_) => (None, Some(context_size as i32))
                                                        }
                                                    },
                                                    Err(_) => (None, Some(context_size as i32))
                                                }
                                            } else {
                                                (None, None)
                                            }
                                        } else {
                                            (None, None)
                                        }
                                    } else {
                                        (None, None)
                                    }
                                } else {
                                    (None, None)
                                }
                            } else {
                                (None, None)
                            };

                            let update_msg = serde_json::json!({
                                "type": "update",
                                "content": current_content,
                                "tokens_used": tokens_used,
                                "max_tokens": max_tokens
                            });

                            eprintln!("[WS_WATCH] Sending update via WebSocket (content length: {}, tokens: {:?}/{})",
                                current_content.len(), tokens_used, max_tokens.unwrap_or(0));

                            let send_result = tokio::time::timeout(
                                tokio::time::Duration::from_millis(50),
                                ws_sender.send(WsMessage::Text(update_msg.to_string()))
                            ).await;

                            match send_result {
                                Ok(Ok(_)) => {
                                    eprintln!("[WS_WATCH] Update sent successfully");
                                }
                                Ok(Err(_)) => {
                                    eprintln!("[WS_WATCH] Failed to send WebSocket message - connection closed");
                                    break;
                                }
                                Err(_) => {
                                    eprintln!("[WS_WATCH] WebSocket send timed out - skipping this update");
                                }
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("[WS_WATCH] Broadcast receiver lagged by {} messages", n);
                        // Continue receiving - just missed some updates
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        eprintln!("[WS_WATCH] Broadcast channel closed");
                        break;
                    }
                }
            }
            ws_msg = ws_receiver.next() => {
                match ws_msg {
                    Some(Ok(WsMessage::Close(_))) | None => {
                        break;
                    }
                    Some(Ok(WsMessage::Ping(data))) => {
                        let _ = ws_sender.send(WsMessage::Pong(data)).await;
                    }
                    Some(Err(_)) => {
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    let _ = ACTIVE_WS_CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
    Ok(())
}
