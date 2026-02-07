// Chat route handlers
use hyper::body::Bytes;
use hyper::{Body, Request, Response, StatusCode};
use serde_json::json;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::task::spawn_blocking;

use crate::web::{
    chat_handler::{generate_llama_response, get_universal_system_prompt_with_tags, get_tool_tags_for_model},
    config::get_resolved_system_prompt,
    database::{conversation::ConversationLogger, SharedDatabase},
    models::{ChatMessage, ChatRequest, ChatResponse, TokenData},
    request_parsing::parse_json_body,
    response_helpers::{json_error, json_response},
    websocket::{handle_conversation_watch, handle_websocket},
    websocket_utils::{
        build_json_error_response, build_websocket_upgrade_response,
        calculate_websocket_accept_key, get_websocket_key, is_websocket_upgrade,
    },
};

// Import logging macros
use crate::{sys_debug, sys_error, sys_info, sys_warn};

#[cfg(not(feature = "mock"))]
use crate::web::models::SharedLlamaState;

#[cfg(not(feature = "mock"))]
use crate::web::model_manager::unload_model;

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

pub async fn handle_post_chat(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] llama_state: SharedLlamaState,
    #[cfg(feature = "mock")] _llama_state: (),
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
                },
                conversation_id: chat_request
                    .conversation_id
                    .unwrap_or_else(|| format!("{}", uuid::Uuid::new_v4())),
                tokens_used: None,
                max_tokens: None,
            };

            return Ok(json_response(StatusCode::OK, &response));
        }

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
            // Create new conversation - determine system prompt based on config
            #[cfg(not(feature = "mock"))]
            let system_prompt = get_resolved_system_prompt(&Some(llama_state.clone()));

            #[cfg(feature = "mock")]
            let system_prompt = get_resolved_system_prompt(&None);

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

        // Spawn generation in background - don't wait for it
        let message_clone = chat_request.message.clone();
        let conversation_logger_clone = conversation_logger.clone();
        let llama_state_clone = llama_state.clone();
        let conv_id_for_log = conversation_id.clone();
        let rt_handle = tokio::runtime::Handle::current();
        sys_info!(
            "[{}] [API_CHAT] Spawning background generation task for conversation: {}",
            timestamp_now(),
            conv_id_for_log
        );
        // Use spawn_blocking to keep heavy work off the core runtime threads
        spawn_blocking(move || {
            sys_debug!(
                "[{}] [BACKGROUND_GEN] Thread started for: {}",
                timestamp_now(),
                conv_id_for_log
            );
            sys_debug!(
                "[{}] [BACKGROUND_GEN] Calling generate_llama_response...",
                timestamp_now()
            );

            // Clone state for use in panic handler (needs to outlive the closure)
            let state_for_unload = llama_state_clone.clone();

            // Wrap generation in panic handler to catch tokenization crashes
            let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                rt_handle.block_on(generate_llama_response(
                    &message_clone,
                    llama_state_clone.clone(),
                    conversation_logger_clone.clone(),
                    None,
                    true,
                ))
            }));

            match panic_result {
                Ok(result) => {
                    // Generation completed or returned error
                    match result {
                        Ok((_content, tokens, max_tok)) => {
                            sys_info!(
                                "[{}] [BACKGROUND_GEN] Generation completed: {} tokens / {} max",
                                timestamp_now(),
                                tokens,
                                max_tok
                            );
                        }
                        Err(err) => {
                            sys_error!(
                                "[{}] [BACKGROUND_GEN] Generation failed: {}",
                                timestamp_now(),
                                err
                            );

                            // Write error to conversation file so it's visible to user
                            let mut logger = conversation_logger_clone.lock().unwrap();
                            logger.log_message("SYSTEM", &format!("⚠️ Generation Error: {err}"));
                            logger.log_message("SYSTEM", "The model encountered an error during generation. This may be due to:");
                            logger.log_message(
                                "SYSTEM",
                                "  - Complex output (large code blocks, JSON)",
                            );
                            logger.log_message("SYSTEM", "  - Context size limitations");
                            logger.log_message("SYSTEM", "  - Model tokenization issues");
                            logger.log_message(
                                "SYSTEM",
                                "Try simplifying your request or reducing context size.",
                            );
                        }
                    }
                }
                Err(panic_err) => {
                    // Tokenization panic caught!
                    let panic_msg = if let Some(s) = panic_err.downcast_ref::<String>() {
                        s.clone()
                    } else if let Some(s) = panic_err.downcast_ref::<&str>() {
                        s.to_string()
                    } else {
                        "Unknown panic".to_string()
                    };

                    sys_error!(
                        "[{}] [BACKGROUND_GEN] PANIC CAUGHT: {}",
                        timestamp_now(),
                        panic_msg
                    );

                    // Write panic to conversation file
                    let mut logger = conversation_logger_clone.lock().unwrap();
                    logger.log_message("SYSTEM", "❌ Generation Crashed (Tokenization Panic)");
                    logger.log_message("SYSTEM", &format!("Panic message: {panic_msg}"));
                    logger.log_message("SYSTEM", "");
                    logger.log_message(
                        "SYSTEM",
                        "This is a known issue with the llama-cpp-2 library.",
                    );
                    logger.log_message(
                        "SYSTEM",
                        "The model has been automatically unloaded for safety.",
                    );
                    logger.log_message("SYSTEM", "");
                    logger.log_message("SYSTEM", "Please try:");
                    logger.log_message("SYSTEM", "  - Reload the model");
                    logger.log_message("SYSTEM", "  - Use a simpler, shorter prompt");
                    logger.log_message("SYSTEM", "  - Reduce context size in model settings");
                    logger.log_message(
                        "SYSTEM",
                        "  - Avoid requests for large code blocks or complex JSON",
                    );
                    drop(logger); // Release lock before unloading model

                    // Automatically unload the model to prevent further crashes
                    sys_warn!(
                        "[{}] [BACKGROUND_GEN] Auto-unloading model after panic...",
                        timestamp_now()
                    );

                    // Unload model asynchronously
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    match rt.block_on(unload_model(state_for_unload)) {
                        Ok(_) => {
                            sys_info!(
                                "[{}] [BACKGROUND_GEN] Model unloaded successfully",
                                timestamp_now()
                            );
                        }
                        Err(e) => {
                            sys_error!(
                                "[{}] [BACKGROUND_GEN] Failed to unload model: {}",
                                timestamp_now(),
                                e
                            );
                        }
                    }
                }
            }
        });

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
    #[cfg(not(feature = "mock"))] llama_state: SharedLlamaState,
    #[cfg(feature = "mock")] _llama_state: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    // Parse request body using helper
    let chat_request: ChatRequest = match parse_json_body(req.into_body()).await {
        Ok(req) => req,
        Err(error_response) => return Ok(error_response),
    };

    #[cfg(not(feature = "mock"))]
    {
        // Look up model-specific tool tags from the loaded model's general_name
        let general_name = {
            let state_guard = llama_state.lock().unwrap_or_else(|p| p.into_inner());
            state_guard.as_ref().and_then(|s| s.general_name.clone())
        };
        let tags = get_tool_tags_for_model(general_name.as_deref());

        // Create a new conversation logger with model-specific system prompt
        let universal_prompt = get_universal_system_prompt_with_tags(tags);

        // Create a new conversation logger for this chat session
        let conversation_logger = match ConversationLogger::new(db.clone(), Some(&universal_prompt))
        {
            Ok(logger) => Arc::new(Mutex::new(logger)),
            Err(e) => {
                return Ok(json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("Failed to create conversation logger: {e}"),
                ));
            }
        };

        // Create channel for streaming tokens
        let (tx, mut rx) = mpsc::unbounded_channel::<TokenData>();
        let (err_tx, mut err_rx) = mpsc::unbounded_channel::<String>();

        // Spawn generation task
        let message = chat_request.message.clone();
        let state_clone = llama_state.clone();
        let err_tx_clone = err_tx.clone();
        tokio::spawn(async move {
            match generate_llama_response(
                &message,
                state_clone,
                conversation_logger,
                Some(tx),
                false,
            )
            .await
            {
                Ok((_content, tokens, max)) => {
                    sys_debug!(
                        "[DEBUG] Generation completed successfully: {} tokens used, {} max",
                        tokens,
                        max
                    );
                }
                Err(e) => {
                    sys_error!("[ERROR] Generation failed: {}", e);
                    let _ = err_tx_clone.send(e.to_string());
                }
            }
        });

        // Use Body::channel for direct control over chunk sending
        let (mut sender, body) = Body::channel();

        // Spawn task to send tokens through the channel
        tokio::spawn(async move {
            let mut error_sent = false;
            loop {
                tokio::select! {
                    Some(token_data) = rx.recv() => {
                        // Send TokenData as JSON
                        let json = serde_json::to_string(&token_data).unwrap_or_else(|_| r#"{"token":"","tokens_used":0,"max_tokens":0}"#.to_string());
                        let event = format!("data: {json}\n\n");

                        // Send chunk immediately - this ensures no buffering
                        if sender.send_data(Bytes::from(event)).await.is_err() {
                            // Client disconnected
                            break;
                        }
                    }
                    Some(err_msg) = err_rx.recv() => {
                        let error_event = format!("event: error\ndata: {}\n\n", json!({ "error": err_msg }));
                        let _ = sender.send_data(Bytes::from(error_event)).await;
                        error_sent = true;
                        break;
                    }
                    else => {
                        // Channel closed, generation complete
                        break;
                    }
                }
            }
            // Send done event unless an error was sent
            if !error_sent {
                let _ = sender.send_data(Bytes::from("data: [DONE]\n\n")).await;
            }
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
        Ok(json_raw(
            StatusCode::OK,
            r#"{"error":"Streaming not available (mock feature enabled)"}"#.to_string(),
        ))
    }
}

pub async fn handle_websocket_chat_stream(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] llama_state: SharedLlamaState,
    #[cfg(feature = "mock")] _llama_state: (),
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
        let llama_state_ws = llama_state.clone();
        let db_ws = db.clone();

        // Spawn WebSocket handler on the upgraded connection
        tokio::spawn(async move {
            match hyper::upgrade::on(req).await {
                Ok(upgraded) => {
                    if let Err(e) = handle_websocket(upgraded, Some(llama_state_ws), db_ws).await {
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
    #[cfg(not(feature = "mock"))] llama_state: SharedLlamaState,
    #[cfg(feature = "mock")] _llama_state: (),
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
        let llama_state_ws = llama_state.clone();
        let db_ws = db.clone();

        // Spawn WebSocket handler on the upgraded connection
        tokio::spawn(async move {
            match hyper::upgrade::on(req).await {
                Ok(upgraded) => {
                    if let Err(e) = handle_conversation_watch(
                        upgraded,
                        conversation_id,
                        Some(llama_state_ws),
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
