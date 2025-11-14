use std::fs;
use std::io::BufReader;
use std::process::Command;
use gguf_llms::{GgufHeader, GgufReader, Value};
use llama_cpp_2::{
    llama_backend::LlamaBackend,
    model::{params::LlamaModelParams, LlamaModel},
};

use super::models::{ModelStatus, SharedLlamaState, LlamaState};

// Helper function to detect available VRAM
fn get_available_vram_gb() -> Option<f64> {
    // Try nvidia-smi first
    if let Ok(output) = Command::new("nvidia-smi")
        .args(&["--query-gpu=memory.free", "--format=csv,noheader,nounits"])
        .output()
    {
        if output.status.success() {
            if let Ok(output_str) = String::from_utf8(output.stdout) {
                if let Ok(vram_mb) = output_str.trim().parse::<f64>() {
                    return Some(vram_mb / 1024.0); // Convert MB to GB
                }
            }
        }
    }

    // Fallback: assume 22GB available (conservative estimate)
    println!("[VRAM] Could not detect VRAM, assuming 22GB available");
    Some(22.0)
}

// Helper function to calculate KV cache size in GB
fn calculate_kv_cache_size_gb(
    n_ctx: u32,
    n_layers: u32,
    n_kv_heads: u32,
    head_dim: u32,
) -> f64 {
    // KV cache = tokens × layers × kv_heads × head_dim × 2 (key+value) × 2 bytes (fp16)
    let bytes = n_ctx as f64 * n_layers as f64 * n_kv_heads as f64 * head_dim as f64 * 2.0 * 2.0;
    bytes / (1024.0 * 1024.0 * 1024.0) // Convert to GB
}

// Helper function to calculate safe context size based on available VRAM
pub fn calculate_safe_context_size(
    model_path: &str,
    requested_ctx: u32,
    available_vram_gb: Option<f64>,
    gpu_layers: Option<u32>,
) -> (u32, bool) {
    let available_vram = available_vram_gb.unwrap_or_else(|| {
        get_available_vram_gb().unwrap_or(22.0)
    });

    // Read model metadata to get architecture details
    let (n_layers, n_kv_heads, embedding_len) = if let Ok(file) = fs::File::open(model_path) {
        let mut reader = BufReader::new(file);
        if let Ok(header) = GgufHeader::parse(&mut reader) {
            if let Ok(metadata) = GgufReader::read_metadata(&mut reader, header.n_kv) {
                // Try to get layer count, kv heads, embedding length
                let layers = metadata.get("gemma3.block_count")
                    .or_else(|| metadata.get("llama.block_count"))
                    .and_then(|v| match v {
                        Value::Uint32(n) => Some(*n),
                        _ => None,
                    }).unwrap_or(48); // Default to 48 layers

                let kv_heads = metadata.get("gemma3.attention.head_count_kv")
                    .or_else(|| metadata.get("llama.attention.head_count_kv"))
                    .and_then(|v| match v {
                        Value::Uint32(n) => Some(*n),
                        _ => None,
                    }).unwrap_or(8); // Default to 8 KV heads

                let emb_len = metadata.get("gemma3.embedding_length")
                    .or_else(|| metadata.get("llama.embedding_length"))
                    .and_then(|v| match v {
                        Value::Uint32(n) => Some(*n),
                        _ => None,
                    }).unwrap_or(3840); // Default to 3840

                (layers, kv_heads, emb_len)
            } else {
                (48, 8, 3840) // Defaults
            }
        } else {
            (48, 8, 3840)
        }
    } else {
        (48, 8, 3840)
    };

    // Calculate head dimension
    let head_dim = embedding_len / (n_kv_heads * 2); // Rough estimate

    // Estimate model size (rough: 12GB for 12B model)
    let model_size_gb = if let Ok(metadata) = fs::metadata(model_path) {
        metadata.len() as f64 / (1024.0 * 1024.0 * 1024.0)
    } else {
        12.0 // Default estimate
    };

    // Calculate GPU layers (auto-detect if not provided by user)
    let gpu_layers_count = gpu_layers.unwrap_or_else(|| calculate_optimal_gpu_layers(model_path));

    // Calculate what fraction of the model is on GPU
    let gpu_fraction = (gpu_layers_count as f64) / (n_layers as f64);
    let model_vram_usage = model_size_gb * gpu_fraction;

    println!("[VRAM] GPU layers: {}/{} ({:.1}% of model)",
             gpu_layers_count, n_layers, gpu_fraction * 100.0);
    println!("[VRAM] Model VRAM usage: {:.2}GB ({:.1}% of {:.2}GB total)",
             model_vram_usage, gpu_fraction * 100.0, model_size_gb);

    // Available VRAM for KV cache = total - model_on_gpu - overhead
    let vram_for_cache = (available_vram - model_vram_usage - 2.0).max(0.0);

    println!("[VRAM] Available: {:.2}GB, Model: {:.2}GB, Available for KV cache: {:.2}GB",
             available_vram, model_size_gb, vram_for_cache);

    // Calculate KV cache size for requested context
    let requested_cache_gb = calculate_kv_cache_size_gb(requested_ctx, n_layers, n_kv_heads, head_dim);

    println!("[VRAM] Requested context: {} tokens, KV cache: {:.2}GB",
             requested_ctx, requested_cache_gb);

    if requested_cache_gb <= vram_for_cache {
        // Requested context fits in VRAM
        println!("[VRAM] ✓ Requested context size fits in available VRAM");
        return (requested_ctx, false);
    }

    // Calculate safe context size
    // max_tokens = vram_for_cache / (layers × kv_heads × head_dim × 4)
    let bytes_per_token = n_layers as f64 * n_kv_heads as f64 * head_dim as f64 * 4.0;
    let safe_tokens = ((vram_for_cache * 1024.0 * 1024.0 * 1024.0) / bytes_per_token) as u32;

    // Round down to nearest power of 2 for cleaner values
    let safe_ctx = if safe_tokens >= 32768 {
        32768
    } else if safe_tokens >= 16384 {
        16384
    } else if safe_tokens >= 8192 {
        8192
    } else if safe_tokens >= 4096 {
        4096
    } else {
        2048
    };

    println!("[VRAM] ⚠️  Requested context ({}) exceeds VRAM capacity!", requested_ctx);
    println!("[VRAM] ⚠️  Auto-reducing to safe context size: {} tokens", safe_ctx);

    (safe_ctx, true) // Return safe context and flag that it was reduced
}

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
            gpu_layers: None,
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
                            } else if s.contains("<start_of_turn>") && s.contains("<end_of_turn>") {
                                Some("Gemma".to_string()) // Gemma 3 format
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
    state.gpu_layers = Some(optimal_gpu_layers);
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
