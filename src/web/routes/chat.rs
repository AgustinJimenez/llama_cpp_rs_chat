// Chat route handlers
use hyper::body::Bytes;
use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use crate::web::{
    chat::{get_universal_system_prompt_with_tags, tool_tags::{default_tags, derive_tool_tags_from_pairs, try_get_tool_tags_for_model}},
    config::load_config,
    database::{conversation::ConversationLogger, SharedDatabase},
    models::{ChatMessage, ChatRequest, ChatResponse},
    request_parsing::parse_json_body,
    response_helpers::{json_error, json_response},
    websocket::{handle_conversation_watch, handle_websocket},
    websocket_utils::{
        build_json_error_response, build_websocket_upgrade_response,
        calculate_websocket_accept_key, get_websocket_key, is_websocket_upgrade,
    },
};

// Import logging macros
use crate::{sys_error, sys_info};

#[cfg(not(feature = "mock"))]
use crate::web::worker::worker_bridge::SharedWorkerBridge;

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
    db: &crate::web::database::Database,
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
            logger.log_message("USER", &chat_request.message);
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
        // Start generation via worker bridge
        let (mut token_rx, _done_rx) = match bridge
            .generate(
                chat_request.message.clone(),
                None,  // New conversation (worker creates it)
                false, // Worker logs user message
                chat_request.image_data.clone(),
            )
            .await
        {
            Ok(rx) => rx,
            Err(e) => {
                return Ok(json_error(StatusCode::SERVICE_UNAVAILABLE, &e));
            }
        };

        // Use Body::channel for direct control over chunk sending
        let (mut sender, body) = Body::channel();

        // Spawn task to send tokens through the channel
        tokio::spawn(async move {
            while let Some(token_data) = token_rx.recv().await {
                // Send TokenData as JSON
                let json_str = serde_json::to_string(&token_data).unwrap_or_else(|_| {
                    r#"{"token":"","tokens_used":0,"max_tokens":0}"#.to_string()
                });
                let event = format!("data: {json_str}\n\n");

                // Send chunk immediately - this ensures no buffering
                if sender.send_data(Bytes::from(event)).await.is_err() {
                    // Client disconnected
                    break;
                }
            }
            // Send done event
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
