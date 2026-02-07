use gguf_llms::{GgufHeader, GgufReader, Value};
use llama_cpp_2::{
    llama_backend::LlamaBackend,
    model::{params::LlamaModelParams, LlamaModel},
};
use std::fs;
use std::io::BufReader;

use super::models::{LlamaState, ModelStatus, SharedLlamaState};
use crate::{log_debug, log_info, log_warn};

// Re-export VRAM functions for backward compatibility (used by other modules)
pub use super::vram_calculator::calculate_optimal_gpu_layers;

// Helper function to get model status
pub fn get_model_status(llama_state: &SharedLlamaState) -> ModelStatus {
    match llama_state.lock() {
        Ok(state_guard) => {
            match state_guard.as_ref() {
                Some(state) => {
                    let loaded = state.model.is_some();
                    let model_path = state.current_model_path.clone();
                    let last_used = state
                        .last_used
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
                },
            }
        }
        Err(_) => ModelStatus {
            loaded: false,
            model_path: None,
            last_used: None,
            memory_usage_mb: None,
        },
    }
}

// Helper function to load a model
pub async fn load_model(llama_state: SharedLlamaState, model_path: &str) -> Result<(), String> {
    log_debug!("system", "load_model called with path: {}", model_path);

    // Handle poisoned mutex by recovering from panic
    let mut state_guard = llama_state.lock().unwrap_or_else(|poisoned| {
        log_debug!("system", "Mutex was poisoned, recovering...");
        poisoned.into_inner()
    });

    // Initialize backend if needed
    if state_guard.is_none() {
        let backend = LlamaBackend::init().map_err(|e| format!("Failed to init backend: {e}"))?;
        *state_guard = Some(LlamaState {
            backend,
            model: None,
            current_model_path: None,
            model_context_length: None,
            chat_template_type: None,
            chat_template_string: None,
            gpu_layers: None,
            last_used: std::time::SystemTime::now(),
            model_default_system_prompt: None,
            general_name: None,
            cached_system_prompt: None,
            cached_prompt_key: None,
            inference_cache: None,
        });
    }

    let state = state_guard
        .as_mut()
        .expect("Model state should be initialized");

    // Check if model is already loaded
    if let Some(ref current_path) = state.current_model_path {
        if current_path == model_path && state.model.is_some() {
            state.last_used = std::time::SystemTime::now();
            return Ok(()); // Model already loaded
        }
    }

    // CRITICAL: Drop inference cache BEFORE dropping the model.
    // The cached context borrows the model, so it must go first.
    state.inference_cache = None;
    // Unload current model if any
    state.model = None;
    state.current_model_path = None;

    // Calculate optimal GPU layers
    let optimal_gpu_layers = calculate_optimal_gpu_layers(model_path);

    // Load new model with calculated GPU acceleration
    let model_params = LlamaModelParams::default().with_n_gpu_layers(optimal_gpu_layers);

    log_info!("system", "Loading model from: {}", model_path);
    log_info!(
        "system",
        "GPU layers configured: {} layers will be offloaded to GPU",
        optimal_gpu_layers
    );

    let model = LlamaModel::load_from_file(&state.backend, model_path, &model_params)
        .map_err(|e| format!("Failed to load model: {e}"))?;

    log_info!("system", "Model loaded successfully!");

    // Read model's context length, token IDs, chat template, general name, and default system prompt from GGUF metadata
    let (
        model_context_length,
        bos_token_id,
        eos_token_id,
        chat_template_type,
        chat_template_string,
        default_system_prompt,
        general_name,
    ) = if let Ok(file) = fs::File::open(model_path) {
        let mut reader = BufReader::new(file);
        if let Ok(header) = GgufHeader::parse(&mut reader) {
            if let Ok(metadata) = GgufReader::read_metadata(&mut reader, header.n_kv) {
                let ctx_len = metadata.get("llama.context_length").and_then(|v| match v {
                    Value::Uint32(n) => Some(*n),
                    Value::Uint64(n) => Some(*n as u32),
                    _ => None,
                });

                let bos_id = metadata
                    .get("tokenizer.ggml.bos_token_id")
                    .and_then(|v| match v {
                        Value::Uint32(n) => Some(*n as i32),
                        Value::Int32(n) => Some(*n),
                        _ => None,
                    });

                let eos_id = metadata
                    .get("tokenizer.ggml.eos_token_id")
                    .and_then(|v| match v {
                        Value::Uint32(n) => Some(*n as i32),
                        Value::Int32(n) => Some(*n),
                        _ => None,
                    });

                // Extract full chat template string and detect type
                let (template_type, template_string) = metadata
                    .get("tokenizer.chat_template")
                    .map(|v| match v {
                        Value::String(s) => {
                            let template_type = if s.contains("<|im_start|>") && s.contains("<|im_end|>") {
                                "ChatML".to_string() // Qwen, OpenAI format
                            } else if s.contains("[INST]") && s.contains("[/INST]") {
                                "Mistral".to_string() // Mistral format
                            } else if s.contains("<|start_header_id|>") {
                                "Llama3".to_string() // Llama 3 format
                            } else if s.contains("<start_of_turn>") && s.contains("<end_of_turn>") {
                                "Gemma".to_string() // Gemma 3 format
                            } else {
                                "Generic".to_string() // Fallback
                            };
                            (Some(template_type), Some(s.clone()))
                        }
                        _ => (None, None),
                    })
                    .unwrap_or((None, None));

                // Extract default system prompt from chat template if available
                // Look for: {%- set default_system_message = '...' %} in the template
                let default_prompt =
                    metadata
                        .get("tokenizer.chat_template")
                        .and_then(|v| match v {
                            Value::String(template) => {
                                if let Some(start_idx) =
                                    template.find("set default_system_message = '")
                                {
                                    let after_start = &template
                                        [start_idx + "set default_system_message = '".len()..];
                                    if let Some(end_idx) = after_start.find("' %}") {
                                        return Some(after_start[..end_idx].to_string());
                                    }
                                }
                                None
                            }
                            _ => None,
                        });

                // Extract general.name from metadata
                let gen_name = metadata.get("general.name").and_then(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                });

                (ctx_len, bos_id, eos_id, template_type, template_string, default_prompt, gen_name)
            } else {
                (None, None, None, None, None, None, None)
            }
        } else {
            (None, None, None, None, None, None, None)
        }
    } else {
        (None, None, None, None, None, None, None)
    };

    if let Some(ctx_len) = model_context_length {
        log_info!("system", "Model context length from GGUF: {}", ctx_len);
    }
    if let Some(bos) = bos_token_id {
        log_info!("system", "Model BOS token ID from GGUF: {}", bos);
    }
    if let Some(eos) = eos_token_id {
        log_info!("system", "Model EOS token ID from GGUF: {}", eos);

        // Validate against what the model reports
        let model_eos = model.token_eos().0; // Extract underlying i32 from LlamaToken
        if eos != model_eos {
            log_warn!(
                "system",
                "WARNING: GGUF EOS token ({}) doesn't match model.token_eos() ({})",
                eos,
                model_eos
            );
        } else {
            log_info!(
                "system",
                "✓ EOS token validation passed: GGUF and model agree on token {}",
                eos
            );
        }
    }

    if let Some(ref template) = chat_template_type {
        log_info!("system", "Detected chat template type: {}", template);
    } else {
        log_info!(
            "system",
            "No chat template detected, using Mistral format as default"
        );
    }

    state.model = Some(model);
    state.current_model_path = Some(model_path.to_string());
    state.model_context_length = model_context_length;
    state.chat_template_type = chat_template_type;
    state.chat_template_string = chat_template_string;
    state.gpu_layers = Some(optimal_gpu_layers);
    state.last_used = std::time::SystemTime::now();
    state.model_default_system_prompt = default_system_prompt.clone();
    state.general_name = general_name.clone();
    // Invalidate caches (model changed)
    state.cached_system_prompt = None;
    state.cached_prompt_key = None;
    state.inference_cache = None;

    if let Some(ref name) = general_name {
        log_info!("system", "Model general.name: {}", name);
    }

    if let Some(ref prompt) = default_system_prompt {
        log_info!(
            "system",
            "Model default system prompt found: {}...",
            &prompt.chars().take(50).collect::<String>()
        );
    }

    Ok(())
}

// Helper function to unload the current model
pub async fn unload_model(llama_state: SharedLlamaState) -> Result<(), String> {
    let mut state_guard = llama_state
        .lock()
        .map_err(|_| "Failed to lock LLaMA state")?;

    // Drop the entire backend + model to guarantee memory is released.
    // This is safe because load_model() will reinitialize the backend on demand.
    if state_guard.is_some() {
        log_info!(
            "system",
            "Unloading model and tearing down backend to free memory"
        );
        // CRITICAL: Drop inference cache before dropping entire state.
        if let Some(ref mut state) = *state_guard {
            state.inference_cache = None;
        }
        *state_guard = None;
    } else {
        log_debug!(
            "system",
            "Unload requested but no backend/model was initialized"
        );
    }

    Ok(())
}

// Tests moved to vram_calculator.rs
