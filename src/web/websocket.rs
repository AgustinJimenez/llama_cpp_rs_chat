use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use hyper::upgrade::Upgraded;
use std::sync::atomic::Ordering;
use tokio::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;

use super::database::SharedDatabase;
use super::models::ChatRequest;
use super::worker::worker_bridge::{GenerationResult, SharedWorkerBridge};

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

// WebSocket handler for real-time token streaming
pub async fn handle_websocket(
    upgraded: Upgraded,
    bridge: SharedWorkerBridge,
    _db: SharedDatabase,
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

                sys_debug!("[WS_CHAT] User message: {}", chat_request.message);

                // Start generation via worker bridge
                let (mut rx, done_rx) = match bridge
                    .generate(
                        chat_request.message.clone(),
                        chat_request.conversation_id.clone(),
                        false, // Worker logs user message
                        chat_request.image_data.clone(),
                    )
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        sys_error!("[WS_CHAT] Failed to start generation: {}", e);
                        let error_msg = serde_json::json!({
                            "type": "error",
                            "error": format!("Failed to start generation: {}", e)
                        });
                        let _ = ws_sender.send(WsMessage::Text(error_msg.to_string())).await;
                        break;
                    }
                };

                sys_debug!("[WS_CHAT] Generation started via worker bridge");

                // Stream tokens through WebSocket with buffering
                let mut pending_tokens = String::new();
                let mut pending_tokens_used: Option<i32> = None;
                let mut pending_max_tokens: Option<i32> = None;
                let mut next_flush = Instant::now() + WS_TOKEN_FLUSH_INTERVAL;
                let mut debug = WsStreamDebug {
                    enabled: std::env::var("LLAMA_CHAT_WS_STREAM_DEBUG").ok().as_deref()
                        == Some("1"),
                    last_log: Instant::now(),
                    chunks_sent: 0,
                    chars_sent: 0,
                };

                loop {
                    tokio::select! {
                        // Receive tokens from the worker via bridge
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

                                    // Get conversation_id and timings from completion result
                                    match done_rx.await {
                                        Ok(GenerationResult::Complete { conversation_id, prompt_tok_per_sec, gen_tok_per_sec, gen_eval_ms, gen_tokens, prompt_eval_ms, prompt_tokens, .. }) => {
                                            let done_msg = serde_json::json!({
                                                "type": "done",
                                                "conversation_id": conversation_id,
                                                "prompt_tok_per_sec": prompt_tok_per_sec,
                                                "gen_tok_per_sec": gen_tok_per_sec,
                                                "gen_eval_ms": gen_eval_ms,
                                                "gen_tokens": gen_tokens,
                                                "prompt_eval_ms": prompt_eval_ms,
                                                "prompt_tokens": prompt_tokens
                                            });
                                            let _ = ws_sender.send(WsMessage::Text(done_msg.to_string())).await;
                                            sys_debug!("[WS_CHAT] Done message sent");
                                        }
                                        Ok(GenerationResult::Cancelled) => {
                                            sys_info!("[WS_CHAT] Generation was cancelled");
                                            let abort_msg = serde_json::json!({ "type": "abort" });
                                            let _ = ws_sender.send(WsMessage::Text(abort_msg.to_string())).await;
                                        }
                                        Ok(GenerationResult::Error(ref e)) => {
                                            sys_error!("[WS_CHAT] Generation error from worker: {}", e);
                                            let error_msg = serde_json::json!({
                                                "type": "error",
                                                "error": e
                                            });
                                            let _ = ws_sender.send(WsMessage::Text(error_msg.to_string())).await;
                                        }
                                        Err(e) => {
                                            sys_error!("[WS_CHAT] Generation result channel closed: {}", e);
                                            let error_msg = serde_json::json!({
                                                "type": "error",
                                                "error": "Generation failed: result channel closed"
                                            });
                                            let _ = ws_sender.send(WsMessage::Text(error_msg.to_string())).await;
                                        }
                                    }
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

/// Persistent status WebSocket — keeps alive with pings, sends initial model
/// status, and lets the frontend detect server crashes via TCP close.
pub async fn handle_status_ws(
    upgraded: Upgraded,
    bridge: SharedWorkerBridge,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws_stream = WebSocketStream::from_raw_socket(
        upgraded,
        tokio_tungstenite::tungstenite::protocol::Role::Server,
        None,
    )
    .await;

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    let _ = ACTIVE_WS_CONNECTIONS.fetch_add(1, Ordering::SeqCst);
    sys_info!("[WS_STATUS] Status WebSocket connected");

    // Send initial model status
    let loaded = bridge.model_status().await.is_some();
    let init_msg = serde_json::json!({
        "type": "model_status",
        "loaded": loaded
    });
    if ws_sender
        .send(WsMessage::Text(init_msg.to_string()))
        .await
        .is_err()
    {
        let _ = ACTIVE_WS_CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
        return Ok(());
    }

    // Keep-alive loop: ping every 20s, listen for client messages
    let mut ping_interval = tokio::time::interval(Duration::from_secs(20));
    ping_interval.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            _ = ping_interval.tick() => {
                if ws_sender.send(WsMessage::Ping(vec![])).await.is_err() {
                    break;
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
                    Some(Ok(WsMessage::Pong(_))) => {
                        // Expected response to our pings — ignore
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
    sys_info!("[WS_STATUS] Status WebSocket disconnected");
    Ok(())
}
