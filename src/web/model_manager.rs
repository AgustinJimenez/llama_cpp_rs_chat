use std::fs;
use std::io::BufReader;
use gguf_llms::{GgufHeader, GgufReader, Value};
use llama_cpp_2::{
    llama_backend::LlamaBackend,
    model::{params::LlamaModelParams, LlamaModel},
};

use super::models::{ModelStatus, SharedLlamaState, LlamaState};

// Helper function to get model status
pub fn get_model_status(llama_state: &SharedLlamaState) -> ModelStatus {
    match llama_state.lock() {
        Ok(state_guard) => {
            match state_guard.as_ref() {
                Some(state) => {
                    let loaded = state.model.is_some();
                    let model_path = state.current_model_path.clone();
                    let last_used = state.last_used
                        .duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .map(|d| d.as_secs().to_string());

                    ModelStatus {
                        loaded,
                        model_path,
                        last_used,
                        memory_usage_mb: if loaded { Some(512) } else { None }, // Rough estimate
                    }
                }
                None => ModelStatus {
                    loaded: false,
                    model_path: None,
                    last_used: None,
                    memory_usage_mb: None,
                }
            }
        }
        Err(_) => ModelStatus {
            loaded: false,
            model_path: None,
            last_used: None,
            memory_usage_mb: None,
        }
    }
}

// Helper function to calculate optimal GPU layers based on available VRAM
pub fn calculate_optimal_gpu_layers(model_path: &str) -> u32 {
    // Get model file size to estimate memory requirements
    let model_size_bytes = match fs::metadata(model_path) {
        Ok(metadata) => metadata.len(),
        Err(_) => {
            println!("[GPU] Could not read model file size, defaulting to 32 layers");
            return 32;
        }
    };

    let model_size_gb = model_size_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    println!("[GPU] Model file size: {:.2} GB", model_size_gb);

    // Try to get available GPU VRAM
    // For NVIDIA GPUs, we can estimate based on typical model requirements
    // A rough heuristic:
    // - Small models (< 5GB): Use all GPU layers (typically ~40 layers)
    // - Medium models (5-15GB): Use proportional layers
    // - Large models (> 15GB): May need CPU offload

    // Estimate based on RTX 4090 with ~24GB VRAM
    // Reserve ~2GB for system/context, leaving ~22GB for model
    let available_vram_gb = 22.0;

    println!("[GPU] Estimated available VRAM: {:.2} GB", available_vram_gb);

    // Calculate what percentage of the model fits in VRAM
    let vram_ratio = (available_vram_gb / model_size_gb).min(1.0);

    // Estimate typical layer count based on model size
    // Small models (~7B params, ~4-8GB): ~32-40 layers
    // Medium models (~13B params, ~8-15GB): ~40-50 layers
    // Large models (~30B+ params, >15GB): ~50-80 layers
    let estimated_total_layers = if model_size_gb < 8.0 {
        36
    } else if model_size_gb < 15.0 {
        45
    } else if model_size_gb < 25.0 {
        60
    } else {
        80
    };

    let optimal_layers = (estimated_total_layers as f64 * vram_ratio).floor() as u32;

    println!("[GPU] Estimated total layers: {}", estimated_total_layers);
    println!("[GPU] VRAM utilization ratio: {:.1}%", vram_ratio * 100.0);
    println!("[GPU] Optimal GPU layers: {} ({}% of model)",
             optimal_layers,
             (optimal_layers as f64 / estimated_total_layers as f64 * 100.0) as u32);

    // Ensure at least 1 layer on GPU if model is small enough
    optimal_layers.max(if vram_ratio > 0.1 { 1 } else { 0 })
}

// Helper function to load a model
pub async fn load_model(llama_state: SharedLlamaState, model_path: &str) -> Result<(), String> {
    println!("[DEBUG] load_model called with path: {}", model_path);

    // Handle poisoned mutex by recovering from panic
    let mut state_guard = llama_state.lock().unwrap_or_else(|poisoned| {
        println!("[DEBUG] Mutex was poisoned, recovering...");
        poisoned.into_inner()
    });

    // Initialize backend if needed
    if state_guard.is_none() {
        let backend = LlamaBackend::init().map_err(|e| format!("Failed to init backend: {}", e))?;
        *state_guard = Some(LlamaState {
            backend,
            model: None,
            current_model_path: None,
            model_context_length: None,
            chat_template_type: None,
            last_used: std::time::SystemTime::now(),
        });
    }

    let state = state_guard.as_mut().unwrap();

    // Check if model is already loaded
    if let Some(ref current_path) = state.current_model_path {
        if current_path == model_path && state.model.is_some() {
            state.last_used = std::time::SystemTime::now();
            return Ok(()); // Model already loaded
        }
    }

    // Unload current model if any
    state.model = None;
    state.current_model_path = None;

    // Calculate optimal GPU layers
    let optimal_gpu_layers = calculate_optimal_gpu_layers(model_path);

    // Load new model with calculated GPU acceleration
    let model_params = LlamaModelParams::default()
        .with_n_gpu_layers(optimal_gpu_layers);

    println!("Loading model from: {}", model_path);
    println!("GPU layers configured: {} layers will be offloaded to GPU", optimal_gpu_layers);

    let model = LlamaModel::load_from_file(&state.backend, model_path, &model_params)
        .map_err(|e| format!("Failed to load model: {}", e))?;

    println!("Model loaded successfully!");

    // Read model's context length, token IDs, and chat template from GGUF metadata
    let (model_context_length, bos_token_id, eos_token_id, chat_template_type) = if let Ok(file) = fs::File::open(model_path) {
        let mut reader = BufReader::new(file);
        if let Ok(header) = GgufHeader::parse(&mut reader) {
            if let Ok(metadata) = GgufReader::read_metadata(&mut reader, header.n_kv) {
                let ctx_len = metadata.get("llama.context_length")
                    .and_then(|v| match v {
                        Value::Uint32(n) => Some(*n),
                        Value::Uint64(n) => Some(*n as u32),
                        _ => None,
                    });

                let bos_id = metadata.get("tokenizer.ggml.bos_token_id")
                    .and_then(|v| match v {
                        Value::Uint32(n) => Some(*n as i32),
                        Value::Int32(n) => Some(*n),
                        _ => None,
                    });

                let eos_id = metadata.get("tokenizer.ggml.eos_token_id")
                    .and_then(|v| match v {
                        Value::Uint32(n) => Some(*n as i32),
                        Value::Int32(n) => Some(*n),
                        _ => None,
                    });

                // Detect chat template type
                let template_type = metadata.get("tokenizer.chat_template")
                    .and_then(|v| match v {
                        Value::String(s) => {
                            // Detect template type based on template content
                            if s.contains("<|im_start|>") && s.contains("<|im_end|>") {
                                Some("ChatML".to_string()) // Qwen, OpenAI format
                            } else if s.contains("[INST]") && s.contains("[/INST]") {
                                Some("Mistral".to_string()) // Mistral format
                            } else if s.contains("<|start_header_id|>") {
                                Some("Llama3".to_string()) // Llama 3 format
                            } else {
                                Some("Generic".to_string()) // Fallback
                            }
                        }
                        _ => None,
                    });

                (ctx_len, bos_id, eos_id, template_type)
            } else {
                (None, None, None, None)
            }
        } else {
            (None, None, None, None)
        }
    } else {
        (None, None, None, None)
    };

    if let Some(ctx_len) = model_context_length {
        println!("Model context length from GGUF: {}", ctx_len);
    }
    if let Some(bos) = bos_token_id {
        println!("Model BOS token ID from GGUF: {}", bos);
    }
    if let Some(eos) = eos_token_id {
        println!("Model EOS token ID from GGUF: {}", eos);

        // Validate against what the model reports
        let model_eos = model.token_eos().0; // Extract underlying i32 from LlamaToken
        if eos != model_eos {
            println!("WARNING: GGUF EOS token ({}) doesn't match model.token_eos() ({})", eos, model_eos);
        } else {
            println!("✓ EOS token validation passed: GGUF and model agree on token {}", eos);
        }
    }

    if let Some(ref template) = chat_template_type {
        println!("Detected chat template type: {}", template);
    } else {
        println!("No chat template detected, using Mistral format as default");
    }

    state.model = Some(model);
    state.current_model_path = Some(model_path.to_string());
    state.model_context_length = model_context_length;
    state.chat_template_type = chat_template_type;
    state.last_used = std::time::SystemTime::now();

    Ok(())
}

// Helper function to unload the current model
pub async fn unload_model(llama_state: SharedLlamaState) -> Result<(), String> {
    let mut state_guard = llama_state.lock().map_err(|_| "Failed to lock LLaMA state")?;

    if let Some(state) = state_guard.as_mut() {
        state.model = None;
        state.current_model_path = None;
    }

    Ok(())
}
