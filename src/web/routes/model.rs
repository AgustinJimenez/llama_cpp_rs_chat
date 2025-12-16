// Model route handlers

use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;
use std::fs;
use std::io::BufReader;
use serde::Deserialize;
use gguf_llms::{GgufHeader, GgufReader};

use crate::web::{
    models::{ModelLoadRequest, ModelResponse},
    config::{load_config, add_to_model_history},
    gguf_utils::{
        value_to_display_string,
        detect_tool_format, extract_default_system_prompt, MetadataExtractor,
    },
    filename_patterns::{detect_architecture, detect_parameters, detect_quantization},
};

#[cfg(not(feature = "mock"))]
use crate::web::{
    models::SharedLlamaState,
    model_manager::{load_model, unload_model, get_model_status},
};

pub async fn handle_get_model_info(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))]
    _llama_state: SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    println!("[DEBUG] /api/model/info endpoint hit");

    // Extract model path from query parameters
    let query = req.uri().query().unwrap_or("");
    println!("[DEBUG] Query string: {}", query);

    let mut model_path = "";

    for param in query.split('&') {
        if let Some((key, value)) = param.split_once('=') {
            if key == "path" {
                // URL decode the path
                model_path = value;
                println!("[DEBUG] Found path parameter (encoded): {}", model_path);
                break;
            }
        }
    }

    if model_path.is_empty() {
        println!("[DEBUG] ERROR: No path parameter provided");
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("content-type", "application/json")
            .header("access-control-allow-origin", "*")
            .body(Body::from(r#"{"error":"Model path is required"}"#))
            .unwrap());
    }

    // URL decode the path properly
    let decoded_path = urlencoding::decode(model_path).unwrap_or(std::borrow::Cow::Borrowed(model_path));
    println!("[DEBUG] Decoded path: {}", decoded_path);

    // Check if file exists
    let path_obj = std::path::Path::new(&*decoded_path);
    let exists = path_obj.exists();
    println!("[DEBUG] File exists: {}", exists);
    println!("[DEBUG] Path is file: {}", path_obj.is_file());
    println!("[DEBUG] Path is dir: {}", path_obj.is_dir());

    if !exists {
        println!("[DEBUG] ERROR: File does not exist at path: {}", decoded_path);
        return Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("content-type", "application/json")
            .header("access-control-allow-origin", "*")
            .body(Body::from(r#"{"error":"Model file not found"}"#))
            .unwrap());
    }

    // Check if path is a directory
    if path_obj.is_dir() {
        println!("[DEBUG] Path is a directory, scanning for .gguf files...");

        // Find all .gguf files in the directory
        let mut gguf_files = Vec::new();
        if let Ok(entries) = fs::read_dir(path_obj) {
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

        println!("[DEBUG] Returning directory error with {} suggestions", gguf_files.len());
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("content-type", "application/json")
            .header("access-control-allow-origin", "*")
            .body(Body::from(response_json.to_string()))
            .unwrap());
    }

    // Check if file has .gguf extension
    if let Some(ext) = path_obj.extension() {
        if !ext.eq_ignore_ascii_case("gguf") {
            println!("[DEBUG] ERROR: File is not a .gguf file");
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"error":"File must have .gguf extension"}"#))
                .unwrap());
        }
    } else {
        println!("[DEBUG] ERROR: File has no extension");
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("content-type", "application/json")
            .header("access-control-allow-origin", "*")
            .body(Body::from(r#"{"error":"File must have .gguf extension"}"#))
            .unwrap());
    }

    // Extract basic model information
    let file_metadata = match fs::metadata(&*decoded_path) {
        Ok(metadata) => metadata,
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"error":"Failed to read file metadata"}"#))
                .unwrap());
        }
    };

    let file_size_bytes = file_metadata.len();
    let file_size = if file_size_bytes >= 1_073_741_824 {
        format!("{:.1} GB", file_size_bytes as f64 / 1_073_741_824.0)
    } else if file_size_bytes >= 1_048_576 {
        format!("{:.1} MB", file_size_bytes as f64 / 1_048_576.0)
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
    let model_size_gb = file_size_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    let estimated_total_layers = if model_size_gb < 8.0 {
        36  // Small models (7B and below)
    } else if model_size_gb < 15.0 {
        45  // Medium models (13B)
    } else if model_size_gb < 25.0 {
        60  // Large models (30B)
    } else {
        80  // Very large models (70B+)
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
    if let Ok(file) = fs::File::open(&*decoded_path) {
        let mut reader = BufReader::new(file);

        if let Ok(header) = GgufHeader::parse(&mut reader) {
            if let Ok(metadata) = GgufReader::read_metadata(&mut reader, header.n_kv) {
                // Use shared MetadataExtractor for cleaner access
                let extractor = MetadataExtractor::new(&metadata);

                // Debug: Print all available metadata keys and values
                println!("=== GGUF Metadata Found ===");
                for (key, value) in metadata.iter() {
                    println!("  {} = {}", key, value_to_display_string(value));
                }
                println!("================================");

                // Create a metadata object with all values using shared utility
                model_info["gguf_metadata"] = serde_json::json!(extractor.to_json_map());

                // Get architecture
                let arch = extractor.get_string("general.architecture")
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
            }
        }
    }

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .header("access-control-allow-origin", "*")
        .body(Body::from(model_info.to_string()))
        .unwrap())
}

pub async fn handle_get_model_status(
    #[cfg(not(feature = "mock"))]
    llama_state: SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    #[cfg(not(feature = "mock"))]
    {
        let status = get_model_status(&llama_state);
        let response_json = match serde_json::to_string(&status) {
            Ok(json) => json,
            Err(_) => r#"{"loaded":false,"model_path":null,"last_used":null,"memory_usage_mb":null}"#.to_string(),
        };

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .header("access-control-allow-origin", "*")
            .body(Body::from(response_json))
            .unwrap())
    }

    #[cfg(feature = "mock")]
    {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .header("access-control-allow-origin", "*")
            .body(Body::from(r#"{"loaded":false,"model_path":null,"last_used":null,"memory_usage_mb":null}"#))
            .unwrap())
    }
}

pub async fn handle_get_model_history(
    #[cfg(not(feature = "mock"))]
    _llama_state: SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    // Load config and return model history
    let config = load_config();
    let response_json = match serde_json::to_string(&config.model_history) {
        Ok(json) => json,
        Err(_) => "[]".to_string(),
    };

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .header("access-control-allow-origin", "*")
        .body(Body::from(response_json))
        .unwrap())
}

pub async fn handle_post_model_history(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))]
    _llama_state: SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    // Add a model path to history
    let body_bytes = match hyper::body::to_bytes(req.into_body()).await {
        Ok(bytes) => bytes,
        Err(e) => {
            println!("[DEBUG] Failed to read request body: {}", e);
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"success":false,"message":"Failed to read request body"}"#))
                .unwrap());
        }
    };

    #[derive(Deserialize)]
    struct AddHistoryRequest {
        model_path: String,
    }

    let request: AddHistoryRequest = match serde_json::from_slice(&body_bytes) {
        Ok(req) => req,
        Err(e) => {
            println!("[DEBUG] JSON parsing error: {}", e);
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"success":false,"message":"Invalid JSON format"}"#))
                .unwrap());
        }
    };

    // Add to history
    add_to_model_history(&request.model_path);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .header("access-control-allow-origin", "*")
        .body(Body::from(r#"{"success":true}"#))
        .unwrap())
}

pub async fn handle_post_model_load(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))]
    llama_state: SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    println!("[DEBUG] /api/model/load endpoint hit");

    #[cfg(not(feature = "mock"))]
    {
        // Parse request body
        let body_bytes = match hyper::body::to_bytes(req.into_body()).await {
            Ok(bytes) => bytes,
            Err(e) => {
                println!("[DEBUG] Failed to read request body: {}", e);
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(r#"{"success":false,"message":"Failed to read request body"}"#))
                    .unwrap());
            }
        };

        println!("[DEBUG] Request body: {}", String::from_utf8_lossy(&body_bytes));

        let load_request: ModelLoadRequest = match serde_json::from_slice(&body_bytes) {
            Ok(req) => req,
            Err(e) => {
                println!("[DEBUG] JSON parsing error in model/load: {}", e);
                println!("[DEBUG] Raw body was: {}", String::from_utf8_lossy(&body_bytes));
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(r#"{"success":false,"message":"Invalid JSON format"}"#))
                    .unwrap());
            }
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

                let response_json = match serde_json::to_string(&response) {
                    Ok(json) => json,
                    Err(_) => r#"{"success":true,"message":"Model loaded successfully","status":null}"#.to_string(),
                };

                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(response_json))
                    .unwrap())
            }
            Err(e) => {
                let response = ModelResponse {
                    success: false,
                    message: format!("Failed to load model: {}", e),
                    status: None,
                };

                let response_json = match serde_json::to_string(&response) {
                    Ok(json) => json,
                    Err(_) => format!(r#"{{"success":false,"message":"Failed to load model: {}","status":null}}"#, e),
                };

                Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(response_json))
                    .unwrap())
            }
        }
    }

    #[cfg(feature = "mock")]
    {
        let _ = req;
        Ok(Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header("content-type", "application/json")
            .header("access-control-allow-origin", "*")
            .body(Body::from(r#"{"success":false,"message":"Model loading not available (mock feature enabled)"}"#))
            .unwrap())
    }
}

pub async fn handle_post_model_unload(
    #[cfg(not(feature = "mock"))]
    llama_state: SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
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

                let response_json = match serde_json::to_string(&response) {
                    Ok(json) => json,
                    Err(_) => r#"{"success":true,"message":"Model unloaded successfully","status":null}"#.to_string(),
                };

                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(response_json))
                    .unwrap())
            }
            Err(e) => {
                let response = ModelResponse {
                    success: false,
                    message: format!("Failed to unload model: {}", e),
                    status: None,
                };

                let response_json = match serde_json::to_string(&response) {
                    Ok(json) => json,
                    Err(_) => format!(r#"{{"success":false,"message":"Failed to unload model: {}","status":null}}"#, e),
                };

                Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(response_json))
                    .unwrap())
            }
        }
    }

    #[cfg(feature = "mock")]
    {
        Ok(Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header("content-type", "application/json")
            .header("access-control-allow-origin", "*")
            .body(Body::from(r#"{"success":false,"message":"Model unloading not available (mock feature enabled)"}"#))
            .unwrap())
    }
}
