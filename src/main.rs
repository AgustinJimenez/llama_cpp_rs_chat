// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use chrono::Local;
use llama_cpp_chat::{
    AppState, ChatRequest, ChatResponse, Message, ModelLoadRequest, ModelMetadata, ModelResponse,
    ModelStatus, SamplerConfig,
};
use std::collections::HashMap;
use tauri::Manager;
use tauri::State;

// Logging
use log::{error, info, warn, LevelFilter};
use log4rs::append::file::FileAppender;
use log4rs::config::{Appender, Config, Root};
use log4rs::encode::pattern::PatternEncoder;
use serde::Deserialize;

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

// Tauri command wrappers
#[tauri::command]
async fn send_message(
    request: ChatRequest,
    state: State<'_, AppState>,
) -> Result<ChatResponse, String> {
    llama_cpp_chat::send_message(request, state).await
}

#[tauri::command]
async fn get_conversations(
    state: State<'_, AppState>,
) -> Result<HashMap<String, Vec<Message>>, String> {
    llama_cpp_chat::get_conversations(state).await
}

#[tauri::command]
async fn get_conversation(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<Message>, String> {
    llama_cpp_chat::get_conversation(conversation_id, state).await
}

#[tauri::command]
async fn get_sampler_config() -> Result<SamplerConfig, String> {
    llama_cpp_chat::get_sampler_config().await
}

#[tauri::command]
async fn update_sampler_config(config: SamplerConfig) -> Result<(), String> {
    llama_cpp_chat::update_sampler_config(config).await
}

#[tauri::command]
async fn get_model_status(state: State<'_, AppState>) -> Result<ModelStatus, String> {
    llama_cpp_chat::get_model_status(state).await
}

#[tauri::command]
async fn load_model(
    request: ModelLoadRequest,
    state: State<'_, AppState>,
) -> Result<ModelResponse, String> {
    llama_cpp_chat::load_model(request, state).await
}

#[tauri::command]
async fn unload_model(state: State<'_, AppState>) -> Result<ModelResponse, String> {
    llama_cpp_chat::unload_model(state).await
}

#[tauri::command]
async fn get_model_metadata(model_path: String) -> Result<ModelMetadata, String> {
    llama_cpp_chat::get_model_metadata(model_path).await
}

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
    if let Err(e) = setup_logging() {
        eprintln!("Failed to set up logging: {e}");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Focus the existing window when a second instance is launched
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            send_message,
            get_conversations,
            get_conversation,
            get_sampler_config,
            update_sampler_config,
            get_model_status,
            load_model,
            unload_model,
            get_model_metadata,
            log_to_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
