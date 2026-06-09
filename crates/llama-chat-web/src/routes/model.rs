// Model route handlers

use hyper::{Body, Request, Response, StatusCode};
use serde::Deserialize;
use std::convert::Infallible;
use std::fs;
use std::io::BufReader;
use tokio::task::spawn_blocking;
use gguf_llms::{GgufHeader, GgufReader};

use llama_chat_config::add_to_model_history;
use llama_chat_db::SharedDatabase;
use llama_chat_engine::filename_patterns::{detect_architecture, detect_parameters, detect_quantization};
use llama_chat_engine::gguf_utils::{
    value_to_display_string, MetadataExtractor,
};
#[cfg(not(feature = "mock"))]
use llama_chat_engine::get_tool_tags_for_model;
#[cfg(not(feature = "mock"))]
use llama_chat_types::models::{ModelLoadRequest, ModelResponse};
use crate::request_parsing::parse_json_body;
use crate::response_helpers::{json_error, json_raw, serialize_with_fallback};

#[cfg(not(feature = "mock"))]
use llama_chat_worker::worker::worker_bridge::SharedWorkerBridge;
#[cfg(not(feature = "mock"))]
use crate::worker_pool::WorkerPool;

mod backend_install;
mod helpers;
#[path = "model/lifecycle.rs"]
mod lifecycle;

#[allow(unused_imports)]
pub use backend_install::handle_post_backends_install;
pub use lifecycle::{
    handle_get_backends, handle_get_model_history, handle_post_model_hard_unload,
    handle_post_model_history, handle_post_model_load, handle_post_model_unload,
};
use helpers::{
    default_model_status_json, detect_nvidia_gpu_hardware, enrich_model_info_from_gguf,
    scan_directory_for_gguf_files, scan_for_mmproj_files,
};

// File size constants
const BYTES_PER_GB: u64 = 1_073_741_824;
const BYTES_PER_MB: u64 = 1_048_576;

// Model size thresholds (in GB) for layer estimation
const SMALL_MODEL_THRESHOLD_GB: f64 = 8.0; // Models < 8GB (7B and below)
const MEDIUM_MODEL_THRESHOLD_GB: f64 = 15.0; // Models 8-15GB (13B)
const LARGE_MODEL_THRESHOLD_GB: f64 = 25.0; // Models 15-25GB (30B)

// Estimated layer counts for different model sizes
const SMALL_MODEL_LAYERS: u32 = 36; // 7B and below
const MEDIUM_MODEL_LAYERS: u32 = 45; // 13B
const LARGE_MODEL_LAYERS: u32 = 60; // 30B
const XLARGE_MODEL_LAYERS: u32 = 80; // 70B+


pub async fn handle_get_model_info(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] _bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
) -> Result<Response<Body>, Infallible> {
    sys_debug!("[DEBUG] /api/model/info endpoint hit");

    // Extract model path from query parameters
    let query = req.uri().query().unwrap_or("");
    sys_debug!("[DEBUG] Query string: {}", query);

    let mut model_path = "";

    for param in query.split('&') {
        if let Some((key, value)) = param.split_once('=') {
            if key == "path" {
                // URL decode the path
                model_path = value;
                sys_debug!("[DEBUG] Found path parameter (encoded): {}", model_path);
                break;
            }
        }
    }

    if model_path.is_empty() {
        sys_error!("[DEBUG] ERROR: No path parameter provided");
        return Ok(json_error(
            StatusCode::BAD_REQUEST,
            "Model path is required",
        ));
    }

    // URL decode the path properly
    let decoded_path =
        urlencoding::decode(model_path).unwrap_or(std::borrow::Cow::Borrowed(model_path));
    sys_debug!("[DEBUG] Decoded path: {}", decoded_path);

    // Check if file exists
    let path_obj = std::path::Path::new(&*decoded_path);
    let exists = path_obj.exists();
    sys_debug!("[DEBUG] File exists: {}", exists);
    sys_debug!("[DEBUG] Path is file: {}", path_obj.is_file());
    sys_debug!("[DEBUG] Path is dir: {}", path_obj.is_dir());

    if !exists {
        sys_error!(
            "[DEBUG] ERROR: File does not exist at path: {}",
            decoded_path
        );
        return Ok(json_error(StatusCode::NOT_FOUND, "Model file not found"));
    }

    // Check if path is a directory
    if path_obj.is_dir() {
        sys_debug!("[DEBUG] Path is a directory, scanning for .gguf files...");

        // Find all .gguf files in the directory (off the async runtime)
        let dir_path = path_obj.to_path_buf();
        let gguf_files = spawn_blocking(move || scan_directory_for_gguf_files(&dir_path))
            .await
            .unwrap_or_else(|_| Vec::new());

        let response_json = if gguf_files.is_empty() {
            serde_json::json!({
                "error": "This is a directory. No .gguf files found in this directory.",
                "is_directory": true,
                "suggestions": []
            })
        } else {
            serde_json::json!({
                "error": format!("This is a directory. Found {} .gguf file(s). Please select one:", gguf_files.len()),
                "is_directory": true,
                "suggestions": gguf_files
            })
        };

        sys_debug!(
            "[DEBUG] Returning directory error with {} suggestions",
            gguf_files.len()
        );
        return Ok(json_raw(StatusCode::BAD_REQUEST, response_json.to_string()));
    }

    // Check if file has .gguf extension
    if let Some(ext) = path_obj.extension() {
        if !ext.eq_ignore_ascii_case("gguf") {
            sys_error!("[DEBUG] ERROR: File is not a .gguf file");
            return Ok(json_error(
                StatusCode::BAD_REQUEST,
                "File must have .gguf extension",
            ));
        }
    } else {
        sys_error!("[DEBUG] ERROR: File has no extension");
        return Ok(json_error(
            StatusCode::BAD_REQUEST,
            "File must have .gguf extension",
        ));
    }

    // Extract basic model information
    let metadata_path = decoded_path.to_string();
    let file_metadata = match spawn_blocking(move || fs::metadata(&metadata_path)).await {
        Ok(Ok(metadata)) => metadata,
        Ok(Err(_)) | Err(_) => {
            return Ok(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to read file metadata",
            ));
        }
    };

    let file_size_bytes = file_metadata.len();
    let file_size = if file_size_bytes >= BYTES_PER_GB {
        format!("{:.1} GB", file_size_bytes as f64 / BYTES_PER_GB as f64)
    } else if file_size_bytes >= BYTES_PER_MB {
        format!("{:.1} MB", file_size_bytes as f64 / BYTES_PER_MB as f64)
    } else {
        format!("{file_size_bytes} bytes")
    };

    let filename = std::path::Path::new(&*decoded_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    // Try to extract model information from filename patterns
    let architecture = detect_architecture(filename);
    let parameters = detect_parameters(filename);
    let quantization = detect_quantization(filename);

    // Estimate total layers based on model size
    let model_size_gb = file_size_bytes as f64 / BYTES_PER_GB as f64;
    let estimated_total_layers = if model_size_gb < SMALL_MODEL_THRESHOLD_GB {
        SMALL_MODEL_LAYERS
    } else if model_size_gb < MEDIUM_MODEL_THRESHOLD_GB {
        MEDIUM_MODEL_LAYERS
    } else if model_size_gb < LARGE_MODEL_THRESHOLD_GB {
        LARGE_MODEL_LAYERS
    } else {
        XLARGE_MODEL_LAYERS
    };

    // Build base model info
    let mut model_info = serde_json::json!({
        "name": filename,
        "architecture": architecture,
        "parameters": parameters,
        "quantization": quantization,
        "file_size": file_size,
        "context_length": "Variable",
        "path": decoded_path.to_string(),
        "estimated_layers": estimated_total_layers
    });

    // Try to parse GGUF metadata
    let metadata_path = decoded_path.to_string();
    let metadata_result = spawn_blocking(
        move || -> Result<_, Box<dyn std::error::Error + Send + Sync>> {
            let file = fs::File::open(&metadata_path)?;
            let mut reader = BufReader::new(file);
            let header = GgufHeader::parse(&mut reader)?;
            let metadata = GgufReader::read_metadata(&mut reader, header.n_kv)?;
            Ok(metadata)
        },
    )
    .await;

    if let Ok(Ok(metadata)) = metadata_result {
        let extractor = MetadataExtractor::new(&metadata);

        if std::env::var("GGUF_DEBUG").unwrap_or_default() == "1" {
            sys_debug!("=== GGUF Metadata Found ===");
            for (key, value) in metadata.iter() {
                sys_debug!("  {} = {}", key, value_to_display_string(value));
            }
            sys_debug!("================================");
        }

        enrich_model_info_from_gguf(&mut model_info, &extractor);
    }

    // Scan for mmproj companion files (vision/multimodal support)
    let mmproj_path = path_obj.to_path_buf();
    let mmproj_files = spawn_blocking(move || scan_for_mmproj_files(&mmproj_path))
        .await
        .unwrap_or_else(|_| Vec::new());

    if !mmproj_files.is_empty() {
        let dir_str = path_obj
            .parent()
            .and_then(|d| d.to_str())
            .unwrap_or("");
        let mmproj_json: Vec<serde_json::Value> = mmproj_files
            .iter()
            .map(|(name, size)| {
                let size_str = if *size >= BYTES_PER_GB {
                    format!("{:.1} GB", *size as f64 / BYTES_PER_GB as f64)
                } else if *size >= BYTES_PER_MB {
                    format!("{:.1} MB", *size as f64 / BYTES_PER_MB as f64)
                } else {
                    format!("{size} bytes")
                };
                let full_path = if dir_str.is_empty() {
                    name.clone()
                } else {
                    format!("{}{}{}", dir_str, std::path::MAIN_SEPARATOR, name)
                };
                serde_json::json!({
                    "name": name,
                    "path": full_path,
                    "file_size": size_str,
                })
            })
            .collect();
        model_info["has_vision"] = serde_json::json!(true);
        model_info["mmproj_files"] = serde_json::json!(mmproj_json);
    }

    Ok(json_raw(StatusCode::OK, model_info.to_string()))
}

pub async fn handle_get_model_status(
    #[cfg(not(feature = "mock"))] pool: WorkerPool,
    #[cfg(feature = "mock")] _pool: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    #[cfg(not(feature = "mock"))]
    {
        // Select the best bridge: prefer any non-default worker with a loaded/generating model.
        // Checks global agent workers (Activate button) first, then per-conversation overflow
        // workers (lazy-spawned when an agent is staged but not activated).
        // Falls back to the default worker when no other worker is active.
        let (bridge, is_agent_model, active_agent_id): (SharedWorkerBridge, bool, Option<String>) = {
            let mut chosen: Option<(SharedWorkerBridge, Option<String>)> = None;

            // 1. Global agent workers (from Activate button)
            for (agent_id, worker_id) in pool.list_agent_bindings() {
                if let Some(b) = pool.get(&worker_id) {
                    let loaded = b.model_status().await.map(|m| m.loaded).unwrap_or(false);
                    if loaded || b.is_generating().await {
                        chosen = Some((b, Some(agent_id)));
                        break;
                    }
                }
            }

            // 2. Per-conversation overflow workers (lazy-spawned agents)
            if chosen.is_none() {
                for (conv_id, worker_id) in pool.list_conversation_workers() {
                    if let Some(b) = pool.get(&worker_id) {
                        let loaded = b.model_status().await.map(|m| m.loaded).unwrap_or(false);
                        if loaded || b.is_generating().await {
                            // Resolve agent_id for this conversation's overflow worker
                            let agent_id = db.get_conversation_agent_id(&conv_id).ok().flatten();
                            chosen = Some((b, agent_id));
                            break;
                        }
                    }
                }
            }

            match chosen {
                Some((b, agent_id)) => (b, true, agent_id),
                None => (pool.get("default").expect("Default worker missing"), false, None),
            }
        };

        // Get model status from worker bridge cached metadata (no IPC round-trip)
        let is_loading = bridge.is_loading();
        let is_generating = bridge.is_generating().await;
        let active_conv_id = bridge.active_conversation_id().await;
        // Expose finish_reason after generation ends (cleared on next generation start)
        let last_finish_reason = if !is_generating {
            bridge.last_finish_reason().await
        } else { None };
        // Try bridge status first (from WebSocket), fall back to worker global status (from IPC)
        let status_msg = match bridge.status_message().await {
            Some(s) => Some(s),
            None => bridge.get_global_status().await,
        };

        // Get cached token overhead from the most recent conversation_context
        let (sys_tokens, tool_tokens) = {
            let conn = db.connection();
            conn.query_row(
                "SELECT system_prompt_tokens, tool_definitions_tokens FROM conversation_context ORDER BY updated_at DESC LIMIT 1",
                [],
                |row| Ok((row.get::<_, i32>(0)?, row.get::<_, i32>(1)?)),
            ).unwrap_or((0, 0))
        };
        let last_model_path_fallback = bridge.last_model_path().await;
        let status = match bridge.model_status().await {
            Some(meta) => {
                let tags = if meta.loaded {
                    Some(get_tool_tags_for_model(meta.general_name.as_deref()))
                } else {
                    None
                };
                let lp = if is_loading { Some(bridge.loading_progress()) } else { None };
                // Get effective context size: prefer agent config, then global config, then model native.
                let agent_context_size = active_agent_id.as_deref()
                    .and_then(|id| db.get_agent(id).ok().flatten())
                    .and_then(|a| a.context_size);
                let config = llama_chat_config::load_config(&db);
                let context_size = agent_context_size.or(config.context_size).or(meta.context_length);
                llama_chat_types::models::ModelStatus {
                    loaded: meta.loaded,
                    loading: if is_loading { Some(true) } else { None },
                    loading_progress: lp,
                    generating: if is_generating { Some(true) } else { None },
                    active_conversation_id: active_conv_id.clone(), status_message: status_msg.clone(),
                    model_path: Some(meta.model_path),
                    last_used: None,
                    memory_usage_mb: if meta.loaded { Some(512) } else { None },
                    has_vision: Some(meta.has_vision),
                    tool_tags: tags,
                    gpu_layers: meta.gpu_layers,
                    block_count: meta.block_count,
                    system_prompt_tokens: if sys_tokens > 0 { Some(sys_tokens) } else { None },
                    tool_definitions_tokens: if tool_tokens > 0 { Some(tool_tokens) } else { None },
                    context_size,
                    last_finish_reason: last_finish_reason.clone(),
                    supports_thinking: if meta.loaded { Some(meta.supports_thinking) } else { None },
                    is_agent_model: if is_agent_model { Some(true) } else { None },
                }
            }
            None => {
                let loading_path = bridge.loading_path().await;
                let lp = if is_loading { Some(bridge.loading_progress()) } else { None };
                // If generating with no cached meta (e.g. post-crash recovery window),
                // the model must be loaded — generation is impossible otherwise.
                let effectively_loaded = is_generating;
                // Use last known model path as fallback when meta is temporarily cleared.
                let model_path = loading_path.or_else(|| {
                    if effectively_loaded { last_model_path_fallback.clone() } else { None }
                });
                llama_chat_types::models::ModelStatus {
                    loaded: effectively_loaded,
                    loading: if is_loading { Some(true) } else { None },
                    loading_progress: lp,
                    generating: if is_generating { Some(true) } else { None },
                    active_conversation_id: active_conv_id.clone(), status_message: status_msg.clone(),
                    model_path,
                    last_used: None,
                    memory_usage_mb: None,
                    has_vision: None,
                    tool_tags: None,
                    gpu_layers: None,
                    block_count: None,
                    system_prompt_tokens: if sys_tokens > 0 { Some(sys_tokens) } else { None },
                    tool_definitions_tokens: if tool_tokens > 0 { Some(tool_tokens) } else { None },
                    context_size: None,
                    last_finish_reason: last_finish_reason.clone(),
                    supports_thinking: None,
                    is_agent_model: if is_agent_model { Some(true) } else { None },
                }
            },
        };

        // Merge remote provider generation state if local model isn't generating
        let status = if !is_generating {
            if let Some(remote) = crate::providers::get_remote_generation() {
                let mut s = status;
                s.generating = Some(true);
                s.active_conversation_id = Some(remote.conversation_id);
                s.status_message = remote.status_message;
                s
            } else {
                status
            }
        } else {
            status
        };

        let response_json = serialize_with_fallback(&status, &default_model_status_json());
        Ok(json_raw(StatusCode::OK, response_json))
    }

    #[cfg(feature = "mock")]
    {
        let _ = &db;
        Ok(json_raw(
            StatusCode::OK,
            default_model_status_json(),
        ))
    }
}


