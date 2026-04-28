// Model route handlers

use gguf_llms::{GgufHeader, GgufReader};
use hyper::{Body, Request, Response, StatusCode};
use serde::Deserialize;
use std::convert::Infallible;
use std::fs;
use std::io::BufReader;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio::task::spawn_blocking;

use llama_chat_engine::{get_tool_tags_for_model, tool_tags::get_tag_pairs_for_model};
use llama_chat_config::add_to_model_history;
use llama_chat_db::SharedDatabase;
use llama_chat_engine::filename_patterns::{detect_architecture, detect_parameters, detect_quantization};
use llama_chat_engine::gguf_utils::{
    detect_tool_format, extract_default_system_prompt, value_to_display_string,
    MetadataExtractor,
};
use llama_chat_types::models::{ModelLoadRequest, ModelResponse};
use crate::request_parsing::parse_json_body;
use crate::response_helpers::{json_error, json_raw, serialize_with_fallback};

#[cfg(not(feature = "mock"))]
use llama_chat_worker::worker::worker_bridge::SharedWorkerBridge;

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

fn default_model_status_json() -> String {
    r#"{"loaded":false,"model_path":null,"last_used":null,"memory_usage_mb":null}"#.to_string()
}

/// Scan a directory for .gguf files and return their filenames
fn scan_directory_for_gguf_files(path: &std::path::Path) -> Vec<String> {
    let mut gguf_files = Vec::new();
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_file() {
                if let Some(ext) = entry_path.extension() {
                    if ext.eq_ignore_ascii_case("gguf") {
                        if let Some(filename) = entry_path.file_name().and_then(|n| n.to_str()) {
                            gguf_files.push(filename.to_string());
                        }
                    }
                }
            }
        }
    }
    gguf_files
}

/// Scan for mmproj (multimodal projection) GGUF files in the same directory as the model.
/// Returns a vec of (filename, file_size_bytes) for each mmproj file found.
/// Vision-capable models in llama.cpp always require a separate mmproj companion file.
fn scan_for_mmproj_files(model_path: &std::path::Path) -> Vec<(String, u64)> {
    let dir = match model_path.parent() {
        Some(d) => d,
        None => return Vec::new(),
    };
    let mut results = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if !entry_path.is_file() {
                continue;
            }
            let ext_ok = entry_path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("gguf"))
                .unwrap_or(false);
            if !ext_ok {
                continue;
            }
            if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
                let lower = name.to_ascii_lowercase();
                if lower.contains("mmproj") {
                    let size = fs::metadata(&entry_path).map(|m| m.len()).unwrap_or(0);
                    results.push((name.to_string(), size));
                }
            }
        }
    }
    results
}

/// Populate a model_info JSON object with fields extracted from GGUF metadata.
fn enrich_model_info_from_gguf(
    model_info: &mut serde_json::Value,
    extractor: &MetadataExtractor,
) {
    model_info["gguf_metadata"] = serde_json::json!(extractor.to_json_map());

    let arch = extractor
        .get_string("general.architecture")
        .unwrap_or_else(|| "llama".to_string());
    model_info["architecture"] = serde_json::json!(arch.clone());

    // Tool calling format + tags
    let model_name = extractor.get_string("general.name").unwrap_or_default();
    model_info["tool_format"] = serde_json::json!(detect_tool_format(&arch, &model_name));
    let detected_tags = get_tool_tags_for_model(Some(&model_name));
    model_info["detected_tool_tags"] = serde_json::json!({
        "exec_open": detected_tags.exec_open,
        "exec_close": detected_tags.exec_close,
        "output_open": detected_tags.output_open,
        "output_close": detected_tags.output_close,
    });
    model_info["detected_tag_pairs"] = serde_json::json!(get_tag_pairs_for_model(Some(&model_name)));

    // Core model information
    let string_fields = [
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
        ("tokenizer.ggml.model", "tokenizer_model"),
        ("tokenizer.ggml.bos_token_id", "bos_token_id"),
        ("tokenizer.ggml.eos_token_id", "eos_token_id"),
        ("tokenizer.ggml.padding_token_id", "padding_token_id"),
    ];
    for (key, field) in &string_fields {
        if let Some(val) = extractor.get_string(key) {
            model_info[*field] = serde_json::json!(val);
        }
    }

    // Context length - try arch-specific, then fallbacks
    for key in &[
        format!("{arch}.context_length"),
        "llama.context_length".to_string(),
        "context_length".to_string(),
    ] {
        if let Some(val) = extractor.get_string(key) {
            model_info["context_length"] = serde_json::json!(val);
            break;
        }
    }

    // Architecture-specific fields
    let arch_fields = [
        ("embedding_length", "embedding_length"),
        ("feed_forward_length", "feed_forward_length"),
        ("attention.head_count", "attention_head_count"),
        ("attention.head_count_kv", "attention_head_count_kv"),
        ("attention.layer_norm_rms_epsilon", "layer_norm_epsilon"),
        ("rope.dimension_count", "rope_dimension_count"),
        ("rope.freq_base", "rope_freq_base"),
    ];
    for (suffix, field) in &arch_fields {
        if let Some(val) = extractor.get_arch_field(&arch, suffix) {
            model_info[*field] = serde_json::json!(val);
        }
    }
    // Block count also updates estimated_layers
    if let Some(val) = extractor.get_arch_field(&arch, "block_count") {
        model_info["block_count"] = serde_json::json!(val.clone());
        if let Ok(count) = val.parse::<u32>() {
            model_info["estimated_layers"] = serde_json::json!(count);
        }
    }

    // Chat template + default system prompt
    if let Some(val) = extractor.get_string("tokenizer.chat_template") {
        model_info["chat_template"] = serde_json::json!(val.clone());
        if let Some(prompt) = extract_default_system_prompt(&val) {
            model_info["default_system_prompt"] = serde_json::json!(prompt);
        }
    }

    // GGUF embedded sampling parameters
    let sampling_fields = [
        ("general.sampling.temp", "temperature"),
        ("general.sampling.top_p", "top_p"),
        ("general.sampling.top_k", "top_k"),
        ("general.sampling.min_p", "min_p"),
        ("general.sampling.repetition_penalty", "repetition_penalty"),
    ];
    let mut recommended_params = serde_json::Map::new();
    for (key, name) in &sampling_fields {
        if let Some(val) = extractor.get_json(key) {
            recommended_params.insert((*name).to_string(), val);
        }
    }
    if !recommended_params.is_empty() {
        model_info["recommended_params"] = serde_json::json!(recommended_params);
    }
}

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
    #[cfg(not(feature = "mock"))] bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    #[cfg(not(feature = "mock"))]
    {
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
        let status = match bridge.model_status().await {
            Some(meta) => {
                let tags = if meta.loaded {
                    Some(get_tool_tags_for_model(meta.general_name.as_deref()))
                } else {
                    None
                };
                let lp = if is_loading { Some(bridge.loading_progress()) } else { None };
                // Get effective context size: config override or model's native context length
                let config = llama_chat_config::load_config(&db);
                let context_size = config.context_size.or(meta.context_length);
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
                }
            }
            None => {
                let loading_path = bridge.loading_path().await;
                let lp = if is_loading { Some(bridge.loading_progress()) } else { None };
                llama_chat_types::models::ModelStatus {
                    loaded: false,
                    loading: if is_loading { Some(true) } else { None },
                    loading_progress: lp,
                    generating: if is_generating { Some(true) } else { None },
                    active_conversation_id: active_conv_id.clone(), status_message: status_msg.clone(),
                    model_path: loading_path,
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
                }
            },
        };

        let response_json = serialize_with_fallback(&status, &default_model_status_json());
        Ok(json_raw(StatusCode::OK, response_json))
    }

    #[cfg(feature = "mock")]
    {
        Ok(json_raw(
            StatusCode::OK,
            default_model_status_json(),
        ))
    }
}

pub async fn handle_get_model_history(
    #[cfg(not(feature = "mock"))] _bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let history = db.get_model_history().unwrap_or_default();
    let response_json = serialize_with_fallback(&history, "[]");

    Ok(json_raw(StatusCode::OK, response_json))
}

pub async fn handle_post_model_history(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] _bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    #[derive(Deserialize)]
    struct AddHistoryRequest {
        model_path: String,
    }

    // Parse request body using helper
    let request: AddHistoryRequest = match parse_json_body(req.into_body()).await {
        Ok(req) => req,
        Err(error_response) => return Ok(error_response),
    };

    // Add to history
    add_to_model_history(&db, &request.model_path);

    Ok(json_raw(StatusCode::OK, r#"{"success":true}"#.to_string()))
}

pub async fn handle_post_model_load(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    sys_debug!("[DEBUG] /api/model/load endpoint hit");

    #[cfg(not(feature = "mock"))]
    {
        // Parse request body using helper
        let load_request: ModelLoadRequest = match parse_json_body(req.into_body()).await {
            Ok(req) => req,
            Err(error_response) => return Ok(error_response),
        };

        // Persist context/cache params to global config if provided in the load request.
        // This ensures the Conversation Config sidebar shows the correct values.
        {
            let mut db_config = db.load_config();
            let mut updated = false;
            if let Some(ctx) = load_request.context_size {
                db_config.context_size = Some(ctx);
                updated = true;
            }
            if let Some(fa) = load_request.flash_attention {
                db_config.flash_attention = fa;
                updated = true;
            }
            if let Some(ref k) = load_request.cache_type_k {
                db_config.cache_type_k = k.clone();
                updated = true;
            }
            if let Some(ref v) = load_request.cache_type_v {
                db_config.cache_type_v = v.clone();
                updated = true;
            }
            if updated {
                if let Err(e) = db.update_config(&db_config) {
                    eprintln!("[WARN] Failed to persist load params to config: {}", e);
                }
            }
        }

        // Attempt to load the model via worker process
        match bridge.load_model(&load_request.model_path, load_request.gpu_layers, load_request.mmproj_path).await {
            Ok(meta) => {
                // Add to model history on successful load
                add_to_model_history(&db, &load_request.model_path);

                let tags = Some(get_tool_tags_for_model(meta.general_name.as_deref()));
                let status = llama_chat_types::models::ModelStatus {
                    loaded: true,
                    loading: None,
                    loading_progress: None,
                    generating: None,
                    active_conversation_id: None, status_message: None,
                    model_path: Some(meta.model_path),
                    last_used: None,
                    memory_usage_mb: Some(512),
                    has_vision: Some(meta.has_vision),
                    tool_tags: tags,
                    gpu_layers: meta.gpu_layers,
                    block_count: meta.block_count,
                    system_prompt_tokens: None,
                    tool_definitions_tokens: None,
                    context_size: None,
                    last_finish_reason: None,
                };
                let response = ModelResponse {
                    success: true,
                    message: format!("Model loaded successfully from {}", load_request.model_path),
                    status: Some(status),
                };

                let response_json = serialize_with_fallback(
                    &response,
                    r#"{"success":true,"message":"Model loaded successfully","status":null}"#,
                );

                Ok(json_raw(StatusCode::OK, response_json))
            }
            Err(e) => {
                let response = ModelResponse {
                    success: false,
                    message: format!("Failed to load model: {e}"),
                    status: None,
                };

                let response_json = serialize_with_fallback(
                    &response,
                    &format!(
                        r#"{{"success":false,"message":"Failed to load model: {e}","status":null}}"#
                    ),
                );

                Ok(json_raw(StatusCode::INTERNAL_SERVER_ERROR, response_json))
            }
        }
    }

    #[cfg(feature = "mock")]
    {
        let _ = req;
        Ok(json_raw(
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"success":false,"message":"Model loading not available (mock feature enabled)"}"#
                .to_string(),
        ))
    }
}

pub async fn handle_post_model_unload(
    #[cfg(not(feature = "mock"))] bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
) -> Result<Response<Body>, Infallible> {
    #[cfg(not(feature = "mock"))]
    {
        // Always use hard-unload (kill worker) to guarantee CUDA memory is reclaimed.
        // Soft unload frees Rust objects but CUDA memory pools stay reserved.
        match bridge.force_unload().await {
            Ok(_) => {
                let status = llama_chat_types::models::ModelStatus {
                    loaded: false,
                    loading: None,
                    loading_progress: None,
                    generating: None,
                    active_conversation_id: None, status_message: None,
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
                };
                let response = ModelResponse {
                    success: true,
                    message: "Model unloaded successfully".to_string(),
                    status: Some(status),
                };
                let response_json = serialize_with_fallback(
                    &response,
                    r#"{"success":true,"message":"Model unloaded successfully","status":null}"#,
                );
                Ok(json_raw(StatusCode::OK, response_json))
            }
            Err(e) => {
                let response = ModelResponse {
                    success: false,
                    message: format!("Failed to unload model: {e}"),
                    status: None,
                };
                let response_json = serialize_with_fallback(
                    &response,
                    &format!(
                        r#"{{"success":false,"message":"Failed to unload model: {e}","status":null}}"#
                    ),
                );
                Ok(json_raw(StatusCode::INTERNAL_SERVER_ERROR, response_json))
            }
        }
    }

    #[cfg(feature = "mock")]
    {
        Ok(json_raw(
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"success":false,"message":"Model unloading not available (mock feature enabled)"}"#
                .to_string(),
        ))
    }
}

/// Force-kill the worker process, instantly reclaiming all VRAM and RAM.
/// Automatically restarts a fresh worker.
pub async fn handle_post_model_hard_unload(
    #[cfg(not(feature = "mock"))] bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
) -> Result<Response<Body>, Infallible> {
    #[cfg(not(feature = "mock"))]
    {
        match bridge.force_unload().await {
            Ok(_) => Ok(json_raw(
                StatusCode::OK,
                r#"{"success":true,"message":"Worker process killed, memory reclaimed"}"#
                    .to_string(),
            )),
            Err(e) => Ok(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to force unload: {e}"),
            )),
        }
    }

    #[cfg(feature = "mock")]
    {
        Ok(json_raw(
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"success":false,"message":"Force unload not available (mock feature enabled)"}"#
                .to_string(),
        ))
    }
}

fn detect_nvidia_gpu_hardware() -> bool {
    #[cfg(target_os = "windows")]
    {
        std::path::Path::new("C:\\Windows\\System32\\nvcuda.dll").exists()
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("nvidia-smi")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// GET /api/backends — list available compute backends (CUDA, Vulkan, CPU, etc.)
pub async fn handle_get_backends(
    #[cfg(not(feature = "mock"))] bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
) -> Result<Response<Body>, Infallible> {
    let nvidia_detected = detect_nvidia_gpu_hardware();

    #[cfg(not(feature = "mock"))]
    {
        match bridge.get_available_backends().await {
            Ok(backends) => {
                let has_cuda = backends.iter().any(|b| b.name == "CUDA" && b.available);
                let body = serde_json::json!({
                    "backends": backends,
                    "nvidia_gpu_detected": nvidia_detected,
                    "cuda_backend_loaded": has_cuda,
                });
                Ok(json_raw(StatusCode::OK, serde_json::to_string(&body).unwrap()))
            }
            Err(e) => Ok(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to get backends: {e}"),
            )),
        }
    }

    #[cfg(feature = "mock")]
    {
        let body = serde_json::json!({
            "backends": [{"name":"CPU","available":true,"devices":[{"name":"CPU","description":"CPU"}]}],
            "nvidia_gpu_detected": nvidia_detected,
            "cuda_backend_loaded": false,
        });
        Ok(json_raw(StatusCode::OK, serde_json::to_string(&body).unwrap()))
    }
}

// ─── GPU Backend Auto-Install ──────────────────────────────────────────────

/// GitHub release URL for pre-built GPU backend DLLs.
/// The release should contain a zip with ggml-cuda.dll (and any deps).
const GPU_BACKEND_RELEASE_URL: &str =
    "https://github.com/AgustinJimenez/llama_cpp_rs_chat/releases/download/backends/ggml-cuda.dll";

/// POST /api/backends/install — download GPU backend DLLs to the app directory.
/// Returns SSE stream with progress events.
pub async fn handle_post_backends_install() -> Result<Response<Body>, Infallible> {
    // Determine the app's exe directory (where DLLs should be placed)
    let exe_dir = match std::env::current_exe() {
        Ok(exe) => exe.parent().unwrap_or_else(|| std::path::Path::new(".")).to_path_buf(),
        Err(e) => {
            return Ok(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Cannot determine app directory: {e}"),
            ));
        }
    };

    let dest_file = exe_dir.join("ggml-cuda.dll");

    // If already installed, return immediately
    if dest_file.exists() {
        let size = std::fs::metadata(&dest_file).map(|m| m.len()).unwrap_or(0);
        let done = serde_json::json!({ "type": "done", "path": dest_file.to_string_lossy(), "bytes": size });
        let sse = format!("data: {done}\n\n");
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .header("cache-control", "no-cache")
            .header("access-control-allow-origin", "*")
            .body(Body::from(sse))
            .unwrap());
    }

    let (mut sender, body) = Body::channel();
    let (progress_tx, mut progress_rx) = mpsc::channel::<String>(64);

    // Download in blocking thread
    spawn_blocking(move || {
        download_backend_blocking(GPU_BACKEND_RELEASE_URL, &dest_file, progress_tx);
    });

    // Forward SSE events
    tokio::spawn(async move {
        while let Some(event) = progress_rx.recv().await {
            let sse = format!("data: {event}\n\n");
            if sender.send_data(hyper::body::Bytes::from(sse)).await.is_err() {
                break;
            }
        }
    });

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("access-control-allow-origin", "*")
        .body(body)
        .unwrap())
}

/// Blocking download of a single file with progress events.
fn download_backend_blocking(url: &str, dest: &PathBuf, tx: mpsc::Sender<String>) {
    use std::io::Write;

    let send = |json: serde_json::Value| {
        let _ = tx.blocking_send(json.to_string());
    };

    // Follow redirects (GitHub releases redirect to CDN)
    let agent = ureq::AgentBuilder::new()
        .redirects(10)
        .timeout(std::time::Duration::from_secs(600))
        .build();

    let resp = match agent.get(url).call() {
        Ok(r) => r,
        Err(e) => {
            send(serde_json::json!({ "type": "error", "message": format!("Download failed: {e}") }));
            return;
        }
    };

    let total = resp
        .header("content-length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    let part_file = dest.with_extension("dll.part");
    let mut file = match std::fs::File::create(&part_file) {
        Ok(f) => f,
        Err(e) => {
            send(serde_json::json!({ "type": "error", "message": format!("Cannot create file: {e}") }));
            return;
        }
    };

    let mut reader = resp.into_reader();
    let mut buf = [0u8; 65536];
    let mut downloaded: u64 = 0;
    let mut last_report = std::time::Instant::now();

    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if file.write_all(&buf[..n]).is_err() {
                    send(serde_json::json!({ "type": "error", "message": "Write error" }));
                    return;
                }
                downloaded += n as u64;
                if last_report.elapsed().as_millis() >= 200 {
                    send(serde_json::json!({
                        "type": "progress",
                        "bytes": downloaded,
                        "total": total,
                        "percent": if total > 0 { (downloaded * 100 / total) as u32 } else { 0 },
                    }));
                    last_report = std::time::Instant::now();
                }
            }
            Err(e) => {
                send(serde_json::json!({ "type": "error", "message": format!("Read error: {e}") }));
                return;
            }
        }
    }

    // Rename .part to final
    if let Err(e) = std::fs::rename(&part_file, dest) {
        send(serde_json::json!({ "type": "error", "message": format!("Rename failed: {e}") }));
        return;
    }

    send(serde_json::json!({
        "type": "done",
        "path": dest.to_string_lossy(),
        "bytes": downloaded,
    }));
}
