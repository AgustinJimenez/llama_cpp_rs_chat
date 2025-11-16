// Chat route handlers

use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use hyper::body::Bytes;

use crate::web::{
    models::{ChatRequest, ChatResponse, ChatMessage, TokenData},
    conversation::ConversationLogger,
    config::load_config,
    chat_handler::generate_llama_response,
    websocket::{handle_websocket, handle_conversation_watch},
};

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
    format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis)
}

pub async fn handle_post_chat(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))]
    llama_state: SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    // Parse request body
    let body_bytes = match hyper::body::to_bytes(req.into_body()).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"error":"Failed to read request body"}"#))
                .unwrap());
        }
    };

    // Debug: log the received JSON
    if let Ok(body_str) = std::str::from_utf8(&body_bytes) {
        println!("Received request body: {}", body_str);
    }

    let chat_request: ChatRequest = match serde_json::from_slice(&body_bytes) {
        Ok(req) => req,
        Err(e) => {
            println!("JSON parsing error: {}", e);
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"error":"Invalid JSON format"}"#))
                .unwrap());
        }
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
                conversation_id: chat_request.conversation_id.unwrap_or_else(|| format!("{}", uuid::Uuid::new_v4())),
                tokens_used: None,
                max_tokens: None,
            };

            return Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(serde_json::to_string(&response).unwrap()))
                .unwrap());
        }

        // Create or load conversation logger
        let conversation_logger = if let Some(conversation_id) = &chat_request.conversation_id {
            // Load existing conversation
            match ConversationLogger::from_existing(conversation_id) {
                Ok(logger) => Arc::new(Mutex::new(logger)),
                Err(e) => {
                    return Ok(Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(format!(r#"{{"error":"Failed to load conversation: {}"}}"#, e)))
                        .unwrap());
                }
            }
        } else {
            // Create new conversation
            let config = load_config();
            let system_prompt = config.system_prompt.as_deref();

            match ConversationLogger::new(system_prompt) {
                Ok(logger) => Arc::new(Mutex::new(logger)),
                Err(e) => {
                    return Ok(Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(format!(r#"{{"error":"Failed to create conversation logger: {}"}}"#, e)))
                        .unwrap());
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
        eprintln!("[{}] [API_CHAT] Spawning background generation task for conversation: {}",
            timestamp_now(), conv_id_for_log);
        // Use std::thread instead of tokio::spawn to avoid deadlocks
        // Generation is CPU-bound and doesn't need async runtime
        std::thread::spawn(move || {
            eprintln!("[{}] [BACKGROUND_GEN] Thread started for: {}",
                timestamp_now(), conv_id_for_log);
            eprintln!("[{}] [BACKGROUND_GEN] Calling generate_llama_response...",
                timestamp_now());

            // Clone state for use in panic handler (needs to outlive the closure)
            let state_for_unload = llama_state_clone.clone();

                // Wrap generation in panic handler to catch tokenization crashes
            let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // Create a new tokio runtime for this thread
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(
                    generate_llama_response(&message_clone, llama_state_clone.clone(), conversation_logger_clone.clone(), None, true)
                )
            }));

            match panic_result {
                Ok(result) => {
                    // Generation completed or returned error
                    match result {
                        Ok((_content, tokens, max_tok)) => {
                            eprintln!("[{}] [BACKGROUND_GEN] Generation completed: {} tokens / {} max",
                                timestamp_now(), tokens, max_tok);
                        },
                        Err(err) => {
                            eprintln!("[{}] [BACKGROUND_GEN] Generation failed: {}",
                                timestamp_now(), err);

                            // Write error to conversation file so it's visible to user
                            let mut logger = conversation_logger_clone.lock().unwrap();
                            logger.log_message("SYSTEM", &format!("⚠️ Generation Error: {}", err));
                            logger.log_message("SYSTEM", "The model encountered an error during generation. This may be due to:");
                            logger.log_message("SYSTEM", "  - Complex output (large code blocks, JSON)");
                            logger.log_message("SYSTEM", "  - Context size limitations");
                            logger.log_message("SYSTEM", "  - Model tokenization issues");
                            logger.log_message("SYSTEM", "Try simplifying your request or reducing context size.");
                        }
                    }
                },
                Err(panic_err) => {
                    // Tokenization panic caught!
                    let panic_msg = if let Some(s) = panic_err.downcast_ref::<String>() {
                        s.clone()
                    } else if let Some(s) = panic_err.downcast_ref::<&str>() {
                        s.to_string()
                    } else {
                        "Unknown panic".to_string()
                    };

                    eprintln!("[{}] [BACKGROUND_GEN] PANIC CAUGHT: {}",
                        timestamp_now(), panic_msg);

                    // Write panic to conversation file
                    let mut logger = conversation_logger_clone.lock().unwrap();
                    logger.log_message("SYSTEM", "❌ Generation Crashed (Tokenization Panic)");
                    logger.log_message("SYSTEM", &format!("Panic message: {}", panic_msg));
                    logger.log_message("SYSTEM", "");
                    logger.log_message("SYSTEM", "This is a known issue with the llama-cpp-2 library.");
                    logger.log_message("SYSTEM", "The model has been automatically unloaded for safety.");
                    logger.log_message("SYSTEM", "");
                    logger.log_message("SYSTEM", "Please try:");
                    logger.log_message("SYSTEM", "  - Reload the model");
                    logger.log_message("SYSTEM", "  - Use a simpler, shorter prompt");
                    logger.log_message("SYSTEM", "  - Reduce context size in model settings");
                    logger.log_message("SYSTEM", "  - Avoid requests for large code blocks or complex JSON");
                    drop(logger); // Release lock before unloading model

                    // Automatically unload the model to prevent further crashes
                    eprintln!("[{}] [BACKGROUND_GEN] Auto-unloading model after panic...",
                        timestamp_now());

                    // Unload model asynchronously
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    match rt.block_on(unload_model(state_for_unload)) {
                        Ok(_) => {
                            eprintln!("[{}] [BACKGROUND_GEN] Model unloaded successfully",
                                timestamp_now());
                        }
                        Err(e) => {
                            eprintln!("[{}] [BACKGROUND_GEN] Failed to unload model: {}",
                                timestamp_now(), e);
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
            tokens_used: None,  // Will be updated via WebSocket
            max_tokens: None,   // Will be updated via WebSocket
        };

        let response_json = match serde_json::to_string(&chat_response) {
            Ok(json) => json,
            Err(_) => r#"{"error":"Failed to serialize response"}"#.to_string(),
        };

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .header("access-control-allow-origin", "*")
            .header("access-control-allow-methods", "GET, POST, OPTIONS")
            .header("access-control-allow-headers", "content-type")
            .body(Body::from(response_json))
            .unwrap())
    }

    #[cfg(feature = "mock")]
    {
        // Fallback mock response when using mock feature
        let mock_response = r#"{"message":{"id":"test","role":"assistant","content":"LLaMA integration not available (mock feature enabled)","timestamp":1234567890},"conversation_id":"test-conversation"}"#;
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .header("access-control-allow-origin", "*")
            .header("access-control-allow-methods", "GET, POST, OPTIONS")
            .header("access-control-allow-headers", "content-type")
            .body(Body::from(mock_response))
            .unwrap())
    }
}

pub async fn handle_post_chat_stream(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))]
    llama_state: SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    // Parse request body
    let body_bytes = match hyper::body::to_bytes(req.into_body()).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"error":"Failed to read request body"}"#))
                .unwrap());
        }
    };

    let chat_request: ChatRequest = match serde_json::from_slice(&body_bytes) {
        Ok(req) => req,
        Err(e) => {
            println!("JSON parsing error: {}", e);
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"error":"Invalid JSON format"}"#))
                .unwrap());
        }
    };

    #[cfg(not(feature = "mock"))]
    {
        // Load configuration to get system prompt
        let config = load_config();
        let system_prompt = config.system_prompt.as_deref();

        // Create a new conversation logger for this chat session
        let conversation_logger = match ConversationLogger::new(system_prompt) {
            Ok(logger) => Arc::new(Mutex::new(logger)),
            Err(e) => {
                return Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(format!(r#"{{"error":"Failed to create conversation logger: {}"}}"#, e)))
                    .unwrap());
            }
        };

        // Create channel for streaming tokens
        let (tx, mut rx) = mpsc::unbounded_channel::<TokenData>();

        // Spawn generation task
        let message = chat_request.message.clone();
        let state_clone = llama_state.clone();
        tokio::spawn(async move {
            match generate_llama_response(&message, state_clone, conversation_logger, Some(tx), false).await {
                Ok((_content, tokens, max)) => {
                    println!("[DEBUG] Generation completed successfully: {} tokens used, {} max", tokens, max);
                }
                Err(e) => {
                    println!("[ERROR] Generation failed: {}", e);
                }
            }
        });

        // Use Body::channel for direct control over chunk sending
        let (mut sender, body) = Body::channel();

        // Spawn task to send tokens through the channel
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Some(token_data) => {
                        // Send TokenData as JSON
                        let json = serde_json::to_string(&token_data).unwrap_or_else(|_| r#"{"token":"","tokens_used":0,"max_tokens":0}"#.to_string());
                        let event = format!("data: {}\n\n", json);

                        // Send chunk immediately - this ensures no buffering
                        if sender.send_data(Bytes::from(event)).await.is_err() {
                            // Client disconnected
                            break;
                        }
                    }
                    None => {
                        // Channel closed, generation complete
                        break;
                    }
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
            .header("x-accel-buffering", "no")  // Disable nginx buffering
            .body(body)
            .unwrap())
    }

    #[cfg(feature = "mock")]
    {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .header("access-control-allow-origin", "*")
            .body(Body::from(r#"{"error":"Streaming not available (mock feature enabled)"}"#))
            .unwrap())
    }
}

pub async fn handle_websocket_chat_stream(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))]
    llama_state: SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    // Check if the request wants to upgrade to WebSocket
    let upgrade_header = req.headers().get("upgrade")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_lowercase());

    if upgrade_header.as_deref() != Some("websocket") {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("content-type", "application/json")
            .header("access-control-allow-origin", "*")
            .body(Body::from(r#"{"error":"WebSocket upgrade required"}"#))
            .unwrap());
    }

    // Extract the WebSocket key before moving req
    let key = req.headers()
        .get("sec-websocket-key")
        .and_then(|k| k.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Calculate accept key using the WebSocket protocol
    let accept_key = {
        use sha1::{Digest, Sha1};
        use base64::{Engine as _, engine::general_purpose};

        let mut hasher = Sha1::new();
        hasher.update(key.as_bytes());
        hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
        let hash = hasher.finalize();
        general_purpose::STANDARD.encode(hash)
    };

    #[cfg(not(feature = "mock"))]
    {
        // Clone state for the WebSocket handler
        let llama_state_ws = llama_state.clone();

        // Spawn WebSocket handler on the upgraded connection
        tokio::spawn(async move {
            match hyper::upgrade::on(req).await {
                Ok(upgraded) => {
                    if let Err(e) = handle_websocket(upgraded, Some(llama_state_ws)).await {
                        println!("[WEBSOCKET ERROR] {}", e);
                    }
                }
                Err(e) => {
                    println!("[WEBSOCKET UPGRADE ERROR] {}", e);
                }
            }
        });
    }

    #[cfg(feature = "mock")]
    {
        // For mock, just ignore the upgrade
        let _ = req;
    }

    // Return 101 Switching Protocols response
    Ok(Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header("upgrade", "websocket")
        .header("connection", "upgrade")
        .header("sec-websocket-accept", accept_key)
        .body(Body::empty())
        .unwrap())
}

pub async fn handle_conversation_watch_websocket(
    req: Request<Body>,
    path: &str,
    #[cfg(not(feature = "mock"))]
    llama_state: SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    // Extract conversation ID from path
    let conversation_id = path.strip_prefix("/ws/conversation/watch/").unwrap_or("").to_string();
    eprintln!("[CONV-WATCH] WebSocket file watcher request for conversation: {}", conversation_id);

    // Check for WebSocket upgrade
    let upgrade_header = req.headers().get("upgrade")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if upgrade_header != "websocket" {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::from("Expected WebSocket upgrade"))
            .unwrap());
    }

    // Get WebSocket key from request
    let key = req.headers().get("sec-websocket-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Generate accept key
    let accept_key = {
        use sha1::{Sha1, Digest};
        use base64::{Engine as _, engine::general_purpose};
        let mut hasher = Sha1::new();
        hasher.update(key.as_bytes());
        hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
        let hash = hasher.finalize();
        general_purpose::STANDARD.encode(hash)
    };

    #[cfg(not(feature = "mock"))]
    {
        // Clone state for the WebSocket handler
        let llama_state_ws = llama_state.clone();

        // Spawn WebSocket handler on the upgraded connection
        tokio::spawn(async move {
            match hyper::upgrade::on(req).await {
                Ok(upgraded) => {
                    if let Err(e) = handle_conversation_watch(upgraded, conversation_id, Some(llama_state_ws)).await {
                        eprintln!("[CONV-WATCH ERROR] {}", e);
                    }
                }
                Err(e) => {
                    eprintln!("[CONV-WATCH UPGRADE ERROR] {}", e);
                }
            }
        });
    }

    #[cfg(feature = "mock")]
    {
        // For mock, just ignore the upgrade
        let _ = req;
        let _ = conversation_id;
    }

    // Return 101 Switching Protocols
    Ok(Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header("upgrade", "websocket")
        .header("connection", "upgrade")
        .header("sec-websocket-accept", accept_key)
        .body(Body::empty())
        .unwrap())
}
