// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod web;

use std::sync::Arc;

use chrono::Local;
use log::{error, info, warn, LevelFilter};
use log4rs::append::file::FileAppender;
use log4rs::config::{Appender, Config, Root};
use log4rs::encode::pattern::PatternEncoder;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use web::config::{
    add_to_model_history, db_config_to_sampler_config, load_config, sampler_config_to_db,
};
use web::database::{Database, SharedDatabase};
use web::models::{
    BrowseFilesResponse, ChatRequest, ConversationContentResponse, ConversationFile,
    ConversationsResponse, FileItem, ModelLoadRequest, ModelResponse, ModelStatus, SamplerConfig,
};
use web::worker::process_manager::ProcessManager;
use web::worker::worker_bridge::{GenerationResult, SharedWorkerBridge, WorkerBridge};

// ─── Event Payloads ───────────────────────────────────────────────────

#[derive(Serialize, Clone)]
struct ChatTokenEvent {
    token: String,
    tokens_used: i32,
    max_tokens: i32,
}

#[derive(Serialize, Clone)]
struct ChatDoneEvent {
    #[serde(rename = "type")]
    event_type: String,
    conversation_id: Option<String>,
    tokens_used: Option<i32>,
    max_tokens: Option<i32>,
    error: Option<String>,
}

// ─── Logging ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct LogEntry {
    level: String,
    message: String,
}

#[tauri::command]
fn log_to_file(logs: Vec<LogEntry>) {
    for log in logs {
        match log.level.as_str() {
            "info" => info!("[FRONTEND] {}", log.message),
            "warn" => warn!("[FRONTEND] {}", log.message),
            "error" => error!("[FRONTEND] {}", log.message),
            _ => info!("[FRONTEND] {}", log.message),
        }
    }
}

// ─── Configuration Commands ───────────────────────────────────────────

#[tauri::command]
async fn get_config(db: tauri::State<'_, SharedDatabase>) -> Result<SamplerConfig, String> {
    let db_config = db.load_config();
    Ok(db_config_to_sampler_config(&db_config))
}

#[tauri::command]
async fn save_config(
    config: SamplerConfig,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    if !(0.0..=2.0).contains(&config.temperature) {
        return Err("temperature must be between 0.0 and 2.0".into());
    }
    if !(0.0..=1.0).contains(&config.top_p) {
        return Err("top_p must be between 0.0 and 1.0".into());
    }
    if config.context_size.unwrap_or(0) == 0 {
        return Err("context_size must be positive".into());
    }

    let existing = db.load_config();
    let mut merged = sampler_config_to_db(&config);
    merged.model_history = existing.model_history;

    db.save_config(&merged)
        .map_err(|e| format!("Failed to save configuration: {e}"))?;

    Ok(serde_json::json!({"success": true}))
}

// ─── Model Commands ───────────────────────────────────────────────────

#[tauri::command]
async fn get_model_status(
    bridge: tauri::State<'_, SharedWorkerBridge>,
) -> Result<ModelStatus, String> {
    Ok(match bridge.model_status().await {
        Some(meta) => ModelStatus {
            loaded: meta.loaded,
            model_path: Some(meta.model_path),
            last_used: None,
            memory_usage_mb: if meta.loaded { Some(512) } else { None },
        },
        None => ModelStatus {
            loaded: false,
            model_path: None,
            last_used: None,
            memory_usage_mb: None,
        },
    })
}

#[tauri::command]
async fn load_model(
    request: ModelLoadRequest,
    bridge: tauri::State<'_, SharedWorkerBridge>,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<ModelResponse, String> {
    match bridge.load_model(&request.model_path, request.gpu_layers).await {
        Ok(meta) => {
            add_to_model_history(&db, &request.model_path);
            Ok(ModelResponse {
                success: true,
                message: format!("Model loaded successfully from {}", request.model_path),
                status: Some(ModelStatus {
                    loaded: true,
                    model_path: Some(meta.model_path),
                    last_used: None,
                    memory_usage_mb: Some(512),
                }),
            })
        }
        Err(e) => Ok(ModelResponse {
            success: false,
            message: format!("Failed to load model: {e}"),
            status: None,
        }),
    }
}

#[tauri::command]
async fn unload_model(
    bridge: tauri::State<'_, SharedWorkerBridge>,
) -> Result<ModelResponse, String> {
    match bridge.unload_model().await {
        Ok(_) => Ok(ModelResponse {
            success: true,
            message: "Model unloaded successfully".into(),
            status: Some(ModelStatus {
                loaded: false,
                model_path: None,
                last_used: None,
                memory_usage_mb: None,
            }),
        }),
        Err(e) => Ok(ModelResponse {
            success: false,
            message: format!("Failed to unload model: {e}"),
            status: None,
        }),
    }
}

#[tauri::command]
async fn hard_unload(
    bridge: tauri::State<'_, SharedWorkerBridge>,
) -> Result<serde_json::Value, String> {
    bridge.force_unload().await?;
    Ok(serde_json::json!({"success": true, "message": "Worker process killed, memory reclaimed"}))
}

#[tauri::command]
async fn get_model_info(model_path: String) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || extract_model_info(&model_path))
        .await
        .map_err(|e| format!("Task failed: {e}"))?
}

#[tauri::command]
async fn get_model_history(
    db: tauri::State<'_, SharedDatabase>,
) -> Result<Vec<String>, String> {
    db.get_model_history()
}

#[tauri::command]
async fn add_model_history(
    model_path: String,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    add_to_model_history(&db, &model_path);
    Ok(serde_json::json!({"success": true}))
}

// ─── Conversation Commands ────────────────────────────────────────────

#[tauri::command]
async fn get_conversations(
    db: tauri::State<'_, SharedDatabase>,
) -> Result<ConversationsResponse, String> {
    let records = db.list_conversations().unwrap_or_default();
    let conversations = records
        .into_iter()
        .map(|r| {
            let timestamp_part = r.id.strip_prefix("chat_").unwrap_or(&r.id).to_string();
            ConversationFile {
                name: format!("{}.txt", r.id),
                display_name: format!("Chat {timestamp_part}"),
                timestamp: timestamp_part,
            }
        })
        .collect();
    Ok(ConversationsResponse { conversations })
}

#[tauri::command]
async fn get_conversation(
    filename: String,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<ConversationContentResponse, String> {
    let conversation_id = filename.trim_end_matches(".txt");
    let content = db.get_conversation_as_text(conversation_id)?;
    let messages = parse_conversation_to_messages(&content);
    Ok(ConversationContentResponse { content, messages })
}

#[tauri::command]
async fn delete_conversation(
    filename: String,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return Err("Invalid filename".into());
    }
    if !filename.starts_with("chat_") {
        return Err("Invalid conversation file".into());
    }
    let conversation_id = filename.trim_end_matches(".txt");
    db.delete_conversation(conversation_id)?;
    Ok(serde_json::json!({"success": true}))
}

// ─── Chat Commands ────────────────────────────────────────────────────

#[tauri::command]
async fn generate_stream(
    app: AppHandle,
    request: ChatRequest,
    bridge: tauri::State<'_, SharedWorkerBridge>,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    use std::sync::Mutex;
    use web::chat_handler::{get_tool_tags_for_model, get_universal_system_prompt_with_tags};
    use web::database::conversation::ConversationLogger;

    // Resolve system prompt
    let general_name = bridge
        .model_status()
        .await
        .and_then(|m| m.general_name.clone());
    let config = load_config(&db);
    let system_prompt = match config.system_prompt.as_deref() {
        Some("__AGENTIC__") => {
            let tags = get_tool_tags_for_model(general_name.as_deref());
            Some(get_universal_system_prompt_with_tags(tags))
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

    // Log user message
    {
        let mut logger = shared_logger.lock().unwrap();
        logger.log_message("USER", &request.message);
    }

    let conversation_id = {
        let logger = shared_logger.lock().unwrap();
        logger.get_conversation_id()
    };

    // Start generation (skip_user_logging since we logged above)
    let (mut token_rx, done_rx) = bridge
        .generate(
            request.message.clone(),
            Some(conversation_id.clone()),
            true,
        )
        .await?;

    // Spawn task to forward tokens as Tauri events
    let conv_id = conversation_id.clone();
    tokio::spawn(async move {
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
                ..
            }) => {
                let _ = app.emit(
                    "chat-done",
                    ChatDoneEvent {
                        event_type: "done".into(),
                        conversation_id: Some(conversation_id),
                        tokens_used: Some(tokens_used),
                        max_tokens: Some(max_tokens),
                        error: None,
                    },
                );
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
                    },
                );
            }
        }
    });

    Ok(serde_json::json!({ "conversation_id": conversation_id }))
}

#[tauri::command]
async fn cancel_generation(
    bridge: tauri::State<'_, SharedWorkerBridge>,
) -> Result<serde_json::Value, String> {
    bridge.cancel_generation().await;
    Ok(serde_json::json!({"success": true, "message": "Cancellation requested"}))
}

// ─── File Browser ─────────────────────────────────────────────────────

#[tauri::command]
async fn browse_files(path: Option<String>) -> Result<BrowseFilesResponse, String> {
    let browse_path = path.unwrap_or_else(|| ".".into());
    let path_obj = std::path::Path::new(&browse_path);
    if !path_obj.exists() {
        return Err("Directory not found".into());
    }

    let mut files = Vec::new();
    let mut dir = tokio::fs::read_dir(&browse_path)
        .await
        .map_err(|e| format!("Failed to read directory: {e}"))?;

    while let Ok(Some(entry)) = dir.next_entry().await {
        let entry_path = entry.path();
        if let (Some(name), Some(path_str)) = (
            entry_path.file_name().and_then(|n| n.to_str()),
            entry_path.to_str(),
        ) {
            let is_directory = entry_path.is_dir();
            let size = if !is_directory {
                entry.metadata().await.ok().map(|m| m.len())
            } else {
                None
            };
            files.push(FileItem {
                name: name.to_string(),
                path: path_str.to_string(),
                is_directory,
                size,
            });
        }
    }

    files.sort_by(|a, b| match (a.is_directory, b.is_directory) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    let parent_path = std::path::Path::new(&browse_path)
        .parent()
        .and_then(|p| p.to_str())
        .map(|s| s.to_string());

    Ok(BrowseFilesResponse {
        files,
        current_path: browse_path,
        parent_path,
    })
}

// ─── Tool Execution ───────────────────────────────────────────────────

#[derive(Deserialize)]
struct ToolExecuteRequest {
    tool_name: String,
    arguments: serde_json::Value,
}

#[tauri::command]
async fn execute_tool(
    request: ToolExecuteRequest,
    bridge: tauri::State<'_, SharedWorkerBridge>,
) -> Result<serde_json::Value, String> {
    let meta = bridge.model_status().await;
    let chat_template = meta
        .as_ref()
        .and_then(|m| m.chat_template_type.as_deref())
        .unwrap_or("Unknown");
    let capabilities = web::models::get_model_capabilities(chat_template);
    let (tool_name, tool_arguments) = web::models::translate_tool_for_model(
        &request.tool_name,
        &request.arguments,
        &capabilities,
    );

    let result = match tool_name.as_str() {
        "read_file" => {
            let path = tool_arguments
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if path.is_empty() {
                return Err("File path is required".into());
            }
            match tokio::fs::read_to_string(path).await {
                Ok(content) => serde_json::json!({"success": true, "result": content, "path": path}),
                Err(e) => serde_json::json!({"success": false, "error": format!("Failed to read file: {e}")}),
            }
        }
        "write_file" => {
            let path = tool_arguments
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let content = tool_arguments
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if path.is_empty() {
                return Err("File path is required".into());
            }
            match tokio::fs::write(path, content).await {
                Ok(_) => serde_json::json!({
                    "success": true,
                    "result": format!("Wrote {} bytes to '{}'", content.len(), path),
                    "path": path,
                    "bytes_written": content.len()
                }),
                Err(e) => serde_json::json!({"success": false, "error": format!("Failed to write file: {e}")}),
            }
        }
        "list_directory" => {
            let path = tool_arguments
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            match tokio::fs::read_dir(path).await {
                Ok(mut entries) => {
                    let mut items = Vec::new();
                    while let Ok(Some(e)) = entries.next_entry().await {
                        let meta = e.metadata().await.ok();
                        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                        let size =
                            meta.and_then(|m| if m.is_file() { Some(m.len()) } else { None });
                        items.push(format!(
                            "{:>10} {:>15} {}",
                            if is_dir { "DIR" } else { "FILE" },
                            size.map(|s| format!("{s} bytes")).unwrap_or_default(),
                            e.file_name().to_string_lossy()
                        ));
                    }
                    serde_json::json!({"success": true, "result": items.join("\n"), "count": items.len()})
                }
                Err(e) => {
                    serde_json::json!({"success": false, "error": format!("Failed to list directory: {e}")})
                }
            }
        }
        "bash" | "shell" | "command" => {
            let command = tool_arguments
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if command.is_empty() {
                return Err("Command is required".into());
            }
            let cmd = command.to_string();
            let exec = tokio::task::spawn_blocking(move || {
                if cfg!(target_os = "windows") {
                    std::process::Command::new("powershell")
                        .args(["-NoProfile", "-NonInteractive", "-Command", &cmd])
                        .output()
                } else {
                    std::process::Command::new("sh")
                        .arg("-c")
                        .arg(&cmd)
                        .output()
                }
            });
            match tokio::time::timeout(std::time::Duration::from_secs(15), exec).await {
                Ok(Ok(Ok(output))) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    let combined = if stderr.is_empty() {
                        stdout
                    } else {
                        format!("{stdout}\nSTDERR:\n{stderr}")
                    };
                    serde_json::json!({"success": true, "result": combined, "exit_code": output.status.code()})
                }
                Ok(Ok(Err(e))) => {
                    serde_json::json!({"success": false, "error": format!("Failed to execute: {e}")})
                }
                Ok(Err(e)) => {
                    serde_json::json!({"success": false, "error": format!("Task failed: {e}")})
                }
                Err(_) => {
                    serde_json::json!({"success": false, "error": "Command timed out after 15s"})
                }
            }
        }
        "web_fetch" => {
            let url = tool_arguments
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if url.is_empty() {
                return Err("URL is required".into());
            }
            let max_chars = tool_arguments
                .get("max_length")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .unwrap_or(10_000);
            let url_owned = url.to_string();
            tokio::task::spawn_blocking(move || {
                web::routes::tools::fetch_url_as_text(&url_owned, max_chars)
            })
            .await
            .map_err(|e| format!("Task failed: {e}"))?
        }
        _ => serde_json::json!({"success": false, "error": format!("Unknown tool: {}", request.tool_name)}),
    };

    Ok(result)
}

#[tauri::command]
async fn web_fetch(
    url: String,
    max_length: Option<usize>,
) -> Result<serde_json::Value, String> {
    let max_chars = max_length.unwrap_or(10_000);
    tokio::task::spawn_blocking(move || web::routes::tools::fetch_url_as_text(&url, max_chars))
        .await
        .map_err(|e| format!("Task failed: {e}"))?
        .pipe(Ok)
}

// ─── System Usage ─────────────────────────────────────────────────────

#[tauri::command]
async fn get_system_usage() -> Result<serde_json::Value, String> {
    #[cfg(target_os = "windows")]
    let (cpu, ram, gpu) = {
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(1500),
            tokio::task::spawn_blocking(web::routes::system::get_windows_system_usage),
        )
        .await;
        match result {
            Ok(Ok(values)) => values,
            _ => web::routes::system::get_cached_windows_system_usage(),
        }
    };

    #[cfg(not(target_os = "windows"))]
    let (cpu, ram, gpu) = (0.0f32, 0.0f32, 0.0f32);

    Ok(serde_json::json!({"cpu": cpu, "gpu": gpu, "ram": ram}))
}

// ─── Helper: Parse conversation text to messages ──────────────────────

fn parse_conversation_to_messages(content: &str) -> Vec<web::models::ChatMessage> {
    let mut messages = Vec::new();
    let mut current_role = String::new();
    let mut current_content = String::new();
    let mut sequence = 0u64;

    for line in content.lines() {
        if line == "SYSTEM:" || line == "USER:" || line == "ASSISTANT:" {
            if !current_role.is_empty() && !current_content.trim().is_empty() {
                messages.push(web::models::ChatMessage {
                    id: format!("msg_{sequence}"),
                    role: current_role.to_lowercase(),
                    content: current_content.trim().to_string(),
                    timestamp: sequence,
                });
                sequence += 1;
            }
            current_role = line.trim_end_matches(':').to_string();
            current_content.clear();
        } else {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    if !current_role.is_empty() && !current_content.trim().is_empty() {
        messages.push(web::models::ChatMessage {
            id: format!("msg_{sequence}"),
            role: current_role.to_lowercase(),
            content: current_content.trim().to_string(),
            timestamp: sequence,
        });
    }

    messages
}

// ─── Helper: Extract GGUF model info ──────────────────────────────────

fn extract_model_info(decoded_path: &str) -> Result<serde_json::Value, String> {
    use gguf_llms::{GgufHeader, GgufReader};
    use std::io::BufReader;
    use web::filename_patterns::{detect_architecture, detect_parameters, detect_quantization};
    use web::gguf_utils::{
        detect_tool_format, extract_default_system_prompt, MetadataExtractor,
    };

    const BYTES_PER_GB: u64 = 1_073_741_824;
    const BYTES_PER_MB: u64 = 1_048_576;

    let path_obj = std::path::Path::new(decoded_path);
    if !path_obj.exists() {
        return Err("Model file not found".into());
    }

    if path_obj.is_dir() {
        let mut gguf_files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(path_obj) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    if let Some(ext) = p.extension() {
                        if ext.eq_ignore_ascii_case("gguf") {
                            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                                gguf_files.push(name.to_string());
                            }
                        }
                    }
                }
            }
        }
        return Err(if gguf_files.is_empty() {
            "This is a directory. No .gguf files found.".into()
        } else {
            format!(
                "This is a directory. Found {} .gguf file(s): {}",
                gguf_files.len(),
                gguf_files.join(", ")
            )
        });
    }

    if let Some(ext) = path_obj.extension() {
        if !ext.eq_ignore_ascii_case("gguf") {
            return Err("File must have .gguf extension".into());
        }
    } else {
        return Err("File must have .gguf extension".into());
    }

    let file_metadata =
        std::fs::metadata(decoded_path).map_err(|_| "Failed to read file metadata".to_string())?;
    let file_size_bytes = file_metadata.len();
    let file_size = if file_size_bytes >= BYTES_PER_GB {
        format!("{:.1} GB", file_size_bytes as f64 / BYTES_PER_GB as f64)
    } else if file_size_bytes >= BYTES_PER_MB {
        format!("{:.1} MB", file_size_bytes as f64 / BYTES_PER_MB as f64)
    } else {
        format!("{file_size_bytes} bytes")
    };

    let filename = path_obj
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let architecture = detect_architecture(filename);
    let parameters = detect_parameters(filename);
    let quantization = detect_quantization(filename);

    let model_size_gb = file_size_bytes as f64 / BYTES_PER_GB as f64;
    let estimated_total_layers = if model_size_gb < 8.0 {
        36
    } else if model_size_gb < 15.0 {
        45
    } else if model_size_gb < 25.0 {
        60
    } else {
        80
    };

    let mut model_info = serde_json::json!({
        "name": filename,
        "architecture": architecture,
        "parameters": parameters,
        "quantization": quantization,
        "file_size": file_size,
        "context_length": "Variable",
        "path": decoded_path,
        "estimated_layers": estimated_total_layers
    });

    // Try to parse GGUF metadata
    if let Ok(file) = std::fs::File::open(decoded_path) {
        let mut reader = BufReader::new(file);
        if let Ok(header) = GgufHeader::parse(&mut reader) {
            if let Ok(metadata) = GgufReader::read_metadata(&mut reader, header.n_kv) {
                let extractor = MetadataExtractor::new(&metadata);
                model_info["gguf_metadata"] = serde_json::json!(extractor.to_json_map());

                let arch = extractor
                    .get_string("general.architecture")
                    .unwrap_or_else(|| "llama".to_string());
                model_info["architecture"] = serde_json::json!(arch.clone());

                let model_name = extractor.get_string("general.name").unwrap_or_default();
                model_info["tool_format"] = serde_json::json!(detect_tool_format(&arch, &model_name));

                // Core model fields
                for (gguf_key, json_key) in [
                    ("general.name", "general_name"),
                    ("general.author", "author"),
                    ("general.version", "version"),
                    ("general.organization", "organization"),
                    ("general.description", "description"),
                    ("general.license", "license"),
                    ("general.url", "url"),
                    ("general.repo_url", "repo_url"),
                    ("general.file_type", "file_type"),
                    ("general.quantization_version", "quantization_version"),
                ] {
                    if let Some(val) = extractor.get_string(gguf_key) {
                        model_info[json_key] = serde_json::json!(val);
                    }
                }

                // Context length
                for key in [
                    format!("{arch}.context_length"),
                    "llama.context_length".into(),
                    "context_length".into(),
                ] {
                    if let Some(val) = extractor.get_string(&key) {
                        model_info["context_length"] = serde_json::json!(val);
                        break;
                    }
                }

                // Architecture-specific fields
                for (field, json_key) in [
                    ("embedding_length", "embedding_length"),
                    ("feed_forward_length", "feed_forward_length"),
                    ("attention.head_count", "attention_head_count"),
                    ("attention.head_count_kv", "attention_head_count_kv"),
                    ("attention.layer_norm_rms_epsilon", "layer_norm_epsilon"),
                    ("rope.dimension_count", "rope_dimension_count"),
                    ("rope.freq_base", "rope_freq_base"),
                ] {
                    if let Some(val) = extractor.get_arch_field(&arch, field) {
                        model_info[json_key] = serde_json::json!(val);
                    }
                }

                // Block count (also used for layer estimation)
                if let Some(val) = extractor.get_arch_field(&arch, "block_count") {
                    model_info["block_count"] = serde_json::json!(val.clone());
                    if let Ok(count) = val.parse::<u32>() {
                        model_info["estimated_layers"] = serde_json::json!(count);
                    }
                }

                // Tokenizer
                for (gguf_key, json_key) in [
                    ("tokenizer.ggml.model", "tokenizer_model"),
                    ("tokenizer.ggml.bos_token_id", "bos_token_id"),
                    ("tokenizer.ggml.eos_token_id", "eos_token_id"),
                    ("tokenizer.ggml.padding_token_id", "padding_token_id"),
                ] {
                    if let Some(val) = extractor.get_string(gguf_key) {
                        model_info[json_key] = serde_json::json!(val);
                    }
                }

                // Chat template
                if let Some(val) = extractor.get_string("tokenizer.chat_template") {
                    model_info["chat_template"] = serde_json::json!(val.clone());
                    if let Some(prompt) = extract_default_system_prompt(&val) {
                        model_info["default_system_prompt"] = serde_json::json!(prompt);
                    }
                }

                // GGUF embedded sampling parameters
                let mut recommended = serde_json::Map::new();
                for (gguf_key, param_name) in [
                    ("general.sampling.temp", "temperature"),
                    ("general.sampling.top_p", "top_p"),
                    ("general.sampling.top_k", "top_k"),
                    ("general.sampling.min_p", "min_p"),
                    ("general.sampling.repetition_penalty", "repetition_penalty"),
                ] {
                    if let Some(val) = extractor.get_json(gguf_key) {
                        recommended.insert(param_name.to_string(), val);
                    }
                }
                if !recommended.is_empty() {
                    model_info["recommended_params"] = serde_json::json!(recommended);
                }
            }
        }
    }

    Ok(model_info)
}

// ─── Helper trait for piping ──────────────────────────────────────────

trait Pipe: Sized {
    fn pipe<F, R>(self, f: F) -> R
    where
        F: FnOnce(Self) -> R,
    {
        f(self)
    }
}
impl<T> Pipe for T {}

// ─── Setup & Main ─────────────────────────────────────────────────────

fn setup_logging() -> Result<(), Box<dyn std::error::Error>> {
    let log_dir = "logs";
    std::fs::create_dir_all(log_dir)?;
    let timestamp = Local::now().format("%Y-%m-%d-%H_%M").to_string();
    let log_path = format!("{log_dir}/{timestamp}.log");

    let file_appender = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new(
            "{d(%Y-%m-%d %H:%M:%S)} - {l} - {m}{n}",
        )))
        .build(log_path)?;

    let config = Config::builder()
        .appender(Appender::builder().build("file", Box::new(file_appender)))
        .build(Root::builder().appender("file").build(LevelFilter::Info))?;

    log4rs::init_config(config)?;

    Ok(())
}

fn main() {
    // Check for --worker flag BEFORE Tauri setup.
    // The worker creates its own runtimes internally,
    // so it must not run inside an existing tokio runtime.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--worker") {
        let db_path = args
            .windows(2)
            .find(|w| w[0] == "--db-path")
            .map(|w| w[1].as_str())
            .unwrap_or("assets/llama_chat.db");
        web::worker::worker_main::run_worker(db_path);
        return;
    }

    if let Err(e) = setup_logging() {
        eprintln!("Failed to set up logging: {e}");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            eprintln!("[TAURI] Setting up WorkerBridge and Database...");

            // Initialize SQLite database
            let db: SharedDatabase = Arc::new(
                Database::new("assets/llama_chat.db")
                    .expect("Failed to initialize SQLite database"),
            );
            eprintln!("[TAURI] Database initialized");

            // Run migrations
            match web::database::migration::migrate_existing_conversations(&db) {
                Ok(count) if count > 0 => {
                    eprintln!("[TAURI] Migrated {count} existing conversations to SQLite");
                }
                Ok(_) => {}
                Err(e) => eprintln!("[TAURI] Warning: Conversation migration failed: {e}"),
            }
            match web::database::migration::migrate_config(&db) {
                Ok(true) => eprintln!("[TAURI] Migrated config.json to SQLite"),
                Ok(false) => {}
                Err(e) => eprintln!("[TAURI] Warning: Config migration failed: {e}"),
            }

            // Spawn worker process
            let pm = Arc::new(
                ProcessManager::spawn("assets/llama_chat.db")
                    .expect("Failed to spawn worker process"),
            );
            let bridge: SharedWorkerBridge = Arc::new(WorkerBridge::new(pm));
            eprintln!("[TAURI] Worker process spawned, bridge ready");

            // Register managed state
            app.manage(db);
            app.manage(bridge);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            log_to_file,
            // Configuration
            get_config,
            save_config,
            // Model
            get_model_status,
            load_model,
            unload_model,
            hard_unload,
            get_model_info,
            get_model_history,
            add_model_history,
            // Conversations
            get_conversations,
            get_conversation,
            delete_conversation,
            // Chat
            generate_stream,
            cancel_generation,
            // Files
            browse_files,
            // Tools
            execute_tool,
            web_fetch,
            // System
            get_system_usage,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
