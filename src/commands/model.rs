//! Model Tauri commands — load, unload, status, info, history.

use crate::web;
use crate::web::worker::worker_bridge::SharedWorkerBridge;
use crate::web::database::SharedDatabase;
use crate::web::chat::tool_tags::get_tool_tags_for_model;
use crate::web::gguf_info::extract_model_info;
use crate::web::models::*;
use crate::web::config::*;

// ─── Model Commands ───────────────────────────────────────────────────

#[tauri::command]
pub async fn get_model_status(
    bridge: tauri::State<'_, SharedWorkerBridge>,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<ModelStatus, String> {
    let is_loading = bridge.is_loading();
    let progress = if is_loading { Some(bridge.loading_progress()) } else { None };
    let is_generating = bridge.is_generating().await;
    let active_conv_id = if is_generating { bridge.active_conversation_id().await } else { None };
    let last_finish_reason = if !is_generating {
        bridge.last_finish_reason().await
    } else {
        None
    };

    let mut status = match bridge.model_status().await {
        Some(meta) => {
            let tags = if meta.loaded {
                Some(get_tool_tags_for_model(meta.general_name.as_deref()))
            } else {
                None
            };
            let config = load_config(&db);
            let context_size = config.context_size.or(meta.context_length);
            ModelStatus {
                loaded: meta.loaded,
                loading: if is_loading { Some(true) } else { None },
                loading_progress: progress,
                generating: if is_generating { Some(true) } else { None },
                active_conversation_id: active_conv_id.clone(),
                status_message: None,
                model_path: Some(meta.model_path),
                last_used: None,
                memory_usage_mb: if meta.loaded { Some(512) } else { None },
                has_vision: Some(meta.has_vision),
                tool_tags: tags,
                gpu_layers: meta.gpu_layers,
                block_count: meta.block_count,
                system_prompt_tokens: None,
                tool_definitions_tokens: None,
                context_size,
                last_finish_reason: last_finish_reason.clone(),
            }
        }
        None => ModelStatus {
            loaded: false,
            loading: if is_loading { Some(true) } else { None },
            loading_progress: progress,
            generating: if is_generating { Some(true) } else { None },
            active_conversation_id: active_conv_id,
            status_message: None,
            model_path: None,
            last_used: None,
            memory_usage_mb: None,
            has_vision: None,
            tool_tags: None,
            gpu_layers: None,
            block_count: None,
            system_prompt_tokens: None,
            tool_definitions_tokens: None,
            context_size: None,
            last_finish_reason,
        },
    };

    // Merge remote provider generation state if local model isn't generating
    if !is_generating {
        if let Some(remote) = web::providers::get_remote_generation() {
            status.generating = Some(true);
            status.active_conversation_id = Some(remote.conversation_id);
            status.status_message = remote.status_message;
        }
    }

    Ok(status)
}

#[tauri::command]
pub async fn load_model(
    request: ModelLoadRequest,
    bridge: tauri::State<'_, SharedWorkerBridge>,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<ModelResponse, String> {
    match bridge.load_model(&request.model_path, request.gpu_layers, request.mmproj_path).await {
        Ok(meta) => {
            add_to_model_history(&db, &request.model_path);
            let config = load_config(&db);
            let context_size = config.context_size.or(meta.context_length);
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
                    tool_definitions_tokens: None,
                    context_size,
                    last_finish_reason: None,
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
pub async fn unload_model(
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
                tool_definitions_tokens: None,
                context_size: None,
                last_finish_reason: None,
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
pub async fn hard_unload(
    bridge: tauri::State<'_, SharedWorkerBridge>,
) -> Result<serde_json::Value, String> {
    bridge.force_unload().await?;
    Ok(serde_json::json!({"success": true, "message": "Worker process killed, memory reclaimed"}))
}

#[tauri::command]
pub async fn get_model_info(model_path: String) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        let mut info = extract_model_info(&model_path)?;
        // Add tag pair detection (same as web routes/model.rs)
        let model_name = info.get("general_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if !model_name.is_empty() {
            use crate::web::chat::tool_tags::get_tag_pairs_for_model;
            info["detected_tag_pairs"] = serde_json::json!(
                get_tag_pairs_for_model(Some(&model_name))
            );
        }
        Ok(info)
    })
    .await
    .map_err(|e| format!("Task failed: {e}"))?
}

#[tauri::command]
pub async fn get_model_history(
    db: tauri::State<'_, SharedDatabase>,
) -> Result<Vec<String>, String> {
    db.get_model_history()
}

#[tauri::command]
pub async fn add_model_history(
    model_path: String,
    db: tauri::State<'_, SharedDatabase>,
) -> Result<serde_json::Value, String> {
    add_to_model_history(&db, &model_path);
    Ok(serde_json::json!({"success": true}))
}
