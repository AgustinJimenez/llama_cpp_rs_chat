use gguf_llms::{GgufHeader, GgufReader, Value};
use llama_cpp_2::{
    llama_backend::LlamaBackend,
    model::{params::{LlamaModelParams, LlamaSplitMode}, LlamaModel},
};
use std::fs;
use std::io::BufReader;

use super::models::{LlamaState, ModelStatus, SharedLlamaState};
#[cfg(feature = "vision")]
use super::models::VisionState;
use crate::{log_debug, log_info, log_warn};

// Re-export VRAM functions for backward compatibility (used by other modules)
pub use super::vram_calculator::{calculate_optimal_gpu_layers, read_gguf_block_count};

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
                        has_vision: None,
                        tool_tags: None,
                    }
                }
                None => ModelStatus {
                    loaded: false,
                    model_path: None,
                    last_used: None,
                    memory_usage_mb: None,
                    has_vision: None,
                    tool_tags: None,
                },
            }
        }
        Err(_) => ModelStatus {
            loaded: false,
            model_path: None,
            last_used: None,
            memory_usage_mb: None,
            has_vision: None,
            tool_tags: None,
        },
    }
}

/// Extra model-level parameters applied at load time.
#[derive(Debug, Clone)]
pub struct ModelParams {
    pub use_mlock: bool,
    pub use_mmap: bool,
    pub main_gpu: i32,
    pub split_mode: String,
}

impl Default for ModelParams {
    fn default() -> Self {
        Self {
            use_mlock: false,
            use_mmap: true,
            main_gpu: 0,
            split_mode: "layer".to_string(),
        }
    }
}

fn parse_split_mode(s: &str) -> LlamaSplitMode {
    match s.to_lowercase().as_str() {
        "none" => LlamaSplitMode::None,
        "row" => LlamaSplitMode::Row,
        _ => LlamaSplitMode::Layer,
    }
}

// Helper function to load a model
pub async fn load_model(llama_state: SharedLlamaState, model_path: &str, requested_gpu_layers: Option<u32>, model_params: Option<&ModelParams>) -> Result<(), String> {
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
            general_name: None,
            cached_system_prompt: None,
            cached_prompt_key: None,
            inference_cache: None,
            #[cfg(feature = "vision")]
            vision_state: None,
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

    // CRITICAL: Drop inference cache and vision state BEFORE dropping the model.
    // Both borrow the model, so they must go first.
    state.inference_cache = None;
    #[cfg(feature = "vision")]
    { state.vision_state = None; }
    // Unload current model if any
    state.model = None;
    state.current_model_path = None;

    // Use requested GPU layers if provided, otherwise auto-calculate
    let mut optimal_gpu_layers = requested_gpu_layers.unwrap_or_else(|| calculate_optimal_gpu_layers(model_path));

    // CRITICAL: Cap gpu_layers at model's actual layer count to avoid offloading
    // the output/embedding layer to GPU. ngl > n_layers causes llama_decode to hang
    // on Qwen3.5 (hybrid MoE+recurrent) with large context sizes.
    if let Some(n_layers) = super::vram_calculator::read_gguf_block_count(model_path) {
        if optimal_gpu_layers > n_layers {
            log_warn!(
                "system",
                "Capping gpu_layers from {} to {} (model block_count) to avoid output layer GPU offload hang",
                optimal_gpu_layers,
                n_layers
            );
            optimal_gpu_layers = n_layers;
        }
    }

    // Load new model with configured GPU acceleration and model params
    let defaults = ModelParams::default();
    let mp = model_params.unwrap_or(&defaults);
    let llama_model_params = LlamaModelParams::default()
        .with_n_gpu_layers(optimal_gpu_layers)
        .with_use_mlock(mp.use_mlock)
        .with_use_mmap(mp.use_mmap)
        .with_main_gpu(mp.main_gpu)
        .with_split_mode(parse_split_mode(&mp.split_mode));

    log_info!("system", "Loading model from: {}", model_path);
    log_info!(
        "system",
        "GPU layers configured: {} layers will be offloaded to GPU",
        optimal_gpu_layers
    );
    if mp.use_mlock {
        log_info!("system", "mlock enabled (force model in RAM)");
    }
    if !mp.use_mmap {
        log_info!("system", "mmap disabled (no memory-mapped loading)");
    }
    if mp.main_gpu != 0 {
        log_info!("system", "Main GPU: {}", mp.main_gpu);
    }
    if mp.split_mode != "layer" {
        log_info!("system", "Split mode: {}", mp.split_mode);
    }

    let model = LlamaModel::load_from_file(&state.backend, model_path, &llama_model_params)
        .map_err(|e| format!("Failed to load model: {e}"))?;

    log_info!("system", "Model loaded successfully!");

    // Read model's context length, token IDs, chat template, and general name from GGUF metadata
    let (
        model_context_length,
        bos_token_id,
        eos_token_id,
        chat_template_type,
        chat_template_string,
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
                            } else if s.contains("<|start|>") && s.contains("<|end|>") && s.contains("<|channel|>") {
                                "Harmony".to_string() // gpt-oss-20b Harmony format
                            } else if s.contains("<|observation|>") && s.contains("<|user|>") && s.contains("<|assistant|>") {
                                "GLM".to_string() // GLM-4 family (has <|observation|> role)
                            } else if s.contains("<|system|>") && s.contains("<|user|>") && s.contains("<|assistant|>") && s.contains("<|end|>") {
                                "Phi".to_string() // Phi-3/Phi-4 format
                            } else {
                                "Generic".to_string() // Fallback
                            };
                            (Some(template_type), Some(s.clone()))
                        }
                        _ => (None, None),
                    })
                    .unwrap_or((None, None));

                // Extract general.name from metadata
                let gen_name = metadata.get("general.name").and_then(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                });

                (ctx_len, bos_id, eos_id, template_type, template_string, gen_name)
            } else {
                (None, None, None, None, None, None)
            }
        } else {
            (None, None, None, None, None, None)
        }
    } else {
        (None, None, None, None, None, None)
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

    // Scan for mmproj companion file for vision support
    #[cfg(feature = "vision")]
    let vision_state = scan_and_init_vision(&model, model_path, optimal_gpu_layers);

    state.model = Some(model);
    state.current_model_path = Some(model_path.to_string());
    state.model_context_length = model_context_length;
    state.chat_template_type = chat_template_type;
    state.chat_template_string = chat_template_string;
    state.gpu_layers = Some(optimal_gpu_layers);
    state.last_used = std::time::SystemTime::now();
    state.general_name = general_name.clone();
    // Invalidate caches (model changed)
    state.cached_system_prompt = None;
    state.cached_prompt_key = None;
    state.inference_cache = None;
    #[cfg(feature = "vision")]
    { state.vision_state = vision_state; }

    if let Some(ref name) = general_name {
        log_info!("system", "Model general.name: {}", name);
    }

    Ok(())
}

#[cfg(feature = "vision")]
/// Scan the model's directory for an mmproj companion GGUF file and initialize
/// an MtmdContext for vision support. Returns None if no mmproj file found or
/// initialization fails.
fn scan_and_init_vision(
    model: &LlamaModel,
    model_path: &str,
    gpu_layers: u32,
) -> Option<VisionState> {
    use llama_cpp_2::mtmd::{MtmdContext, MtmdContextParams};
    use std::ffi::CString;
    use std::path::Path;

    let model_dir = Path::new(model_path).parent()?;

    // Find the first mmproj file in the same directory
    let mmproj_path = fs::read_dir(model_dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|e| e.path())
        .find(|p| {
            p.extension().map(|e| e == "gguf").unwrap_or(false)
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.contains("mmproj"))
                    .unwrap_or(false)
        })?;

    let mmproj_str = mmproj_path.to_string_lossy().to_string();
    log_info!("system", "Found mmproj file: {}", mmproj_str);

    // Initialize MtmdContext
    let use_gpu = gpu_layers > 0;
    log_info!("system", "Vision: attempting to init mmproj from {}, use_gpu={}", mmproj_str, use_gpu);
    let params = MtmdContextParams {
        use_gpu,
        print_timings: false,
        n_threads: 4,
        media_marker: CString::new("<__media__>").unwrap(),
    };

    match MtmdContext::init_from_file(&mmproj_str, model, &params) {
        Ok(ctx) => {
            let has_vision = ctx.support_vision();
            log_info!(
                "system",
                "Vision context initialized (vision={}, gpu={})",
                has_vision,
                use_gpu
            );
            Some(VisionState {
                context: ctx,
                mmproj_path: mmproj_str,
            })
        }
        Err(e) => {
            log_warn!("system", "Failed to init vision context: {}", e);
            None
        }
    }
}

// Tests moved to vram_calculator.rs
