// Chat route handlers
use hyper::body::Bytes;
use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use llama_chat_engine::{get_universal_system_prompt_with_tags, tool_tags::{default_tags, derive_tool_tags_from_pairs, try_get_tool_tags_for_model}};
use llama_chat_config::load_config;
use llama_chat_db::{conversation::ConversationLogger, SharedDatabase};
use llama_chat_types::models::{ChatMessage, ChatRequest, ChatResponse};
use crate::request_parsing::parse_json_body;
use crate::response_helpers::{json_error, json_response};
use crate::websocket::{
    handle_conversation_watch, handle_websocket, make_server_continuation_message,
    should_server_auto_continue, spawn_title_generation, MAX_SERVER_AUTO_CONTINUES,
};
use crate::websocket_utils::{
    build_json_error_response, build_websocket_upgrade_response,
    calculate_websocket_accept_key, get_websocket_key, is_websocket_upgrade,
};

#[cfg(not(feature = "mock"))]
use llama_chat_worker::worker::worker_bridge::{GenerationResult, SharedWorkerBridge};

// Helper function to get current timestamp for logging
fn timestamp_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    let hours = (secs / 3600) % 24;
    let minutes = (secs / 60) % 60;
    let seconds = secs % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}

/// Resolve system prompt from database config and model general_name.
#[cfg(not(feature = "mock"))]
fn resolve_system_prompt(
    db: &llama_chat_db::Database,
    general_name: Option<&str>,
) -> Option<String> {
    let config = load_config(db);
    match config.system_prompt.as_deref() {
        Some("__AGENTIC__") => {
            // Known models use native tags; unknown fall back to saved tag_pairs
            let tags = try_get_tool_tags_for_model(general_name)
                .or_else(|| config.tag_pairs.as_ref().and_then(|pairs| derive_tool_tags_from_pairs(pairs)))
                .unwrap_or_else(default_tags);
            Some(get_universal_system_prompt_with_tags(&tags))
        }
        Some(custom) => Some(custom.to_string()),
        None => None,
    }
}

pub async fn handle_post_chat(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    // Parse request body using helper
    let chat_request: ChatRequest = match parse_json_body(req.into_body()).await {
        Ok(req) => req,
        Err(error_response) => return Ok(error_response),
    };

    #[cfg(not(feature = "mock"))]
    {
        // Check for test mode environment variable
        if std::env::var("TEST_MODE").unwrap_or_default() == "true" {
            // Fast test response
            let test_response = format!(
                "Hello! This is a test response to your message: '{}'",
                chat_request.message
            );

            let response = ChatResponse {
                message: ChatMessage {
                    id: format!("{}", uuid::Uuid::new_v4()),
                    role: "assistant".to_string(),
                    content: test_response,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    prompt_tok_per_sec: None,
                    gen_tok_per_sec: None,
                    gen_eval_ms: None,
                    gen_tokens: None,
                    prompt_eval_ms: None,
                    prompt_tokens: None,
                    compacted: false,
                    sequence_order: None,
                },
                conversation_id: chat_request
                    .conversation_id
                    .unwrap_or_else(|| format!("{}", uuid::Uuid::new_v4())),
                tokens_used: None,
                max_tokens: None,
            };

            return Ok(json_response(StatusCode::OK, &response));
        }

        // Get model's general_name from bridge metadata
        let general_name = bridge
            .model_status()
            .await
            .and_then(|m| m.general_name.clone());

        // Create or load conversation logger
        let conversation_logger = if let Some(conversation_id) = &chat_request.conversation_id {
            // Load existing conversation
            match ConversationLogger::from_existing(db.clone(), conversation_id) {
                Ok(logger) => Arc::new(Mutex::new(logger)),
                Err(e) => {
                    return Ok(json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        &format!("Failed to load conversation: {e}"),
                    ));
                }
            }
        } else {
            // Create new conversation with resolved system prompt
            let system_prompt = resolve_system_prompt(&db, general_name.as_deref());
            match ConversationLogger::new(db.clone(), system_prompt.as_deref()) {
                Ok(logger) => Arc::new(Mutex::new(logger)),
                Err(e) => {
                    return Ok(json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        &format!("Failed to create conversation logger: {e}"),
                    ));
                }
            }
        };

        // Log user message immediately so WebSocket can pick it up
        {
            let mut logger = conversation_logger.lock().unwrap();
            let estimated_tokens = (chat_request.message.len() / 4).max(1) as i32;
            logger.log_message_with_tokens("USER", &chat_request.message, Some(estimated_tokens));
        }

        // Extract conversation ID immediately so we can return it
        let conversation_id = {
            let logger = conversation_logger.lock().unwrap();
            logger.get_conversation_id()
        };

        // Submit generation to worker (skip_user_logging since we logged above)
        sys_info!(
            "[{}] [API_CHAT] Submitting generation to worker for conversation: {}",
            timestamp_now(),
            conversation_id
        );

        match bridge
            .generate(
                chat_request.message.clone(),
                Some(conversation_id.clone()),
                true, // skip_user_logging — already logged above
                chat_request.image_data.clone(),
            )
            .await
        {
            Ok(_receivers) => {
                // Drop receivers — generation runs in worker, client watches via WebSocket
            }
            Err(e) => {
                return Ok(json_error(StatusCode::SERVICE_UNAVAILABLE, &e));
            }
        }

        // Return immediately with conversation_id so frontend can connect WebSocket
        let chat_response = ChatResponse {
            message: ChatMessage {
                id: uuid::Uuid::new_v4().to_string(),
                role: "assistant".to_string(),
                content: "".to_string(), // Empty - real content comes via WebSocket
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                prompt_tok_per_sec: None,
                gen_tok_per_sec: None,
                gen_eval_ms: None,
                gen_tokens: None,
                prompt_eval_ms: None,
                prompt_tokens: None,
                compacted: false,
                sequence_order: None,
            },
            conversation_id,
            tokens_used: None, // Will be updated via WebSocket
            max_tokens: None,  // Will be updated via WebSocket
        };

        Ok(json_response(StatusCode::OK, &chat_response))
    }

    #[cfg(feature = "mock")]
    {
        let mock_response = ChatResponse {
            message: ChatMessage {
                id: "test".to_string(),
                role: "assistant".to_string(),
                content: "LLaMA integration not available (mock feature enabled)".to_string(),
                timestamp: 1234567890,
                prompt_tok_per_sec: None,
                gen_tok_per_sec: None,
                gen_eval_ms: None,
                gen_tokens: None,
                prompt_eval_ms: None,
                prompt_tokens: None,
                compacted: false,
            },
            conversation_id: "test-conversation".to_string(),
            tokens_used: None,
            max_tokens: None,
        };
        Ok(json_response(StatusCode::OK, &mock_response))
    }
}

pub async fn handle_post_chat_stream(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let _ = &db; // Worker handles DB operations

    // Parse request body using helper
    let chat_request: ChatRequest = match parse_json_body(req.into_body()).await {
        Ok(req) => req,
        Err(error_response) => return Ok(error_response),
    };

    #[cfg(not(feature = "mock"))]
    {
        // Use Body::channel for direct control over chunk sending
        let (mut sender, body) = Body::channel();
        let bridge_clone = bridge.clone();
        let db_clone = db.clone();
        let original_message = chat_request.message.clone();
        let initial_conv_id = chat_request.conversation_id.clone();
        let initial_image_data = chat_request.image_data.clone();
        let initial_auto_continue = chat_request.auto_continue;

        tokio::spawn(async move {
            let mut current_message = original_message.clone();
            let mut current_conv_id = initial_conv_id;
            let mut server_auto_continue_count = 0u32;
            let mut final_conv_id_for_title: Option<String> = None;

            'gen_loop: loop {
                let skip_user_log = server_auto_continue_count > 0 || initial_auto_continue;
                let image_data = if server_auto_continue_count == 0 {
                    initial_image_data.clone()
                } else {
                    None
                };

                let (mut token_rx, done_rx) = match bridge_clone
                    .generate(
                        current_message.clone(),
                        current_conv_id.clone(),
                        skip_user_log,
                        image_data,
                    )
                    .await
                {
                    Ok(rx) => rx,
                    Err(e) => {
                        let error_json = serde_json::json!({
                            "type": "error",
                            "error": e
                        });
                        let _ = sender
                            .send_data(Bytes::from(format!("data: {error_json}\n\n")))
                            .await;
                        break 'gen_loop;
                    }
                };

                while let Some(token_data) = token_rx.recv().await {
                    let json_str = serde_json::to_string(&token_data).unwrap_or_else(|_| {
                        r#"{"token":"","tokens_used":0,"max_tokens":0}"#.to_string()
                    });
                    let event = format!("data: {json_str}\n\n");

                    if sender.send_data(Bytes::from(event)).await.is_err() {
                        break 'gen_loop;
                    }
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
                        token_breakdown,
                    }) => {
                        bridge_clone
                            .set_last_finish_reason(finish_reason.clone())
                            .await;

                        let finish_str = finish_reason.as_deref().unwrap_or("");
                        let can_continue = should_server_auto_continue(finish_str)
                            && server_auto_continue_count < MAX_SERVER_AUTO_CONTINUES;

                        if can_continue {
                            server_auto_continue_count += 1;
                            current_conv_id = Some(conversation_id);
                            current_message =
                                make_server_continuation_message(finish_str, &original_message);
                            continue 'gen_loop;
                        }

                        final_conv_id_for_title = Some(conversation_id.clone());
                        let done_json = serde_json::json!({
                            "type": "done",
                            "conversation_id": conversation_id,
                            "tokens_used": tokens_used,
                            "max_tokens": max_tokens,
                            "prompt_tok_per_sec": prompt_tok_per_sec,
                            "gen_tok_per_sec": gen_tok_per_sec,
                            "gen_eval_ms": gen_eval_ms,
                            "gen_tokens": gen_tokens,
                            "prompt_eval_ms": prompt_eval_ms,
                            "prompt_tokens": prompt_tokens,
                            "finish_reason": finish_reason,
                            "token_breakdown": token_breakdown
                        });
                        if sender
                            .send_data(Bytes::from(format!("data: {done_json}\n\n")))
                            .await
                            .is_err()
                        {
                            break 'gen_loop;
                        }
                        break 'gen_loop;
                    }
                    Ok(GenerationResult::Cancelled) => {
                        let abort_json = serde_json::json!({ "type": "abort" });
                        let _ = sender
                            .send_data(Bytes::from(format!("data: {abort_json}\n\n")))
                            .await;
                        break 'gen_loop;
                    }
                    Ok(GenerationResult::Error(e)) => {
                        let error_json = serde_json::json!({
                            "type": "error",
                            "error": e
                        });
                        let _ = sender
                            .send_data(Bytes::from(format!("data: {error_json}\n\n")))
                            .await;
                        break 'gen_loop;
                    }
                    Err(e) => {
                        let error_json = serde_json::json!({
                            "type": "error",
                            "error": format!("Generation failed: result channel closed ({e})")
                        });
                        let _ = sender
                            .send_data(Bytes::from(format!("data: {error_json}\n\n")))
                            .await;
                        break 'gen_loop;
                    }
                }
            }

            if let Some(conv_id) = final_conv_id_for_title {
                spawn_title_generation(conv_id, db_clone, bridge_clone);
            }

            let _ = sender.send_data(Bytes::from("data: [DONE]\n\n")).await;
        });

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .header("cache-control", "no-cache")
            .header("access-control-allow-origin", "*")
            .header("connection", "keep-alive")
            .header("x-accel-buffering", "no") // Disable nginx buffering
            .body(body)
            .unwrap())
    }

    #[cfg(feature = "mock")]
    {
        Ok(json_error(
            StatusCode::OK,
            "Streaming not available (mock feature enabled)",
        ))
    }
}

/// Cancel the currently in-progress generation.
pub async fn handle_post_chat_cancel(
    #[cfg(not(feature = "mock"))] bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
) -> Result<Response<Body>, Infallible> {
    #[cfg(not(feature = "mock"))]
    {
        bridge.cancel_generation().await;
        sys_info!("[API_CHAT_CANCEL] Cancellation requested");
    }
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"success":true,"message":"Cancellation requested"}"#,
        ))
        .unwrap())
}

pub async fn handle_websocket_chat_stream(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    // Check if the request wants to upgrade to WebSocket
    if !is_websocket_upgrade(&req) {
        return Ok(build_json_error_response(
            StatusCode::BAD_REQUEST,
            "WebSocket upgrade required",
        ));
    }

    // Extract the WebSocket key and calculate accept key
    let key = get_websocket_key(&req).unwrap_or_default();
    let accept_key = calculate_websocket_accept_key(&key);

    #[cfg(not(feature = "mock"))]
    {
        // Clone state for the WebSocket handler
        let bridge_ws = bridge.clone();
        let db_ws = db.clone();

        // Spawn WebSocket handler on the upgraded connection
        tokio::spawn(async move {
            match hyper::upgrade::on(req).await {
                Ok(upgraded) => {
                    if let Err(e) = handle_websocket(upgraded, bridge_ws, db_ws).await {
                        sys_error!("[WEBSOCKET ERROR] {}", e);
                    }
                }
                Err(e) => {
                    sys_error!("[WEBSOCKET UPGRADE ERROR] {}", e);
                }
            }
        });
    }

    #[cfg(feature = "mock")]
    {
        // For mock, just ignore the upgrade
        let _ = req;
        let _ = db;
    }

    // Return 101 Switching Protocols response
    Ok(build_websocket_upgrade_response(&accept_key))
}

pub async fn handle_conversation_watch_websocket(
    req: Request<Body>,
    path: &str,
    #[cfg(not(feature = "mock"))] bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    // Extract conversation ID from path
    let conversation_id = path
        .strip_prefix("/ws/conversation/watch/")
        .unwrap_or("")
        .to_string();
    sys_info!(
        "[CONV-WATCH] WebSocket file watcher request for conversation: {}",
        conversation_id
    );

    // Check for WebSocket upgrade
    if !is_websocket_upgrade(&req) {
        return Ok(build_json_error_response(
            StatusCode::BAD_REQUEST,
            "Expected WebSocket upgrade",
        ));
    }

    // Get WebSocket key and calculate accept key
    let key = get_websocket_key(&req).unwrap_or_default();
    let accept_key = calculate_websocket_accept_key(&key);

    #[cfg(not(feature = "mock"))]
    {
        // Clone state for the WebSocket handler
        let bridge_ws = bridge.clone();
        let db_ws = db.clone();

        // Spawn WebSocket handler on the upgraded connection
        tokio::spawn(async move {
            match hyper::upgrade::on(req).await {
                Ok(upgraded) => {
                    if let Err(e) = handle_conversation_watch(
                        upgraded,
                        conversation_id,
                        bridge_ws,
                        db_ws,
                    )
                    .await
                    {
                        sys_error!("[CONV-WATCH ERROR] {}", e);
                    }
                }
                Err(e) => {
                    sys_error!("[CONV-WATCH UPGRADE ERROR] {}", e);
                }
            }
        });
    }

    #[cfg(feature = "mock")]
    {
        // For mock, just ignore the upgrade
        let _ = req;
        let _ = conversation_id;
        let _ = db;
    }

    // Return 101 Switching Protocols
    Ok(build_websocket_upgrade_response(&accept_key))
}
