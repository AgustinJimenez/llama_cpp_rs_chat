//! Chat WebSocket handler — real-time token streaming with server-side auto-continue.

use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use hyper::upgrade::Upgraded;
use tokio::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;

use llama_chat_db::SharedDatabase;
use llama_chat_types::models::ChatRequest;
use llama_chat_worker::worker::worker_bridge::GenerationResult;
use crate::worker_pool::{resolve_bridge_for_request, WorkerPool};

use super::{
    make_server_continuation_message, should_server_auto_continue, ACTIVE_WS_CONNECTIONS,
    MAX_SERVER_AUTO_CONTINUES,
};
use super::title::spawn_title_generation;
use std::sync::atomic::Ordering;

const WS_TOKEN_FLUSH_INTERVAL: Duration = Duration::from_millis(40);
const WS_TOKEN_FLUSH_MAX_CHARS: usize = 1024;

pub(super) struct WsStreamDebug {
    pub enabled: bool,
    pub last_log: Instant,
    pub chunks_sent: u64,
    pub chars_sent: usize,
}

pub(super) async fn flush_pending_tokens(
    ws_sender: &mut SplitSink<WebSocketStream<Upgraded>, WsMessage>,
    pending_tokens: &mut String,
    pending_tokens_used: &mut Option<i32>,
    pending_max_tokens: &mut Option<i32>,
    pending_gen_tok_per_sec: &mut Option<f64>,
    pending_gen_tokens: &mut Option<i32>,
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
    let gen_tok_per_sec = pending_gen_tok_per_sec.take();
    let gen_tokens = pending_gen_tokens.take();

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

    let mut json = serde_json::json!({
        "type": "token",
        "token": token_chunk,
        "tokens_used": tokens_used,
        "max_tokens": max_tokens
    });
    if let Some(tps) = gen_tok_per_sec {
        json["gen_tok_per_sec"] = serde_json::json!(tps);
    }
    if let Some(gt) = gen_tokens {
        json["gen_tokens"] = serde_json::json!(gt);
    }

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

/// WebSocket handler for real-time token streaming.
pub async fn handle_websocket(
    upgraded: hyper::upgrade::Upgraded,
    pool: WorkerPool,
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

                sys_debug!("[WS_CHAT] User message: {}", chat_request.message);

                // ── Reconnect mode ─────────────────────────────────────────
                // Client dropped connection mid-stream and is reconnecting.
                // Don't start a new generation — wait for the in-progress one
                // to finish, then send a synthetic done so the client reloads.
                if chat_request.reconnect {
                    if let Some(conv_id) = chat_request.conversation_id.as_deref() {
                        // Resolve bridge so we can inspect its state.
                        if let Ok(bridge) = resolve_bridge_for_request(
                            &pool, &db,
                            Some(conv_id),
                            chat_request.worker_id.as_deref(),
                            chat_request.agent_id.as_deref(),
                        ).await {
                            // Poll until generation finishes (max 10 min).
                            for _ in 0u16..1200 {
                                if !bridge.is_generating().await {
                                    break;
                                }
                                tokio::time::sleep(Duration::from_millis(500)).await;
                            }
                        }
                        let done_msg = serde_json::json!({
                            "type": "done",
                            "conversation_id": conv_id
                        });
                        let _ = ws_sender.send(WsMessage::Text(done_msg.to_string())).await;
                    }
                    break;
                }

                let bridge = match resolve_bridge_for_request(
                    &pool,
                    &db,
                    chat_request.conversation_id.as_deref(),
                    chat_request.worker_id.as_deref(),
                    chat_request.agent_id.as_deref(),
                ).await {
                    Ok(bridge) => bridge,
                    Err(e) => {
                        let error_msg = serde_json::json!({
                            "type": "error",
                            "error": e
                        });
                        let _ = ws_sender.send(WsMessage::Text(error_msg.to_string())).await;
                        break;
                    }
                };

                // ── Server-side auto-continue setup ────────────────────────
                let original_message = chat_request.message.clone();
                let mut current_message = chat_request.message.clone();
                let mut current_conv_id = chat_request.conversation_id.clone();
                let requested_worker_id = chat_request
                    .worker_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|id| !id.is_empty() && *id != "default")
                    .map(str::to_string);
                let mut server_auto_continue_count = 0u32;
                let mut final_conv_id_for_title = String::new();

                // Buffering state persists across continuation turns (stream is continuous)
                let mut pending_tokens = String::new();
                let mut pending_tokens_used: Option<i32> = None;
                let mut pending_max_tokens: Option<i32> = None;
                let mut pending_gen_tok_per_sec: Option<f64> = None;
                let mut pending_gen_tokens: Option<i32> = None;
                let mut next_flush = Instant::now() + WS_TOKEN_FLUSH_INTERVAL;
                let mut debug = WsStreamDebug {
                    enabled: std::env::var("LLAMA_CHAT_WS_STREAM_DEBUG").ok().as_deref()
                        == Some("1"),
                    last_log: Instant::now(),
                    chunks_sent: 0,
                    chars_sent: 0,
                };
                // Heartbeat: send a keepalive every 15s during long-running silent commands.
                let mut heartbeat_deadline = Instant::now() + Duration::from_secs(15);
                // Worker silence watchdog: if no token arrives for 3 minutes, the worker
                // is stuck (blocked thread, IPC overflow, crashed without dropping sender).
                // Reset on every token; fire → synthetic error so the UI recovers.
                const WORKER_SILENCE_TIMEOUT: Duration = Duration::from_secs(180);
                let mut worker_silence_deadline = Instant::now() + WORKER_SILENCE_TIMEOUT;

                'gen_loop: loop {
                    let skip_user_log = server_auto_continue_count > 0 || chat_request.auto_continue;
                    let image_data = if server_auto_continue_count == 0 { chat_request.image_data.clone() } else { None };

                    let (mut rx, done_rx) = match bridge
                        .generate(
                            current_message.clone(),
                            current_conv_id.clone(),
                            skip_user_log,
                            image_data,
                            chat_request.agent_id.clone(),
                        )
                        .await
                    {
                        Ok(r) => r,
                        Err(e) => {
                            sys_error!("[WS_CHAT] Failed to start generation: {}", e);
                            let msg = format!("Failed to start generation: {}", e);
                            if let Some(ref conv_id) = current_conv_id {
                                let _ = db.append_error_message(conv_id, &msg);
                            }
                            let error_msg = serde_json::json!({"type": "error", "error": msg});
                            let _ = ws_sender.send(WsMessage::Text(error_msg.to_string())).await;
                            break 'gen_loop;
                        }
                    };

                    sys_debug!("[WS_CHAT] Generation started (server-continue #{})", server_auto_continue_count);

                    // Per-generation completion state (reset each turn)
                    let mut completed_conv_id: Option<String> = None;
                    let mut completed_finish_reason: Option<String> = None;
                    let mut completed_done_msg: Option<serde_json::Value> = None;

                    'stream_loop: loop {
                        tokio::select! {
                            token_result = rx.recv() => {
                                match token_result {
                                    Some(token_data) => {
                                        // Status messages: send via WebSocket AND update bridge for API polling
                                        if let Some(status) = &token_data.status {
                                            let status_json = serde_json::json!({
                                                "type": "status",
                                                "message": status
                                            });
                                            let _ = ws_sender.send(WsMessage::Text(status_json.to_string())).await;
                                            bridge.set_status_message(Some(status.clone())).await;
                                        }
                                        // Tool timing events: send immediately so frontend can annotate live
                                        if let Some(ref timing) = token_data.tool_timing {
                                            let timing_json = serde_json::json!({
                                                "type": "tool_timing",
                                                "name": timing.name,
                                                "duration_ms": timing.duration_ms
                                            });
                                            let _ = ws_sender.send(WsMessage::Text(timing_json.to_string())).await;
                                        }
                                        pending_tokens.push_str(&token_data.token);
                                        pending_tokens_used = Some(token_data.tokens_used);
                                        pending_max_tokens = Some(token_data.max_tokens);
                                        if token_data.gen_tok_per_sec.is_some() {
                                            pending_gen_tok_per_sec = token_data.gen_tok_per_sec;
                                        }
                                        if token_data.gen_tokens.is_some() {
                                            pending_gen_tokens = token_data.gen_tokens;
                                        }
                                        heartbeat_deadline = Instant::now() + Duration::from_secs(15);
                                        worker_silence_deadline = Instant::now() + WORKER_SILENCE_TIMEOUT;

                                        if pending_tokens.len() >= WS_TOKEN_FLUSH_MAX_CHARS
                                            && flush_pending_tokens(
                                                &mut ws_sender,
                                                &mut pending_tokens,
                                                &mut pending_tokens_used,
                                                &mut pending_max_tokens,
                                                &mut pending_gen_tok_per_sec,
                                                &mut pending_gen_tokens,
                                                &mut next_flush,
                                                &mut debug,
                                            ).await.is_err() {
                                                eprintln!("[WS_CHAT] BREAK: flush_pending_tokens failed (token path)");
                                                break 'gen_loop;
                                            }
                                    }
                                    None => {
                                        // Channel closed — this generation turn is complete
                                        bridge.set_status_message(None).await;
                                        sys_info!("[WS_CHAT] Generation complete, sending done message");
                                        let _ = flush_pending_tokens(
                                            &mut ws_sender,
                                            &mut pending_tokens,
                                            &mut pending_tokens_used,
                                            &mut pending_max_tokens,
                                            &mut pending_gen_tok_per_sec,
                                            &mut pending_gen_tokens,
                                            &mut next_flush,
                                            &mut debug,
                                        ).await;

                                        match done_rx.await {
                                            Ok(GenerationResult::Complete { conversation_id, prompt_tok_per_sec, gen_tok_per_sec, gen_eval_ms, gen_tokens, prompt_eval_ms, prompt_tokens, finish_reason, token_breakdown, .. }) => {
                                                if chat_request.conversation_id.is_none() {
                                                    let _ = db.set_conversation_worker_id(
                                                        &conversation_id,
                                                        requested_worker_id.as_deref(),
                                                    );
                                                }
                                                eprintln!("[WS_CHAT] Complete: conv={conversation_id}, finish={finish_reason:?}");
                                                bridge.set_last_finish_reason(finish_reason.clone()).await;
                                                // Store done — send after can_continue check so the frontend
                                                // WS isn't closed before server auto-continue has a chance to run.
                                                completed_done_msg = Some(serde_json::json!({
                                                    "type": "done",
                                                    "conversation_id": conversation_id,
                                                    "prompt_tok_per_sec": prompt_tok_per_sec,
                                                    "gen_tok_per_sec": gen_tok_per_sec,
                                                    "gen_eval_ms": gen_eval_ms,
                                                    "gen_tokens": gen_tokens,
                                                    "prompt_eval_ms": prompt_eval_ms,
                                                    "prompt_tokens": prompt_tokens,
                                                    "finish_reason": finish_reason,
                                                    "token_breakdown": token_breakdown
                                                }));
                                                completed_conv_id = Some(conversation_id);
                                                completed_finish_reason = finish_reason;
                                            }
                                            Ok(GenerationResult::Cancelled) => {
                                                sys_info!("[WS_CHAT] Generation was cancelled");
                                                let abort_msg = serde_json::json!({ "type": "abort" });
                                                let _ = ws_sender.send(WsMessage::Text(abort_msg.to_string())).await;
                                            }
                                            Ok(GenerationResult::Error(ref e)) => {
                                                sys_error!("[WS_CHAT] Generation error from worker: {}", e);
                                                if let Some(ref conv_id) = current_conv_id {
                                                    let _ = db.append_error_message(conv_id, e);
                                                }
                                                let error_msg = serde_json::json!({"type": "error", "error": e});
                                                let _ = ws_sender.send(WsMessage::Text(error_msg.to_string())).await;
                                            }
                                            Err(e) => {
                                                sys_error!("[WS_CHAT] Generation result channel closed: {}", e);
                                                let msg = "Generation failed: result channel closed";
                                                if let Some(ref conv_id) = current_conv_id {
                                                    let _ = db.append_error_message(conv_id, msg);
                                                }
                                                let error_msg = serde_json::json!({"type": "error", "error": msg});
                                                let _ = ws_sender.send(WsMessage::Text(error_msg.to_string())).await;
                                            }
                                        }
                                        break 'stream_loop;
                                    }
                                }
                            }
                            _ = tokio::time::sleep_until(next_flush), if !pending_tokens.is_empty() => {
                                if flush_pending_tokens(
                                    &mut ws_sender,
                                    &mut pending_tokens,
                                    &mut pending_tokens_used,
                                    &mut pending_max_tokens,
                                    &mut pending_gen_tok_per_sec,
                                    &mut pending_gen_tokens,
                                    &mut next_flush,
                                    &mut debug,
                                ).await.is_err() {
                                    break 'gen_loop;
                                }
                            }
                            // Heartbeat: keep frontend alive during long-running silent commands
                            _ = tokio::time::sleep_until(heartbeat_deadline) => {
                                let hb = serde_json::json!({ "type": "heartbeat" });
                                if ws_sender.send(WsMessage::Text(hb.to_string())).await.is_err() {
                                    eprintln!("[WS_CHAT] BREAK: heartbeat send failed");
                                    break 'gen_loop;
                                }
                                heartbeat_deadline = Instant::now() + Duration::from_secs(15);
                            }
                            // Worker silence watchdog: no token for 3 min → worker is stuck
                            _ = tokio::time::sleep_until(worker_silence_deadline) => {
                                eprintln!("[WS_CHAT] Worker silent for {}s — sending error to frontend", WORKER_SILENCE_TIMEOUT.as_secs());
                                let _ = flush_pending_tokens(
                                    &mut ws_sender, &mut pending_tokens,
                                    &mut pending_tokens_used, &mut pending_max_tokens,
                                    &mut pending_gen_tok_per_sec, &mut pending_gen_tokens,
                                    &mut next_flush, &mut debug,
                                ).await;
                                let msg = "Generation timed out — worker stopped responding. Please try again.";
                                if let Some(ref conv_id) = current_conv_id {
                                    let _ = db.append_error_message(conv_id, msg);
                                }
                                let error_msg = serde_json::json!({"type": "error", "error": msg});
                                let _ = ws_sender.send(WsMessage::Text(error_msg.to_string())).await;
                                break 'gen_loop;
                            }
                            // Handle client disconnection or close messages
                            ws_msg = ws_receiver.next() => {
                                match ws_msg {
                                    Some(Ok(WsMessage::Close(_))) | None => {
                                        eprintln!("[WS_CHAT] BREAK: client close/disconnect");
                                        let _ = flush_pending_tokens(
                                            &mut ws_sender,
                                            &mut pending_tokens,
                                            &mut pending_tokens_used,
                                            &mut pending_max_tokens,
                                            &mut pending_gen_tok_per_sec,
                                            &mut pending_gen_tokens,
                                            &mut next_flush,
                                            &mut debug,
                                        ).await;
                                        break 'gen_loop;
                                    }
                                    Some(Ok(WsMessage::Ping(data))) => {
                                        let _ = ws_sender.send(WsMessage::Pong(data)).await;
                                    }
                                    Some(Err(_e)) => {
                                        eprintln!("[WS_CHAT] BREAK: client error: {_e}");
                                        break 'gen_loop;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    } // end 'stream_loop

                    // ── Server-side continuation decision ──────────────────
                    let finish_str = completed_finish_reason.as_deref().unwrap_or("");
                    let can_continue = should_server_auto_continue(finish_str)
                        && server_auto_continue_count < MAX_SERVER_AUTO_CONTINUES;

                    if let Some(conv_id) = completed_conv_id {
                        if can_continue {
                            server_auto_continue_count += 1;
                            eprintln!(
                                "[WS_CHAT] Server auto-continue {server_auto_continue_count}/{MAX_SERVER_AUTO_CONTINUES} (reason={finish_str})"
                            );
                            // Notify frontend to stay alive without completing.
                            // The frontend must NOT close the WS here — the next generation
                            // will stream tokens on the same connection.
                            let continuing_msg = serde_json::json!({"type": "server_continuing"});
                            let _ = ws_sender.send(WsMessage::Text(continuing_msg.to_string())).await;
                            current_conv_id = Some(conv_id);
                            current_message = make_server_continuation_message(finish_str, &original_message);
                            continue 'gen_loop;
                        } else {
                            // Final completion — send the deferred done message
                            if let Some(done_msg) = completed_done_msg {
                                let _ = ws_sender.send(WsMessage::Text(done_msg.to_string())).await;
                                sys_debug!("[WS_CHAT] Done message sent");
                            }
                            final_conv_id_for_title = conv_id;
                        }
                    }
                    break 'gen_loop;
                } // end 'gen_loop

                // Background: auto-generate/update title after the final response.
                if !final_conv_id_for_title.is_empty() {
                    spawn_title_generation(final_conv_id_for_title, db.clone(), bridge.clone());
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
