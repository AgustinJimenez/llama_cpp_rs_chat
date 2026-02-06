// Model route handlers

use gguf_llms::{GgufHeader, GgufReader};
use hyper::{Body, Request, Response, StatusCode};
use serde::Deserialize;
use std::convert::Infallible;
use std::fs;
use std::io::BufReader;
use std::sync::{Mutex as StdMutex, OnceLock, TryLockError};
use tokio::task::spawn_blocking;

use crate::web::{
    config::{add_to_model_history, load_config},
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
use crate::web::{
    model_manager::{get_model_status, load_model, unload_model},
    models::SharedLlamaState,
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

static MODEL_STATUS_CACHE_JSON: OnceLock<StdMutex<String>> = OnceLock::new();

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

pub async fn handle_get_model_info(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] _llama_state: SharedLlamaState,
    #[cfg(feature = "mock")] _llama_state: (),
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
        format!("{} bytes", file_size_bytes)
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
        // Use shared MetadataExtractor for cleaner access
        let extractor = MetadataExtractor::new(&metadata);

        let log_metadata = std::env::var("GGUF_DEBUG").unwrap_or_default() == "1";
        if log_metadata {
            // Debug: Print all available metadata keys and values
            sys_debug!("=== GGUF Metadata Found ===");
            for (key, value) in metadata.iter() {
                sys_debug!("  {} = {}", key, value_to_display_string(value));
            }
            sys_debug!("================================");
        }

        // Create a metadata object with all values using shared utility
        model_info["gguf_metadata"] = serde_json::json!(extractor.to_json_map());

        // Get architecture
        let arch = extractor
            .get_string("general.architecture")
            .unwrap_or_else(|| "llama".to_string());

        // Update architecture
        model_info["architecture"] = serde_json::json!(arch.clone());

        // Detect tool calling format using shared utility
        let model_name = extractor.get_string("general.name").unwrap_or_default();
        let tool_format = detect_tool_format(&arch, &model_name);
        model_info["tool_format"] = serde_json::json!(tool_format);

        // Core model information
        if let Some(val) = extractor.get_string("general.name") {
            model_info["general_name"] = serde_json::json!(val);
        }
        if let Some(val) = extractor.get_string("general.author") {
            model_info["author"] = serde_json::json!(val);
        }
        if let Some(val) = extractor.get_string("general.version") {
            model_info["version"] = serde_json::json!(val);
        }
        if let Some(val) = extractor.get_string("general.organization") {
            model_info["organization"] = serde_json::json!(val);
        }
        if let Some(val) = extractor.get_string("general.description") {
            model_info["description"] = serde_json::json!(val);
        }
        if let Some(val) = extractor.get_string("general.license") {
            model_info["license"] = serde_json::json!(val);
        }
        if let Some(val) = extractor.get_string("general.url") {
            model_info["url"] = serde_json::json!(val);
        }
        if let Some(val) = extractor.get_string("general.repo_url") {
            model_info["repo_url"] = serde_json::json!(val);
        }
        if let Some(val) = extractor.get_string("general.file_type") {
            model_info["file_type"] = serde_json::json!(val);
        }
        if let Some(val) = extractor.get_string("general.quantization_version") {
            model_info["quantization_version"] = serde_json::json!(val);
        }

        // Context length - try multiple keys
        let context_keys = vec![
            format!("{}.context_length", arch),
            "llama.context_length".to_string(),
            "context_length".to_string(),
        ];
        for key in &context_keys {
            if let Some(val) = extractor.get_string(&key) {
                model_info["context_length"] = serde_json::json!(val);
                break;
            }
        }

        // Architecture-specific fields using extractor helper
        if let Some(val) = extractor.get_arch_field(&arch, "embedding_length") {
            model_info["embedding_length"] = serde_json::json!(val);
        }
        if let Some(val) = extractor.get_arch_field(&arch, "block_count") {
            model_info["block_count"] = serde_json::json!(val.clone());
            // Use actual block count for layers
            if let Ok(block_count) = val.parse::<u32>() {
                model_info["estimated_layers"] = serde_json::json!(block_count);
            }
        }
        if let Some(val) = extractor.get_arch_field(&arch, "feed_forward_length") {
            model_info["feed_forward_length"] = serde_json::json!(val);
        }
        if let Some(val) = extractor.get_arch_field(&arch, "attention.head_count") {
            model_info["attention_head_count"] = serde_json::json!(val);
        }
        if let Some(val) = extractor.get_arch_field(&arch, "attention.head_count_kv") {
            model_info["attention_head_count_kv"] = serde_json::json!(val);
        }
        if let Some(val) = extractor.get_arch_field(&arch, "attention.layer_norm_rms_epsilon") {
            model_info["layer_norm_epsilon"] = serde_json::json!(val);
        }
        if let Some(val) = extractor.get_arch_field(&arch, "rope.dimension_count") {
            model_info["rope_dimension_count"] = serde_json::json!(val);
        }
        if let Some(val) = extractor.get_arch_field(&arch, "rope.freq_base") {
            model_info["rope_freq_base"] = serde_json::json!(val);
        }

        // Tokenizer information
        if let Some(val) = extractor.get_string("tokenizer.ggml.model") {
            model_info["tokenizer_model"] = serde_json::json!(val);
        }
        if let Some(val) = extractor.get_string("tokenizer.ggml.bos_token_id") {
            model_info["bos_token_id"] = serde_json::json!(val);
        }
        if let Some(val) = extractor.get_string("tokenizer.ggml.eos_token_id") {
            model_info["eos_token_id"] = serde_json::json!(val);
        }
        if let Some(val) = extractor.get_string("tokenizer.ggml.padding_token_id") {
            model_info["padding_token_id"] = serde_json::json!(val);
        }
        if let Some(val) = extractor.get_string("tokenizer.chat_template") {
            model_info["chat_template"] = serde_json::json!(val.clone());

            // Extract default system prompt using shared utility
            if let Some(prompt) = extract_default_system_prompt(&val) {
                model_info["default_system_prompt"] = serde_json::json!(prompt);
            }
        }

        // GGUF embedded sampling parameters (if present)
        // These are the model creator's recommended defaults
        let mut recommended_params = serde_json::Map::new();
        if let Some(val) = extractor.get_json("general.sampling.temp") {
            recommended_params.insert("temperature".to_string(), val);
        }
        if let Some(val) = extractor.get_json("general.sampling.top_p") {
            recommended_params.insert("top_p".to_string(), val);
        }
        if let Some(val) = extractor.get_json("general.sampling.top_k") {
            recommended_params.insert("top_k".to_string(), val);
        }
        if let Some(val) = extractor.get_json("general.sampling.min_p") {
            recommended_params.insert("min_p".to_string(), val);
        }
        if let Some(val) = extractor.get_json("general.sampling.repetition_penalty") {
            recommended_params.insert("repetition_penalty".to_string(), val);
        }
        if !recommended_params.is_empty() {
            model_info["recommended_params"] = serde_json::json!(recommended_params);
        }
    }

    Ok(json_raw(StatusCode::OK, model_info.to_string()))
}

pub async fn handle_get_model_status(
    #[cfg(not(feature = "mock"))] llama_state: SharedLlamaState,
    #[cfg(feature = "mock")] _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    #[cfg(not(feature = "mock"))]
    {
        // Avoid blocking/hanging the status endpoint if the model mutex is held by a long operation.
        // Return the last known-good status (cached) when the mutex would block.
        let cache = MODEL_STATUS_CACHE_JSON.get_or_init(|| StdMutex::new(default_model_status_json()));

        let guard = match llama_state.try_lock() {
            Ok(guard) => Some(guard),
            Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        };

        let response_json = if let Some(state_guard) = guard {
            // Compute status using the already-held guard instead of taking a second lock.
            let status = match state_guard.as_ref() {
                Some(state) => {
                    let loaded = state.model.is_some();
                    let model_path = state.current_model_path.clone();
                    let last_used = state
                        .last_used
                        .duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .map(|d| d.as_secs().to_string());

                    crate::web::models::ModelStatus {
                        loaded,
                        model_path,
                        last_used,
                        memory_usage_mb: if loaded { Some(512) } else { None },
                    }
                }
                None => crate::web::models::ModelStatus {
                    loaded: false,
                    model_path: None,
                    last_used: None,
                    memory_usage_mb: None,
                },
            };

            let json = serialize_with_fallback(&status, &default_model_status_json());
            if let Ok(mut cached) = cache.lock() {
                *cached = json.clone();
            }
            json
        } else {
            cache
                .lock()
                .map(|g| g.clone())
                .unwrap_or_else(|_| default_model_status_json())
        };

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
    #[cfg(not(feature = "mock"))] _llama_state: SharedLlamaState,
    #[cfg(feature = "mock")] _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    // Load config and return model history
    let config = load_config();
    let response_json = serialize_with_fallback(&config.model_history, "[]");

    Ok(json_raw(StatusCode::OK, response_json))
}

pub async fn handle_post_model_history(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] _llama_state: SharedLlamaState,
    #[cfg(feature = "mock")] _llama_state: (),
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
    add_to_model_history(&request.model_path);

    Ok(json_raw(StatusCode::OK, r#"{"success":true}"#.to_string()))
}

pub async fn handle_post_model_load(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] llama_state: SharedLlamaState,
    #[cfg(feature = "mock")] _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    sys_debug!("[DEBUG] /api/model/load endpoint hit");

    #[cfg(not(feature = "mock"))]
    {
        // Parse request body using helper
        let load_request: ModelLoadRequest = match parse_json_body(req.into_body()).await {
            Ok(req) => req,
            Err(error_response) => return Ok(error_response),
        };

        // Attempt to load the model
        match load_model(llama_state.clone(), &load_request.model_path).await {
            Ok(_) => {
                // Add to model history on successful load
                add_to_model_history(&load_request.model_path);

                let status = get_model_status(&llama_state);
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
                    message: format!("Failed to load model: {}", e),
                    status: None,
                };

                let response_json = serialize_with_fallback(
                    &response,
                    &format!(
                        r#"{{"success":false,"message":"Failed to load model: {}","status":null}}"#,
                        e
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
    #[cfg(not(feature = "mock"))] llama_state: SharedLlamaState,
    #[cfg(feature = "mock")] _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    #[cfg(not(feature = "mock"))]
    {
        match unload_model(llama_state.clone()).await {
            Ok(_) => {
                let status = get_model_status(&llama_state);
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
                    message: format!("Failed to unload model: {}", e),
                    status: None,
                };

                let response_json = serialize_with_fallback(
                    &response,
                    &format!(
                        r#"{{"success":false,"message":"Failed to unload model: {}","status":null}}"#,
                        e
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
