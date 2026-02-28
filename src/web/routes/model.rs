// Model route handlers

use gguf_llms::{GgufHeader, GgufReader};
use hyper::{Body, Request, Response, StatusCode};
use serde::Deserialize;
use std::convert::Infallible;
use std::fs;
use std::io::BufReader;
use tokio::task::spawn_blocking;

use crate::web::{
    chat::get_tool_tags_for_model,
    config::add_to_model_history,
    database::SharedDatabase,
    filename_patterns::{detect_architecture, detect_parameters, detect_quantization},
    gguf_utils::{
        detect_tool_format, extract_default_system_prompt, value_to_display_string,
        MetadataExtractor,
    },
    models::{ModelLoadRequest, ModelResponse},
    request_parsing::parse_json_body,
    response_helpers::{json_error, json_raw, serialize_with_fallback},
};

// Import logging macros
use crate::{sys_debug, sys_error};

#[cfg(not(feature = "mock"))]
use crate::web::worker::worker_bridge::SharedWorkerBridge;

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
) -> Result<Response<Body>, Infallible> {
    #[cfg(not(feature = "mock"))]
    {
        // Get model status from worker bridge cached metadata (no IPC round-trip)
        let status = match bridge.model_status().await {
            Some(meta) => {
                let tags = if meta.loaded {
                    Some(get_tool_tags_for_model(meta.general_name.as_deref()))
                } else {
                    None
                };
                crate::web::models::ModelStatus {
                    loaded: meta.loaded,
                    model_path: Some(meta.model_path),
                    last_used: None,
                    memory_usage_mb: if meta.loaded { Some(512) } else { None },
                    has_vision: Some(meta.has_vision),
                    tool_tags: tags,
                }
            }
            None => crate::web::models::ModelStatus {
                loaded: false,
                model_path: None,
                last_used: None,
                memory_usage_mb: None,
                has_vision: None,
                tool_tags: None,
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

        // Attempt to load the model via worker process
        match bridge.load_model(&load_request.model_path, load_request.gpu_layers).await {
            Ok(meta) => {
                // Add to model history on successful load
                add_to_model_history(&db, &load_request.model_path);

                let tags = Some(get_tool_tags_for_model(meta.general_name.as_deref()));
                let status = crate::web::models::ModelStatus {
                    loaded: true,
                    model_path: Some(meta.model_path),
                    last_used: None,
                    memory_usage_mb: Some(512),
                    has_vision: Some(meta.has_vision),
                    tool_tags: tags,
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
                let status = crate::web::models::ModelStatus {
                    loaded: false,
                    model_path: None,
                    last_used: None,
                    memory_usage_mb: None,
                    has_vision: None,
                    tool_tags: None,
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
