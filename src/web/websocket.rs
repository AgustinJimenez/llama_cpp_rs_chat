use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use hyper::upgrade::Upgraded;
use llama_cpp_2::model::AddBos;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;

use super::chat_handler::apply_model_chat_template;
use super::config::get_resolved_system_prompt;
use super::database::{conversation::ConversationLogger, SharedDatabase};
use super::generation_queue::{GenerationRequest, SharedGenerationQueue};
use super::models::*;

// Import logging macros
use crate::{sys_debug, sys_error, sys_info, sys_warn};

// Import the global counter
use std::sync::atomic::AtomicU32;
pub static ACTIVE_WS_CONNECTIONS: AtomicU32 = AtomicU32::new(0);

const WS_TOKEN_FLUSH_INTERVAL: Duration = Duration::from_millis(40);
const WS_TOKEN_FLUSH_MAX_CHARS: usize = 1024;

struct WsStreamDebug {
    enabled: bool,
    last_log: Instant,
    chunks_sent: u64,
    chars_sent: usize,
}

async fn flush_pending_tokens(
    ws_sender: &mut SplitSink<WebSocketStream<Upgraded>, WsMessage>,
    pending_tokens: &mut String,
    pending_tokens_used: &mut Option<i32>,
    pending_max_tokens: &mut Option<i32>,
    next_flush: &mut Instant,
    debug: &mut WsStreamDebug,
) -> Result<(), ()> {
    if pending_tokens.is_empty() {
        *next_flush = Instant::now() + WS_TOKEN_FLUSH_INTERVAL;
        return Ok(());
    }

    let token_chunk = std::mem::take(pending_tokens);
    let tokens_used = pending_tokens_used.take();
    let max_tokens = pending_max_tokens.take();

    debug.chunks_sent = debug.chunks_sent.saturating_add(1);
    debug.chars_sent = debug.chars_sent.saturating_add(token_chunk.len());
    if debug.enabled && debug.last_log.elapsed() >= Duration::from_secs(1) {
        sys_debug!(
            "[WS_CHAT] Stream stats: {} chunks, {} chars sent (last 1s+)",
            debug.chunks_sent,
            debug.chars_sent
        );
        debug.last_log = Instant::now();
        debug.chunks_sent = 0;
        debug.chars_sent = 0;
    }

    let json = serde_json::json!({
        "type": "token",
        "token": token_chunk,
        "tokens_used": tokens_used,
        "max_tokens": max_tokens
    });

    *next_flush = Instant::now() + WS_TOKEN_FLUSH_INTERVAL;

    if ws_sender
        .send(WsMessage::Text(json.to_string()))
        .await
        .is_err()
    {
        return Err(());
    }
    let _ = ws_sender.flush().await;
    Ok(())
}

/// Calculate token counts for conversation content
/// Returns (tokens_used, max_tokens) tuple where both values are Option<i32>
fn calculate_tokens_for_content(
    content: &str,
    llama_state: &Option<SharedLlamaState>,
) -> (Option<i32>, Option<i32>) {
    if let Some(ref state) = llama_state {
        if let Ok(state_lock) = state.lock() {
            if let Some(ref llama_state_inner) = *state_lock {
                if let Some(ref model) = llama_state_inner.model {
                    if let Some(context_size) = llama_state_inner.model_context_length {
                        // Apply chat template to get the prompt
                        let template_type = llama_state_inner.chat_template_type.as_deref();
                        match apply_model_chat_template(content, template_type) {
                            Ok(prompt) => match model.str_to_token(&prompt, AddBos::Always) {
                                Ok(tokens) => {
                                    return (Some(tokens.len() as i32), Some(context_size as i32))
                                }
                                Err(_) => return (None, Some(context_size as i32)),
                            },
                            Err(_) => return (None, Some(context_size as i32)),
                        }
                    }
                }
            }
        }
    }
    (None, None)
}

// WebSocket handler for real-time token streaming
pub async fn handle_websocket(
    upgraded: Upgraded,
    llama_state: Option<SharedLlamaState>,
    generation_queue: SharedGenerationQueue,
    db: SharedDatabase,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Convert the upgraded connection to a WebSocket stream
    let ws_stream = WebSocketStream::from_raw_socket(
        upgraded,
        tokio_tungstenite::tungstenite::protocol::Role::Server,
        None,
    )
    .await;

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    let _conn_count = ACTIVE_WS_CONNECTIONS.fetch_add(1, Ordering::SeqCst) + 1;
    sys_info!("[WS_CHAT] New WebSocket connection established");

    // Wait for the first message from the client (should be the chat request)
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(WsMessage::Text(text)) => {
                sys_debug!(
                    "[WS_CHAT] Received message: {}",
                    text.chars().take(100).collect::<String>()
                );

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
                let system_prompt = get_resolved_system_prompt(&db, &llama_state);

                // Create or load conversation logger based on conversation_id
                let conversation_logger = match &chat_request.conversation_id {
                    Some(conv_id) => {
                        sys_info!("[WS_CHAT] Loading existing conversation: {}", conv_id);
                        // Load existing conversation
                        match ConversationLogger::from_existing(db.clone(), conv_id) {
                            Ok(logger) => {
                                sys_info!(
                                    "[WS_CHAT] Successfully loaded conversation: {}",
                                    conv_id
                                );
                                Arc::new(Mutex::new(logger))
                            }
                            Err(e) => {
                                sys_error!("[WS_CHAT] Failed to load conversation: {}", e);
                                let error_msg = serde_json::json!({
                                    "type": "error",
                                    "error": format!("Failed to load conversation: {}", e)
                                });
                                let _ =
                                    ws_sender.send(WsMessage::Text(error_msg.to_string())).await;
                                break;
                            }
                        }
                    }
                    None => {
                        sys_info!("[WS_CHAT] Creating new conversation with system prompt mode");
                        // Create a new conversation with config-based system prompt
                        match ConversationLogger::new(db.clone(), system_prompt.as_deref()) {
                            Ok(logger) => {
                                sys_info!("[WS_CHAT] Successfully created new conversation");
                                Arc::new(Mutex::new(logger))
                            }
                            Err(e) => {
                                sys_error!("[WS_CHAT] Failed to create conversation: {}", e);
                                let error_msg = serde_json::json!({
                                    "type": "error",
                                    "error": format!("Failed to create conversation logger: {}", e)
                                });
                                let _ =
                                    ws_sender.send(WsMessage::Text(error_msg.to_string())).await;
                                break;
                            }
                        }
                    }
                };

                // Get conversation ID to send back to client
                let conversation_id = {
                    let logger = conversation_logger
                        .lock()
                        .expect("Conversation logger mutex poisoned");
                    logger.get_conversation_id()
                };
                sys_info!("[WS_CHAT] Conversation ID: {}", conversation_id);
                sys_debug!("[WS_CHAT] User message: {}", chat_request.message);

                // Create channel for streaming tokens
                let (tx, mut rx) = mpsc::unbounded_channel::<TokenData>();

                // Submit generation to queue
                let message = chat_request.message.clone();
                sys_debug!("[WS_CHAT] Submitting generation to queue");

                let cancel = Arc::new(AtomicBool::new(false));
                let (result_tx, _result_rx) = tokio::sync::oneshot::channel();

                if let Some(state) = llama_state.clone() {
                    let gen_request = GenerationRequest {
                        user_message: message,
                        llama_state: state,
                        conversation_logger: conversation_logger.clone(),
                        token_sender: Some(tx),
                        skip_user_logging: false,
                        db: db.clone(),
                        cancel: cancel.clone(),
                        result_sender: result_tx,
                    };

                    if let Err(e) = generation_queue.submit(gen_request).await {
                        sys_error!("[WS_CHAT] Failed to submit to queue: {}", e);
                    }
                } else {
                    sys_error!("[WS_CHAT] ERROR: llama_state is None - no model loaded!");
                }

                // Stream tokens through WebSocket
                let mut pending_tokens = String::new();
                let mut pending_tokens_used: Option<i32> = None;
                let mut pending_max_tokens: Option<i32> = None;
                let mut next_flush = Instant::now() + WS_TOKEN_FLUSH_INTERVAL;
                let mut debug = WsStreamDebug {
                    enabled: std::env::var("LLAMA_CHAT_WS_STREAM_DEBUG").ok().as_deref() == Some("1"),
                    last_log: Instant::now(),
                    chunks_sent: 0,
                    chars_sent: 0,
                };

                loop {
                    tokio::select! {
                        // Receive tokens from the generation task
                        token_result = rx.recv() => {
                            match token_result {
                                Some(token_data) => {
                                    pending_tokens.push_str(&token_data.token);
                                    pending_tokens_used = Some(token_data.tokens_used);
                                    pending_max_tokens = Some(token_data.max_tokens);

                                    if pending_tokens.len() >= WS_TOKEN_FLUSH_MAX_CHARS
                                        && flush_pending_tokens(
                                            &mut ws_sender,
                                            &mut pending_tokens,
                                            &mut pending_tokens_used,
                                            &mut pending_max_tokens,
                                            &mut next_flush,
                                            &mut debug,
                                        ).await.is_err() {
                                            break;
                                        }
                                }
                                None => {
                                    // Channel closed, generation complete
                                    sys_info!("[WS_CHAT] Generation complete, sending done message");
                                    let _ = flush_pending_tokens(
                                        &mut ws_sender,
                                        &mut pending_tokens,
                                        &mut pending_tokens_used,
                                        &mut pending_max_tokens,
                                        &mut next_flush,
                                        &mut debug,
                                    ).await;
                                    let done_msg = serde_json::json!({
                                        "type": "done",
                                        "conversation_id": conversation_id
                                    });
                                    let _ = ws_sender.send(WsMessage::Text(done_msg.to_string())).await;
                                    sys_debug!("[WS_CHAT] Done message sent");
                                    break;
                                }
                            }
                        }
                        _ = tokio::time::sleep_until(next_flush), if !pending_tokens.is_empty() => {
                            if flush_pending_tokens(
                                &mut ws_sender,
                                &mut pending_tokens,
                                &mut pending_tokens_used,
                                &mut pending_max_tokens,
                                &mut next_flush,
                                &mut debug,
                            ).await.is_err() {
                                break;
                            }
                        }
                        // Handle client disconnection or close messages
                        ws_msg = ws_receiver.next() => {
                            match ws_msg {
                                Some(Ok(WsMessage::Close(_))) | None => {
                                    let _ = flush_pending_tokens(
                                        &mut ws_sender,
                                        &mut pending_tokens,
                                        &mut pending_tokens_used,
                                        &mut pending_max_tokens,
                                        &mut next_flush,
                                        &mut debug,
                                    ).await;
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
                sys_debug!("[WS_CHAT] Message processing complete, waiting for next message");
            }
            Ok(WsMessage::Close(_)) => {
                sys_info!("[WS_CHAT] Received Close message");
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
    sys_info!("[WS_CHAT] WebSocket connection closed");
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
    )
    .await;

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    let _ = ACTIVE_WS_CONNECTIONS.fetch_add(1, Ordering::SeqCst);

    // Remove .txt extension if present for database lookup
    let conv_id = conversation_id.trim_end_matches(".txt").to_string();

    sys_info!("[WS_WATCH] Watching conversation: {}", conv_id);

    // Subscribe to streaming updates FIRST (before reading initial content)
    // This prevents race conditions where generation completes before we subscribe
    let mut streaming_rx = db.subscribe_streaming();
    sys_debug!("[WS_WATCH] Subscribed to streaming updates via broadcast channel");

    // Read initial content from database (now safe - we won't miss broadcasts)
    let initial_content = db.get_conversation_as_text(&conv_id).unwrap_or_default();
    sys_debug!(
        "[WS_WATCH] Initial content length: {}",
        initial_content.len()
    );

    // Calculate token counts for initial content
    let (tokens_used, max_tokens) = calculate_tokens_for_content(&initial_content, &llama_state);

    // Send initial content with token info
    let initial_msg = serde_json::json!({
        "type": "update",
        "content": initial_content,
        "tokens_used": tokens_used,
        "max_tokens": max_tokens
    });

    let _ = ws_sender
        .send(WsMessage::Text(initial_msg.to_string()))
        .await;

    let mut last_sent_len = initial_content.len();
    let mut last_sent_at = Instant::now();

    loop {
        tokio::select! {
            // Receive streaming updates from broadcast channel
            update_result = streaming_rx.recv() => {
                match update_result {
                    Ok(update) => {
                        // Only process updates for this conversation
                        if update.conversation_id == conv_id {
                            sys_debug!("[WS_WATCH] Received update for conversation: {} (complete: {})",
                                conv_id, update.is_complete);

                            // Get full conversation content including new tokens
                            let current_content = db.get_conversation_as_text(&conv_id).unwrap_or_default();
                            let current_len = current_content.len();
                            let now = Instant::now();

                            // Avoid spamming identical or too-frequent updates; always send final update.
                            if !update.is_complete {
                                if current_len == last_sent_len {
                                    continue;
                                }
                                if now.duration_since(last_sent_at) < Duration::from_millis(200) {
                                    continue;
                                }
                            }
                            last_sent_len = current_len;
                            last_sent_at = now;

                            // Use token counts from the broadcast (set by generation loop)
                            let tokens_used = if update.tokens_used > 0 { Some(update.tokens_used) } else { None };
                            let max_tokens = if update.max_tokens > 0 { Some(update.max_tokens) } else { None };

                            let update_msg = serde_json::json!({
                                "type": "update",
                                "content": current_content,
                                "tokens_used": tokens_used,
                                "max_tokens": max_tokens
                            });

                            sys_debug!("[WS_WATCH] Sending update via WebSocket (content length: {}, tokens: {:?}/{})",
                                current_content.len(), tokens_used, max_tokens.unwrap_or(0));

                            let send_result = tokio::time::timeout(
                                tokio::time::Duration::from_millis(50),
                                ws_sender.send(WsMessage::Text(update_msg.to_string()))
                            ).await;

                            match send_result {
                                Ok(Ok(_)) => {
                                    sys_debug!("[WS_WATCH] Update sent successfully");
                                }
                                Ok(Err(_)) => {
                                    sys_warn!("[WS_WATCH] Failed to send WebSocket message - connection closed");
                                    break;
                                }
                                Err(_) => {
                                    sys_warn!("[WS_WATCH] WebSocket send timed out - skipping this update");
                                }
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        sys_warn!("[WS_WATCH] Broadcast receiver lagged by {} messages", n);
                        // Continue receiving - just missed some updates
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        sys_info!("[WS_WATCH] Broadcast channel closed");
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
