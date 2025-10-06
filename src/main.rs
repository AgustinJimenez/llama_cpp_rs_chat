// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use llama_cpp_chat::{AppState, ChatRequest, ChatResponse, Message, SamplerConfig};
use std::collections::HashMap;
use tauri::State;

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

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            send_message,
            get_conversations,
            get_conversation,
            get_sampler_config,
            update_sampler_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}