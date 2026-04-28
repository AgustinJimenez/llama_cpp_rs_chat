// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate llama_chat_types;

#[allow(dead_code)]
mod web;
mod commands;

use std::sync::Arc;

use chrono::Local;
use log::{error, info, warn, LevelFilter};
use log4rs::append::file::FileAppender;
use log4rs::config::{Appender, Config, Root};
use log4rs::encode::pattern::PatternEncoder;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
// WebviewBuilder, LogicalPosition, LogicalSize, WebviewUrl moved to commands::browser_panel

mod mcp_ui_server;
use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::tray::TrayIconBuilder;

use web::config::load_config;
use web::database::{Database, SharedDatabase};
use web::models::{BrowseFilesResponse, ChatRequest, FileItem};
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

    // Notify frontend that a new conversation was created (so sidebar updates immediately)
    let _ = app.emit("conversation-title-updated", serde_json::json!({
        "conversation_id": &conversation_id,
    }));

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

                // Auto-generate/update title in background
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

                    // Check if title already exists
                    let existing_title = db_bg.list_conversations().ok()
                        .and_then(|convs| convs.into_iter().find(|c| c.id == conv_clean))
                        .map(|c| c.title)
                        .unwrap_or_default();

                    let fu: String = first_user.unwrap().content.chars().take(200).collect();
                    let fa: String = first_asst.unwrap().content.chars().take(200).collect();

                    if !existing_title.is_empty() {
                        // Title exists — only update if topic changed
                        // (last user message is substantially different from first)
                        let last_user = messages.iter().rev().find(|m| m.role == "user");
                        if let Some(lu) = last_user {
                            let fu_content = &first_user.unwrap().content;
                            // Same user message or auto-continue ("Continue working on")
                            if lu.content == *fu_content
                                || lu.content.starts_with("Continue working on")
                                || lu.content == "Continue"
                            {
                                eprintln!("[TAURI_TITLE] Same topic, keeping: '{existing_title}'");
                                return;
                            }
                        }
                        eprintln!("[TAURI_TITLE] Topic may have changed, regenerating");
                    }

                    // Generate new title
                    let prompt = format!("User: {fu}\nAssistant: {fa}");
                    match bridge_bg.generate_title(conv_clean, &prompt).await {
                        Ok(raw) => {
                            let title = crate::web::websocket::sanitize_title(&raw);
                            eprintln!("[TAURI_TITLE] Generated: '{title}'");
                            if !title.is_empty() {
                                let _ = db_bg.update_conversation_title(conv_clean, &title);
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

// ─── Helper: Parse conversation text to messages ──────────────────────

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
            commands::config::get_config,
            commands::config::save_config,
            // Model
            commands::model::get_model_status,
            commands::model::load_model,
            commands::model::unload_model,
            commands::model::hard_unload,
            commands::model::get_model_info,
            commands::model::get_model_history,
            commands::model::add_model_history,
            // Conversations
            commands::conversation::get_conversations,
            commands::conversation::get_conversation,
            commands::conversation::delete_conversation,
            commands::conversation::truncate_conversation,
            commands::conversation::get_conversation_metrics,
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
            // Native browser panel
            commands::browser_panel::browser_panel_open,
            commands::browser_panel::browser_panel_navigate,
            commands::browser_panel::browser_panel_get_info,
            commands::browser_panel::browser_panel_zoom,
            commands::browser_panel::browser_panel_set_zoom,
            commands::browser_panel::browser_panel_eval_js,
            commands::browser_panel::browser_panel_reload,
            commands::browser_panel::browser_panel_go_back,
            commands::browser_panel::browser_panel_go_forward,
            commands::browser_panel::browser_panel_resize,
            commands::browser_panel::browser_panel_close,
            // Providers
            list_providers,
            list_configured_providers,
            list_cli_providers,
            // HuggingFace Hub
            commands::hub::search_hub_models,
            commands::hub::fetch_hub_tree,
            commands::hub::verify_hub_downloads,
            commands::hub::delete_hub_download,
            commands::hub::download_hub_model,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
