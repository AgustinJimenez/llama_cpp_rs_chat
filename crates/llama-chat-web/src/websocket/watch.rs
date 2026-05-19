//! Conversation watch WebSocket handler — streams incremental updates to watchers.

use futures_util::{SinkExt, StreamExt};
use hyper::upgrade::Upgraded;
use tokio::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;

use llama_chat_db::SharedDatabase;
use llama_chat_worker::worker::worker_bridge::SharedWorkerBridge;

use super::ACTIVE_WS_CONNECTIONS;
use std::sync::atomic::Ordering;

/// WebSocket handler for watching conversation updates via broadcast channel.
pub async fn handle_conversation_watch(
    upgraded: Upgraded,
    conversation_id: String,
    bridge: SharedWorkerBridge,
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

    let conv_id = conversation_id;

    sys_info!("[WS_WATCH] Watching conversation: {}", conv_id);

    // Subscribe to streaming updates FIRST (before reading initial content)
    // This prevents race conditions where generation completes before we subscribe
    let mut streaming_rx = db.subscribe_streaming();
    sys_debug!("[WS_WATCH] Subscribed to streaming updates via broadcast channel");

    // Read initial content from database (now safe - we won't miss broadcasts)
    // For remote provider conversations, skip raw text content — the frontend
    // already loaded structured messages via the HTTP API with proper tool_call widgets.
    let is_remote_provider = db
        .get_conversation_provider_session(&conv_id)
        .map(|(pid, _)| pid.is_some())
        .unwrap_or(false);
    let initial_content = if is_remote_provider {
        String::new()
    } else {
        db.get_conversation_as_text(&conv_id).unwrap_or_default()
    };
    sys_debug!(
        "[WS_WATCH] Initial content length: {}",
        initial_content.len()
    );

    // Get max_tokens from bridge metadata (can't count tokens without model in-process)
    let max_tokens = bridge
        .model_status()
        .await
        .and_then(|m| m.context_length)
        .map(|c| c as i32);

    // Send initial content with token info
    let initial_msg = serde_json::json!({
        "type": "update",
        "content": initial_content,
        "tokens_used": null,
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
