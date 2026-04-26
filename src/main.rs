// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[allow(dead_code)]
mod web;

use std::sync::Arc;

use chrono::Local;
use log::{error, info, warn, LevelFilter};
use log4rs::append::file::FileAppender;
use log4rs::config::{Appender, Config, Root};
use log4rs::encode::pattern::PatternEncoder;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

mod mcp_ui_server;
use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::webview::WebviewBuilder;
use tauri::{LogicalPosition, LogicalSize, WebviewUrl};

use web::config::{
    add_to_model_history, db_config_to_sampler_config, load_config, sampler_config_to_db,
};
use web::database::{Database, SharedDatabase};
use web::models::{
    BrowseFilesResponse, ChatRequest, ConversationContentResponse, ConversationFile,
    ConversationsResponse, FileItem, ModelLoadRequest, ModelResponse, ModelStatus, SamplerConfig,
};
use web::chat::tool_tags::get_tool_tags_for_model;
use web::gguf_info::extract_model_info;
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
    prompt_tok_per_sec: Option<f64>,
    gen_tok_per_sec: Option<f64>,
    gen_eval_ms: Option<f64>,
    gen_tokens: Option<i32>,
    prompt_eval_ms: Option<f64>,
    prompt_tokens: Option<i32>,
}

// ─── Logging ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct LogEntry {
    level: String,
    message: String,
}

#[derive(Deserialize)]
struct AppErrorInput {
    level: String,
    source: String,
    message: String,
    details: Option<String>,
    timestamp: Option<i64>,
}

#[derive(Serialize)]
struct AppErrorEntry {
    id: i64,
    level: String,
    source: String,
    message: String,
    details: Option<String>,
    timestamp: i64,
}

fn truncate_owned(mut text: String, max_len: usize) -> String {
    if text.len() > max_len {
        let mut end = max_len;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
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

#[tauri::command]
fn record_app_error(
    error: AppErrorInput,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    let level = error.level.trim();
    let source = error.source.trim();
    let message = error.message.trim();
    if level.is_empty() || source.is_empty() || message.is_empty() {
        return Err("level, source, and message are required".into());
    }

    let level = truncate_owned(level.to_string(), 32);
    let source = truncate_owned(source.to_string(), 128);
    let message = truncate_owned(message.to_string(), 8_000);
    let details = error.details.map(|value| truncate_owned(value, 32_000));

    db.record_app_error(
        &level,
        &source,
        &message,
        details.as_deref(),
        error.timestamp,
    )?;

    Ok(serde_json::json!({"success": true}))
}

#[tauri::command]
fn get_app_errors(
    limit: Option<usize>,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<Vec<AppErrorEntry>, String> {
    db.get_app_errors(limit.unwrap_or(100).clamp(1, 500))?
        .into_iter()
        .map(|row| {
            Ok(AppErrorEntry {
                id: row.id,
                level: row.level,
                source: row.source,
                message: row.message,
                details: row.details,
                timestamp: row.timestamp,
            })
        })
        .collect()
}

#[tauri::command]
fn clear_app_errors(db: tauri::State<'_, SharedDatabase>) -> Result<serde_json::Value, String> {
    let deleted = db.clear_app_errors()?;
    Ok(serde_json::json!({"success": true, "deleted": deleted}))
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
    let is_loading = bridge.is_loading();
    let progress = if is_loading { Some(bridge.loading_progress()) } else { None };
    let is_generating = bridge.is_generating().await;

    Ok(match bridge.model_status().await {
        Some(meta) => {
            let tags = if meta.loaded {
                Some(get_tool_tags_for_model(meta.general_name.as_deref()))
            } else {
                None
            };
            ModelStatus {
                loaded: meta.loaded,
                loading: if is_loading { Some(true) } else { None },
                loading_progress: progress,
                generating: if is_generating { Some(true) } else { None },
                active_conversation_id: None,
                status_message: None,
                model_path: Some(meta.model_path),
                last_used: None,
                memory_usage_mb: if meta.loaded { Some(512) } else { None },
                has_vision: Some(meta.has_vision),
                tool_tags: tags,
                gpu_layers: meta.gpu_layers,
                block_count: meta.block_count,
                system_prompt_tokens: None,
                tool_definitions_tokens: None, last_finish_reason: None,
            }
        }
        None => ModelStatus {
            loaded: false,
            loading: if is_loading { Some(true) } else { None },
            loading_progress: progress,
            generating: if is_generating { Some(true) } else { None },
            active_conversation_id: None,
            status_message: None,
            model_path: None,
            last_used: None,
            memory_usage_mb: None,
            has_vision: None,
            tool_tags: None,
            gpu_layers: None,
            block_count: None,
            system_prompt_tokens: None,
            tool_definitions_tokens: None, last_finish_reason: None,
        },
    })
}

#[tauri::command]
async fn load_model(
    request: ModelLoadRequest,
    bridge: tauri::State<'_, SharedWorkerBridge>,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<ModelResponse, String> {
    match bridge.load_model(&request.model_path, request.gpu_layers, request.mmproj_path).await {
        Ok(meta) => {
            add_to_model_history(&db, &request.model_path);
            Ok(ModelResponse {
                success: true,
                message: format!("Model loaded successfully from {}", request.model_path),
                status: Some(ModelStatus {
                    loaded: true,
                    loading: None,
                    loading_progress: None,
                    generating: None,
                    active_conversation_id: None,
                    status_message: None,
                    model_path: Some(meta.model_path.clone()),
                    last_used: None,
                    memory_usage_mb: Some(512),
                    has_vision: Some(meta.has_vision),
                    tool_tags: Some(get_tool_tags_for_model(meta.general_name.as_deref())),
                    gpu_layers: meta.gpu_layers,
                    block_count: meta.block_count,
                    system_prompt_tokens: None,
                    tool_definitions_tokens: None, last_finish_reason: None,
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
                loading: None,
                loading_progress: None,
                generating: None,
                active_conversation_id: None,
                status_message: None,
                model_path: None,
                last_used: None,
                memory_usage_mb: None,
                has_vision: None,
                tool_tags: None,
                gpu_layers: None,
                block_count: None,
                system_prompt_tokens: None,
                tool_definitions_tokens: None, last_finish_reason: None,
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
                title: None,
                provider_id: None,
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
    Ok(ConversationContentResponse { content, messages, provider_id: None, provider_session_id: None })
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

#[tauri::command]
async fn truncate_conversation(
    conversation_id: String,
    from_sequence: i32,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    let id = conversation_id.trim_end_matches(".txt");
    let deleted = db.truncate_messages(id, from_sequence)?;
    Ok(serde_json::json!({"success": true, "deleted": deleted}))
}

#[tauri::command]
async fn get_conversation_metrics(
    conversation_id: String,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    let conv_id = conversation_id.trim_end_matches(".txt");
    let logs = db.get_logs_for_conversation(conv_id)?;
    let metrics: Vec<_> = logs.into_iter().filter(|l| l.level == "metrics").collect();
    Ok(serde_json::to_value(&metrics).unwrap_or_default())
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
    use web::chat::{get_tool_tags_for_model, get_universal_system_prompt_with_tags};
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
            Some(get_universal_system_prompt_with_tags(&tags))
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
            request.image_data.clone(),
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
                prompt_tok_per_sec,
                gen_tok_per_sec,
                gen_eval_ms,
                gen_tokens,
                prompt_eval_ms,
                prompt_tokens,
                ..
            }) => {
                let _ = app.emit(
                    "chat-done",
                    ChatDoneEvent {
                        event_type: "done".into(),
                        conversation_id: Some(conversation_id.clone()),
                        tokens_used: Some(tokens_used),
                        max_tokens: Some(max_tokens),
                        error: None,
                        prompt_tok_per_sec,
                        gen_tok_per_sec,
                        gen_eval_ms,
                        gen_tokens,
                        prompt_eval_ms,
                        prompt_tokens,
                    },
                );

                // Auto-generate title in background
                let bridge_bg: SharedWorkerBridge = app.state::<SharedWorkerBridge>().inner().clone();
                let db_bg: SharedDatabase = app.state::<SharedDatabase>().inner().clone();
                let conv_id_for_title = conversation_id.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let conv_clean = conv_id_for_title.trim_end_matches(".txt");
                    let messages = match db_bg.get_messages(conv_clean) {
                        Ok(m) => m,
                        Err(_) => return,
                    };
                    let first_user = messages.iter().find(|m| m.role == "user");
                    let first_asst = messages.iter().find(|m| m.role == "assistant");
                    if first_user.is_none() || first_asst.is_none() { return; }
                    let fu: String = first_user.unwrap().content.chars().take(200).collect();
                    let fa: String = first_asst.unwrap().content.chars().take(200).collect();
                    let prompt = format!("User: {fu}\nAssistant: {fa}");
                    match bridge_bg.generate_title(conv_clean, &prompt).await {
                        Ok(raw) => {
                            let title = crate::web::websocket::sanitize_title(&raw);
                            eprintln!("[TAURI_TITLE] Generated: '{title}'");
                            if !title.is_empty() {
                                let _ = db_bg.update_conversation_title(conv_clean, &title);
                                // Notify frontend to refresh sidebar
                                let _ = app.emit("conversation-title-updated", serde_json::json!({
                                    "conversation_id": conv_clean,
                                    "title": title,
                                }));
                            }
                        }
                        Err(e) => eprintln!("[TAURI_TITLE] Failed: {e}"),
                    }
                });
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
                        prompt_tok_per_sec: None,
                        gen_tok_per_sec: None,
                        gen_eval_ms: None,
                        gen_tokens: None,
                        prompt_eval_ms: None,
                        prompt_tokens: None,
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
                        prompt_tok_per_sec: None,
                        gen_tok_per_sec: None,
                        gen_eval_ms: None,
                        gen_tokens: None,
                        prompt_eval_ms: None,
                        prompt_tokens: None,
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
                        prompt_tok_per_sec: None,
                        gen_tok_per_sec: None,
                        gen_eval_ms: None,
                        gen_tokens: None,
                        prompt_eval_ms: None,
                        prompt_tokens: None,
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
    let (cpu, ram, gpu, cpu_perf_pct) = {
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
    let (cpu, ram, gpu, cpu_perf_pct) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);

    // Hardware totals — populated by get_windows_system_usage on its first call.
    // Without these the frontend can't tell GPU-equipped machines from CPU-only ones,
    // which is critical so the VRAM optimizer doesn't try to load layers on a non-existent GPU.
    #[cfg(target_os = "windows")]
    let (total_ram_gb, total_vram_gb, cpu_cores, cpu_base_mhz) =
        web::routes::system::get_hardware_totals();
    #[cfg(not(target_os = "windows"))]
    let (total_ram_gb, total_vram_gb, cpu_cores, cpu_base_mhz) =
        (0.0_f32, 0.0_f32, 0_u32, 0_u32);

    let cpu_ghz = (cpu_base_mhz as f32) * cpu_perf_pct / 100.0 / 1000.0;

    Ok(serde_json::json!({
        "cpu": cpu,
        "gpu": gpu,
        "ram": ram,
        "total_ram_gb": total_ram_gb,
        "total_vram_gb": total_vram_gb,
        "cpu_cores": cpu_cores,
        "cpu_ghz": cpu_ghz,
    }))
}

fn load_provider_api_keys_json(db: &SharedDatabase) -> Option<String> {
    let conn = db.connection();
    conn.query_row(
        "SELECT provider_api_keys FROM config WHERE id = 1",
        [],
        |row| row.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
}

#[tauri::command]
async fn list_providers(db: tauri::State<'_, SharedDatabase>) -> Result<serde_json::Value, String> {
    let api_keys_json = load_provider_api_keys_json(&db);
    let providers = web::providers::list_providers_with_keys(api_keys_json.as_deref()).await;
    Ok(serde_json::json!({ "providers": providers }))
}

#[tauri::command]
async fn list_configured_providers(
    db: tauri::State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    let api_keys_json = load_provider_api_keys_json(&db);
    let providers = web::providers::list_configured_providers_with_keys(api_keys_json.as_deref());
    Ok(serde_json::json!({ "providers": providers }))
}

#[tauri::command]
async fn list_cli_providers() -> Result<serde_json::Value, String> {
    let providers = web::providers::list_cli_providers().await;
    Ok(serde_json::json!({ "providers": providers }))
}

// ─── HuggingFace Hub ──────────────────────────────────────────────────

#[tauri::command]
async fn search_hub_models(
    query: String,
    limit: Option<usize>,
    sort: Option<String>,
) -> Result<Vec<web::routes::hub::HubModel>, String> {
    let limit = limit.unwrap_or(20).min(50);
    let sort = sort.unwrap_or_else(|| "downloads".to_string());
    tokio::task::spawn_blocking(move || web::routes::hub::search_hf(&query, limit, &sort))
        .await
        .map_err(|e| format!("Task failed: {e}"))?
}

#[tauri::command]
async fn fetch_hub_tree(model_id: String) -> Result<Vec<web::routes::hub::HubFile>, String> {
    tokio::task::spawn_blocking(move || web::routes::hub::tree_hf(&model_id))
        .await
        .map_err(|e| format!("Task failed: {e}"))?
}

#[tauri::command]
async fn verify_hub_downloads(
    db: tauri::State<'_, SharedDatabase>,
) -> Result<Vec<web::database::hub_downloads::HubDownloadRecord>, String> {
    let db_clone = db.inner().clone();
    tokio::task::spawn_blocking(move || web::routes::download::verify_hub_downloads(&db_clone))
        .await
        .map_err(|e| format!("Task failed: {e}"))
}

#[tauri::command]
async fn delete_hub_download(
    id: i64,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<(), String> {
    let db_clone = db.inner().clone();
    tokio::task::spawn_blocking(move || web::routes::download::delete_hub_download_by_id(&db_clone, id))
        .await
        .map_err(|e| format!("Task failed: {e}"))?
}

#[tauri::command]
async fn download_hub_model(
    app: AppHandle,
    model_id: String,
    filename: String,
    destination: String,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<(), String> {
    use std::path::{Path, PathBuf};

    // Sanitize filename — strip any path components
    let sanitized = Path::new(&filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download.gguf")
        .to_string();

    let dest_dir = PathBuf::from(&destination);
    if !dest_dir.is_dir() {
        return Err("Destination directory does not exist".to_string());
    }

    let dest_file = dest_dir.join(&sanitized);
    let part_file = dest_dir.join(format!("{sanitized}.part"));
    let key = format!("{model_id}/{filename}");

    // If the final file already exists, emit done immediately
    if dest_file.exists() {
        let size = std::fs::metadata(&dest_file).map(|m| m.len()).unwrap_or(0);
        let _ = app.emit(
            "hub-download-progress",
            serde_json::json!({
                "key": key,
                "type": "done",
                "path": dest_file.to_string_lossy(),
                "bytes": size,
            }),
        );
        return Ok(());
    }

    // Build HF download URL
    let url = format!(
        "https://huggingface.co/{}/resolve/main/{}",
        model_id,
        urlencoding::encode(&filename),
    );

    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel::<String>(64);
    let db_clone: SharedDatabase = db.inner().clone();

    // Clone values for the blocking thread
    let model_id_b = model_id.clone();
    let filename_b = filename.clone();
    let dest_path_str = destination.clone();

    // Blocking download thread
    tokio::task::spawn_blocking(move || {
        web::routes::download::download_file_blocking(
            &url,
            &dest_file,
            &part_file,
            db_clone,
            &model_id_b,
            &filename_b,
            &dest_path_str,
            progress_tx,
        );
    });

    // Forward progress events as Tauri events — key is embedded so the
    // frontend can demux multiple concurrent downloads.
    let key_clone = key.clone();
    tokio::spawn(async move {
        while let Some(raw) = progress_rx.recv().await {
            let mut payload: serde_json::Value = match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if let serde_json::Value::Object(ref mut map) = payload {
                map.insert("key".to_string(), serde_json::Value::String(key_clone.clone()));
            }
            let _ = app.emit("hub-download-progress", payload);
        }
    });

    Ok(())
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
                    prompt_tok_per_sec: None,
                    gen_tok_per_sec: None,
                    gen_eval_ms: None,
                    gen_tokens: None,
                    prompt_eval_ms: None,
                    prompt_tokens: None,
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
            prompt_tok_per_sec: None,
            gen_tok_per_sec: None,
            gen_eval_ms: None,
            gen_tokens: None,
            prompt_eval_ms: None,
            prompt_tokens: None,
        });
    }

    messages
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
    let base = std::env::var("LLAMA_CHAT_DATA_DIR").unwrap_or_else(|_| ".".to_string());
    let log_dir = format!("{base}/logs");
    std::fs::create_dir_all(&log_dir)?;
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

// ─── Native browser panel (child WebView, no iframe restrictions) ───
//
// Opens a real native webview as a CHILD of the main window, positioned to
// look embedded inside the React UI. Unlike `<iframe>`, this webview is a
// top-level browser process — sites with X-Frame-Options (Google, Twitter,
// banks) load normally. Requires the `unstable` Tauri feature flag.

const BROWSER_PANEL_LABEL: &str = "browser-panel";

fn parse_url(s: &str) -> Result<tauri::Url, String> {
    s.parse::<tauri::Url>().map_err(|e| format!("Invalid URL: {e}"))
}

#[tauri::command]
async fn browser_panel_open(
    app: AppHandle,
    url: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let main_window = app
        .get_window("main")
        .ok_or("Main window not found")?;
    // Close any existing panel first so we don't leak webviews
    if let Some(existing) = app.webviews().get(BROWSER_PANEL_LABEL) {
        let _ = existing.close();
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    }
    let parsed = parse_url(&url)?;
    let builder = WebviewBuilder::new(BROWSER_PANEL_LABEL, WebviewUrl::External(parsed));
    main_window
        .add_child(
            builder,
            LogicalPosition::new(x, y),
            LogicalSize::new(width.max(50.0), height.max(50.0)),
        )
        .map_err(|e| format!("Failed to attach webview: {e}"))?;
    Ok(())
}

#[tauri::command]
async fn browser_panel_navigate(app: AppHandle, url: String) -> Result<(), String> {
    let webview = app
        .webviews()
        .get(BROWSER_PANEL_LABEL)
        .cloned()
        .ok_or("Browser panel not open")?;
    let parsed = parse_url(&url)?;
    webview
        .navigate(parsed)
        .map_err(|e| format!("Navigate failed: {e}"))?;
    Ok(())
}

#[tauri::command]
async fn browser_panel_resize(
    app: AppHandle,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let webview = app
        .webviews()
        .get(BROWSER_PANEL_LABEL)
        .cloned()
        .ok_or("Browser panel not open")?;
    webview
        .set_position(LogicalPosition::new(x, y))
        .map_err(|e| format!("set_position failed: {e}"))?;
    webview
        .set_size(LogicalSize::new(width.max(50.0), height.max(50.0)))
        .map_err(|e| format!("set_size failed: {e}"))?;
    Ok(())
}

#[tauri::command]
async fn browser_panel_close(app: AppHandle) -> Result<(), String> {
    if let Some(webview) = app.webviews().get(BROWSER_PANEL_LABEL).cloned() {
        webview.close().map_err(|e| format!("Close failed: {e}"))?;
    }
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

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED
                        | tauri_plugin_window_state::StateFlags::VISIBLE,
                )
                .build(),
        )
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            // Forward deep link URLs from second instance
            for arg in &args {
                if arg.starts_with("llamachat://") {
                    let _ = app.emit("deep-link", arg.clone());
                }
            }
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_deep_link::init())
        .setup(|app| {
            // Resolve app data directory for persistent storage
            let data_dir = app.path().app_data_dir()
                .expect("Failed to resolve app data directory");
            std::fs::create_dir_all(&data_dir)
                .expect("Failed to create app data directory");
            let data_dir_str = data_dir.to_string_lossy().to_string();

            // Set env var so worker process and log modules use the same directory
            std::env::set_var("LLAMA_CHAT_DATA_DIR", &data_dir_str);
            eprintln!("[TAURI] Data directory: {data_dir_str}");

            // Initialize logging (after data dir is set)
            if let Err(e) = setup_logging() {
                eprintln!("Failed to set up logging: {e}");
            }

            let db_path = data_dir.join("llama_chat.db");
            let db_path_str = db_path.to_string_lossy().to_string();

            // Initialize SQLite database
            let db: SharedDatabase = Arc::new(
                Database::new(&db_path_str)
                    .expect("Failed to initialize SQLite database"),
            );
            eprintln!("[TAURI] Database initialized at {db_path_str}");

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
                ProcessManager::spawn(&db_path_str)
                    .expect("Failed to spawn worker process"),
            );
            let bridge: SharedWorkerBridge = Arc::new(
                tauri::async_runtime::block_on(async { WorkerBridge::new(pm) }),
            );
            eprintln!("[TAURI] Worker process spawned, bridge ready");

            // Register managed state
            app.manage(db);
            app.manage(bridge);

            // MCP UI server — direct WebView2 ExecuteScript (no HTTP callbacks needed)
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                mcp_ui_server::start_default(app_handle).await;
            });

            // ─── App Menu ────────────────────────────────────────────
            let new_chat = MenuItemBuilder::with_id("new-chat", "New Chat")
                .accelerator("CmdOrCtrl+N")
                .build(app)?;
            let settings = MenuItemBuilder::with_id("open-settings", "Settings...")
                .accelerator("CmdOrCtrl+,")
                .build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit")
                .accelerator("CmdOrCtrl+Q")
                .build(app)?;

            let file_menu = SubmenuBuilder::new(app, "File")
                .item(&new_chat)
                .separator()
                .item(&settings)
                .separator()
                .item(&quit)
                .build()?;

            let edit_menu = SubmenuBuilder::new(app, "Edit")
                .item(&PredefinedMenuItem::undo(app, None)?)
                .item(&PredefinedMenuItem::redo(app, None)?)
                .separator()
                .item(&PredefinedMenuItem::cut(app, None)?)
                .item(&PredefinedMenuItem::copy(app, None)?)
                .item(&PredefinedMenuItem::paste(app, None)?)
                .item(&PredefinedMenuItem::select_all(app, None)?)
                .build()?;

            let menu = MenuBuilder::new(app)
                .item(&file_menu)
                .item(&edit_menu)
                .build()?;

            app.set_menu(menu)?;

            // ─── System Tray ─────────────────────────────────────────
            let tray_show = MenuItemBuilder::with_id("tray-show", "Show Window").build(app)?;
            let tray_new = MenuItemBuilder::with_id("tray-new-chat", "New Chat").build(app)?;
            let tray_quit = MenuItemBuilder::with_id("tray-quit", "Quit").build(app)?;

            let tray_menu = MenuBuilder::new(app)
                .item(&tray_show)
                .item(&tray_new)
                .separator()
                .item(&tray_quit)
                .build()?;

            TrayIconBuilder::new()
                .menu(&tray_menu)
                .tooltip("LLaMA Chat")
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "tray-show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        "tray-new-chat" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                            let _ = app.emit("new-chat", ());
                        }
                        "tray-quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                })
                .build(app)?;

            eprintln!("[TAURI] Menu and tray icon initialized");

            Ok(())
        })
        // ─── Menu Event Handler ──────────────────────────────────────
        .on_menu_event(|app, event| {
            match event.id().as_ref() {
                "new-chat" => {
                    let _ = app.emit("new-chat", ());
                }
                "open-settings" => {
                    let _ = app.emit("open-settings", ());
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        // ─── Window Close → Hide to Tray ─────────────────────────────
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Hide to tray instead of closing
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            log_to_file,
            record_app_error,
            get_app_errors,
            clear_app_errors,
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
            truncate_conversation,
            get_conversation_metrics,
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
            // Native browser panel (Tauri-only real webview)
            browser_panel_open,
            browser_panel_navigate,
            browser_panel_resize,
            browser_panel_close,
            // Providers
            list_providers,
            list_configured_providers,
            list_cli_providers,
            // HuggingFace Hub
            search_hub_models,
            fetch_hub_tree,
            verify_hub_downloads,
            delete_hub_download,
            download_hub_model,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
