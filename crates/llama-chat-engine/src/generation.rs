
use llama_cpp_2::{
    llama_batch::LlamaBatch,
    model::AddBos,
};
#[cfg(feature = "vision")]
use llama_cpp_2::{context::LlamaContext, token::LlamaToken};
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

use llama_chat_config::load_config_for_conversation;
use super::model_manager::load_model;
use llama_chat_types::*;
use crate::SharedConversationLogger;
use super::templates::{apply_system_prompt_by_type_with_tags, get_behavioral_system_prompt};
use super::jinja_templates::get_available_tools_openai_with_mcp;
use super::sampler::create_sampler;
use llama_chat_db::event_log::log_event;
use super::sub_checks::quick_task_completion_check;

// Re-export submodule items used by sibling modules
pub(crate) use super::context_eval::create_fresh_context;

use super::context_eval::{evaluate_text_prompt, CONTEXT_SIZE, MODEL_PATH};
#[cfg(feature = "vision")]
use super::context_eval::build_context_params;
use super::prompt_builder::{resolve_tool_tags, snapshot_context_overhead};
#[cfg(feature = "vision")]
use super::prompt_builder::inject_media_markers;
use super::token_loop::{TokenGenState, TokenGenConfig, VisionCtxRef, run_generation_loop};
use super::stop_conditions::ExecBlockTracker;

/// Output from a generation run, including timing metrics.
pub struct GenerationOutput {
    #[allow(dead_code)]
    pub response: String,
    pub tokens_used: i32,
    pub max_tokens: i32,
    /// Why generation stopped: "stop" (EOS), "length" (max_tokens), "cancelled", "tool_calls", "error".
    pub finish_reason: String,
    /// Prompt evaluation speed in tokens/second.
    pub prompt_tok_per_sec: Option<f64>,
    /// Generation speed in tokens/second.
    pub gen_tok_per_sec: Option<f64>,
    /// Generation time in milliseconds.
    pub gen_eval_ms: Option<f64>,
    /// Number of tokens generated.
    pub gen_tokens: Option<i32>,
    /// Prompt evaluation time in milliseconds.
    pub prompt_eval_ms: Option<f64>,
    /// Number of prompt tokens evaluated.
    pub prompt_tokens: Option<i32>,
    /// Token usage breakdown by category.
    pub token_breakdown: Option<llama_chat_types::TokenBreakdown>,
}

/// Quick Y/N check: did a tool call produce useful results?
/// Returns true if the output looks successful, false if it's an error/404/empty.
pub async fn generate_llama_response(
    user_message: &str,
    llama_state: SharedLlamaState,
    conversation_logger: SharedConversationLogger,
    token_sender: Option<mpsc::UnboundedSender<TokenData>>,
    skip_user_logging: bool,
    db: llama_chat_db::SharedDatabase,
    cancel: Arc<AtomicBool>,
    image_data: Option<&[String]>,
    mcp_manager: Option<Arc<dyn llama_chat_tools::McpManagerOps>>,
) -> Result<GenerationOutput, String> {
    sys_debug!(
        "[GENERATION] generate_llama_response called, token_sender is {}",
        if token_sender.is_some() {
            "Some"
        } else {
            "None"
        }
    );

    // Get conversation ID for logging
    let conversation_id = {
        let logger = conversation_logger
            .lock()
            .map_err(|_| "Failed to lock conversation logger")?;
        logger.get_conversation_id()
    };
    sys_debug!("[GENERATION] Conversation ID: {}", conversation_id);

    // Log user message to conversation file (unless already logged)
    if !skip_user_logging {
        let mut logger = conversation_logger
            .lock()
            .map_err(|_| "Failed to lock conversation logger")?;
        // Estimate token count (~4 chars/token). Exact count requires model tokenizer
        // which isn't available until after the model lock below.
        let estimated_tokens = (user_message.len() / 4).max(1) as i32;
        logger.log_message_with_tokens("USER", user_message, Some(estimated_tokens));
    }

    // Load configuration to get model path and context size
    // Uses per-conversation config if available, falls back to global
    let config = load_config_for_conversation(&db, &conversation_id);
    let model_path = config.model_path.as_deref().unwrap_or(MODEL_PATH);
    let stop_tokens = config
        .stop_tokens
        .clone()
        .unwrap_or_else(get_common_stop_tokens);

    // Ensure model is loaded
    load_model(llama_state.clone(), model_path, None, None, None, None).await?;

    // Now use the shared state for generation (mutable for inference cache)
    let mut state_guard = llama_state
        .lock()
        .map_err(|_| "Failed to lock LLaMA state")?;
    let state = state_guard.as_mut().ok_or("LLaMA state not initialized")?;
    let model = state.model.as_ref().ok_or("No model loaded")?;

    // Get context size: prefer user config, then cap GGUF context_length to our default,
    // since many models declare 128K+ which OOMs on most GPUs.
    let context_size = config.context_size.unwrap_or_else(|| {
        state
            .model_context_length
            .map(|ctx| ctx.min(CONTEXT_SIZE))
            .unwrap_or(CONTEXT_SIZE)
    });

    log_info!(
        &conversation_id,
        "Using context size: {} (model max: {:?}, default cap: {})",
        context_size,
        state.model_context_length,
        CONTEXT_SIZE
    );

    // Create sampler based on configuration (pass model for DRY sampler)
    let mut sampler = create_sampler(&config, &conversation_id, Some(model));

    // Read conversation history from file and create chat prompt
    let raw_conversation_content = {
        let logger = conversation_logger
            .lock()
            .map_err(|_| "Failed to lock conversation logger")?;
        logger
            .load_conversation_from_file()
            .unwrap_or_else(|_| logger.get_full_conversation())
    };

    // Auto-compact conversation if it's approaching context window limit
    // Use real overhead from conversation_context if available (from previous generation)
    let conv_id_for_overhead = conversation_id.trim_end_matches(".txt");
    let cached_overhead = db.get_context_overhead_tokens(conv_id_for_overhead);

    // Only drop inference cache if compaction actually needs to run.
    // Compaction creates a temporary 4K context for summarization — on GPUs with tight
    // VRAM (24GB), having two contexts simultaneously causes OOM and worker crash.
    // For normal turns (no compaction), preserving the cache lets us skip re-evaluating
    // the entire conversation, saving 10-90+ seconds on long conversations.
    let needs_compaction = super::compaction::should_compact(
        &raw_conversation_content,
        context_size,
        if cached_overhead > 0 { Some(cached_overhead) } else { None },
    );
    if needs_compaction {
        state.inference_cache = None;
    }

    let conversation_content = super::compaction::maybe_compact_conversation(
        &raw_conversation_content,
        context_size,
        &conversation_id,
        &db,
        model,
        &state.backend,
        state.chat_template_string.as_deref(),
        if cached_overhead > 0 { Some(cached_overhead) } else { None },
        token_sender.as_ref(),
    );

    // Log if compaction changed the conversation
    if conversation_content != raw_conversation_content {
        eprintln!("[COMPACTION] Conversation compacted — cache was dropped");
    }

    // Convert conversation to chat format using the new 3-system prompt approach
    let template_type = state.chat_template_type.clone();
    let chat_template_string = state.chat_template_string.clone();
    let general_name = state.general_name.clone();

    // Harmony models use <|end|> as a sub-turn separator (analysis→commentary→final),
    // NOT as a generation stop. Remove it so the model can produce multi-channel responses
    // including tool calls on the "commentary" channel.
    let stop_tokens = if template_type.as_deref() == Some("Harmony") {
        stop_tokens.into_iter().filter(|t| t != "<|end|>").collect()
    } else {
        stop_tokens
    };

    // Resolve tool tags: saved tag_pairs → old override fields → model name lookup
    let tags = resolve_tool_tags(&config, general_name.as_deref());
    // Get model's actual BOS/EOS token text for Jinja templates
    #[allow(deprecated)]
    use llama_cpp_2::model::Special;
    #[allow(deprecated)]
    let bos_text = model
        .token_to_str(model.token_bos(), Special::Tokenize)
        .unwrap_or_else(|_| "<s>".to_string());
    #[allow(deprecated)]
    let eos_text = model
        .token_to_str(model.token_eos(), Special::Tokenize)
        .unwrap_or_else(|_| "</s>".to_string());

    log_info!(&conversation_id, "=== TEMPLATE DEBUG ===");
    log_info!(&conversation_id, "Template type: {:?}", template_type);
    log_info!(&conversation_id, "General name: {:?}", general_name);
    log_info!(&conversation_id, "BOS token text: {:?}, EOS token text: {:?}", bos_text, eos_text);
    log_info!(&conversation_id, "Tool tags: exec_open={}, exec_close={}", tags.exec_open, tags.exec_close);
    log_info!(
        &conversation_id,
        "Conversation content:\n{}",
        conversation_content
    );

    // Get MCP tool definitions if manager is available
    let mcp_tool_defs = mcp_manager.as_ref()
        .map(|mgr| mgr.get_tool_definitions())
        .unwrap_or_default();
    let mcp_tools_ref = if mcp_tool_defs.is_empty() { None } else { Some(mcp_tool_defs.as_slice()) };

    // Use the 3-system prompt dispatcher with model-specific tool tags
    let prompt = apply_system_prompt_by_type_with_tags(
        &conversation_content,
        template_type.as_deref(),
        chat_template_string.as_deref(),
        &tags,
        &bos_text,
        &eos_text,
        mcp_tools_ref,
    )?;
    log_info!(&conversation_id, "=== FINAL PROMPT BEING SENT TO MODEL ===");
    log_info!(&conversation_id, "{}", prompt);
    log_info!(
        &conversation_id,
        "=== END PROMPT (length: {} chars) ===",
        prompt.len()
    );

    // Compute and cache token breakdown (system prompt + tool defs) in conversation_context table.
    // Uses content hash to skip re-tokenization when nothing changed (~0.1ms each, pure CPU).
    let system_prompt_text = get_behavioral_system_prompt();
    let tools_json = serde_json::to_string(
        &get_available_tools_openai_with_mcp(mcp_tools_ref)
    ).unwrap_or_default();

    let conv_id_clean = conversation_id.trim_end_matches(".txt");
    let (system_prompt_token_count, tool_def_token_count) = snapshot_context_overhead(
        &db, conv_id_clean, model, &system_prompt_text, &tools_json, &conversation_id,
    );
    log_info!(
        &conversation_id,
        "Token breakdown: system_prompt={}, tool_definitions={} (cached in conversation_context)",
        system_prompt_token_count, tool_def_token_count
    );

    // Context parameters (n_ctx/n_batch used by vision feature)
    #[allow(unused_variables)]
    let n_ctx = NonZeroU32::new(context_size).expect("Context size must be non-zero");
    let offload_kqv = state.gpu_layers.unwrap_or(0) > 0;
    let flash_attention = config.flash_attention;
    let cache_type_k = config.cache_type_k.clone();
    let cache_type_v = config.cache_type_v.clone();
    #[allow(unused_variables)]
    let n_batch = config.n_batch;
    if offload_kqv {
        log_info!(
            &conversation_id,
            "⚡ KV cache on GPU ({} layers offloaded)",
            state.gpu_layers.unwrap_or(0)
        );
    }
    if flash_attention {
        log_info!(&conversation_id, "⚡ Flash attention enabled");
    }
    if cache_type_k != "f16" || cache_type_v != "f16" {
        log_info!(
            &conversation_id,
            "KV cache quantization: K={}, V={}",
            cache_type_k,
            cache_type_v
        );
    }

    const PROMPT_BATCH_CAP: usize = 2048;
    let batch_cap = PROMPT_BATCH_CAP;

    // Check cancellation before expensive prompt decode
    if cancel.load(Ordering::Relaxed) {
        return Err("Cancelled".to_string());
    }

    // Decode base64 image data if present (supports multiple images)
    let image_bytes_vec: Vec<Vec<u8>> = if let Some(images) = image_data {
        use base64::Engine;
        images.iter().filter_map(|data_str| {
            // Strip data URI prefix if present (e.g., "data:image/png;base64,...")
            let b64 = if let Some(comma_pos) = data_str.find(',') {
                &data_str[comma_pos + 1..]
            } else {
                data_str.as_str()
            };
            match base64::engine::general_purpose::STANDARD.decode(b64) {
                Ok(bytes) => {
                    log_info!(&conversation_id, "Decoded image: {} bytes", bytes.len());
                    Some(bytes)
                }
                Err(e) => {
                    log_warn!(&conversation_id, "Failed to decode image base64: {}", e);
                    None
                }
            }
        }).collect()
    } else {
        Vec::new()
    };

    // Determine if we should use vision path
    #[cfg(feature = "vision")]
    let use_vision = !image_bytes_vec.is_empty() && state.vision_state.is_some();
    #[cfg(not(feature = "vision"))]
    let use_vision = false;
    if use_vision {
        log_info!(&conversation_id, "Using vision path with {} images", image_bytes_vec.len());
    }

    // Two code paths: vision (mtmd) or standard text-only
    let (mut context, prompt_tokens, tokens) = if use_vision {
        #[cfg(feature = "vision")]
        {
        // === VISION PATH: Use MtmdContext to process text + images ===
        use llama_cpp_2::mtmd::{MtmdBitmap, MtmdInputText};

        let vision = state.vision_state.as_ref().unwrap();

        // Insert <__media__> markers before the user's message in the prompt.
        // One marker per image tells mtmd where each image's embeddings go in the token stream.
        let vision_prompt = inject_media_markers(&prompt, user_message, image_bytes_vec.len());
        log_debug!(&conversation_id, "Vision prompt with {} markers, len={}", image_bytes_vec.len(), vision_prompt.len());

        // Create bitmaps from raw image bytes (supports JPG, PNG, BMP, GIF, etc.)
        let bitmaps: Vec<MtmdBitmap> = image_bytes_vec.iter().enumerate().map(|(i, bytes)| {
            log_debug!(&conversation_id, "Creating bitmap {} from {} bytes", i, bytes.len());
            let bmp = MtmdBitmap::from_buffer(&vision.context, bytes)
                .map_err(|e| format!("Failed to create image bitmap {}: {e}", i))?;
            log_debug!(&conversation_id, "Bitmap {}: {}x{}", i, bmp.nx(), bmp.ny());
            Ok(bmp)
        }).collect::<Result<Vec<_>, String>>()?;
        let bitmap_refs: Vec<&MtmdBitmap> = bitmaps.iter().collect();

        // Tokenize the prompt + images into chunks
        log_debug!(&conversation_id, "Tokenizing with {} bitmaps...", bitmap_refs.len());
        let text_input = MtmdInputText {
            text: vision_prompt.clone(),
            add_special: true,
            parse_special: true,
        };
        let chunks = vision.context.tokenize(text_input, &bitmap_refs)
            .map_err(|e| format!("Vision tokenization failed: {e}"))?;
        let n_prompt_tokens = chunks.total_tokens();
        log_info!(&conversation_id, "Vision tokenized: {} chunks, {} total tokens ({} images)", chunks.len(), n_prompt_tokens, bitmaps.len());

        // Create fresh context (no KV cache reuse for vision — image embeddings can't be cached simply)
        drop(state.inference_cache.take());
        let mut ctx_params = build_context_params(n_ctx, offload_kqv, &config);
        // Handle non-causal attention for vision models
        if vision.context.decode_use_non_causal() {
            ctx_params = ctx_params.with_flash_attention_policy(0); // Must disable flash attention for non-causal
        }

        log_debug!(&conversation_id, "Creating vision context...");
        let ctx = unsafe {
            let real_ctx = model
                .new_context(&state.backend, ctx_params)
                .map_err(|e| format!("Context creation failed: {e}"))?;
            std::mem::transmute::<LlamaContext<'_>, LlamaContext<'static>>(real_ctx)
        };
        log_debug!(&conversation_id, "Vision context created, starting eval_chunks...");

        // Evaluate all chunks (text tokens + image embeddings) through the model
        let n_past = chunks.eval_chunks(&vision.context, &ctx, 0, 0, n_batch as i32, true)
            .map_err(|e| format!("Vision eval_chunks failed: {e}"))?;
        log_info!(&conversation_id, "Vision eval_chunks complete: n_past={}", n_past);

        // Create a dummy tokens vec for cache storage (vision doesn't use standard tokens)
        let dummy_tokens = vec![LlamaToken(0); n_past as usize];
        (ctx, n_prompt_tokens, dummy_tokens)
        }
        #[cfg(not(feature = "vision"))]
        unreachable!("Vision feature not enabled")
    } else {
        // === STANDARD TEXT PATH ===
        let tokens = model
            .str_to_token(&prompt, AddBos::Never)
            .map_err(|e| format!("Tokenization failed: {e}"))?;
        log_debug!(&conversation_id, "Tokenized to {} tokens", tokens.len());

        // Guard: if prompt exceeds 95% of context, it won't fit regardless of retries
        if tokens.len() as u32 > context_size.saturating_sub(context_size / 20) {
            log_event(&conversation_id, "context_overflow", &format!(
                "Prompt {} tokens > 95% of context {} — conversation too large even after compaction",
                tokens.len(), context_size
            ));
            return Err(format!(
                "Context too small for conversation ({} tokens in {} context) — try increasing context size or starting a new conversation",
                tokens.len(), context_size
            ));
        }

        let (ctx, _skip_tokens) = match evaluate_text_prompt(
            &mut state.inference_cache, model, &state.backend,
            &tokens, &conversation_id, context_size,
            offload_kqv, flash_attention, &cache_type_k, &cache_type_v,
            &config, batch_cap, Some(&cancel),
        ) {
            Ok(result) => result,
            Err(e) if e.contains("Context too small") => {
                eprintln!("[GENERATION] Prompt decode failed, retrying in 2s...");
                state.inference_cache = None;
                std::thread::sleep(std::time::Duration::from_secs(2));
                evaluate_text_prompt(
                    &mut state.inference_cache, model, &state.backend,
                    &tokens, &conversation_id, context_size,
                    offload_kqv, flash_attention, &cache_type_k, &cache_type_v,
                    &config, batch_cap, Some(&cancel),
                )?
            },
            Err(e) => return Err(e),
        };
        let prompt_tokens = tokens.len();
        (ctx, prompt_tokens, tokens)
    };

    let gen_start = Instant::now();

    let mut batch = LlamaBatch::new(batch_cap, 1);

    // Start assistant message in conversation log (enables streaming broadcast)
    {
        let mut logger = conversation_logger
            .lock()
            .map_err(|_| "Failed to lock conversation logger")?;
        logger.start_assistant_message();
    }

    let token_pos = tokens.len() as i32;
    let remaining_context = (context_size as i32) - token_pos - 128;
    let max_total_tokens = remaining_context.max(512);

    // Log generation start
    log_event(&conversation_id, "gen_start", &format!(
        "ctx={}, prompt_tokens={}, remaining={}, flash_attn={}, kv_cache={}",
        context_size, token_pos, max_total_tokens, flash_attention,
        if cache_type_k == "f16" && cache_type_v == "f16" { "f16".to_string() } else { format!("K={} V={}", cache_type_k, cache_type_v) }
    ));

    log_info!(
        &conversation_id,
        "Context size: {}, Prompt tokens: {}, Max tokens to generate: {}",
        context_size, token_pos, max_total_tokens
    );

    let mut gen = TokenGenState {
        response: String::new(),
        token_pos,
        total_tokens_generated: 0,
        generated_token_ids: Vec::new(),
        logger_synced_len: 0,
        last_logger_sync: Instant::now(),
        exec_tracker: ExecBlockTracker::new(),
        recent_commands: Vec::new(),
        consecutive_loop_blocks: 0,
        last_exec_scan_pos: 0,
        finish_reason: "stop".to_string(),
        tool_response_tokens: 0,
        loop_recoveries: 0,
    };

    let cfg = TokenGenConfig {
        conversation_id: &conversation_id,
        tags: &tags,
        template_type: template_type.as_deref(),
        stop_tokens: &stop_tokens,
        context_size,
        max_total_tokens,
        web_search_provider: config.web_search_provider.as_deref(),
        web_search_api_key: config.web_search_api_key.as_deref(),
        use_rtk: config.use_rtk,
        use_htmd: config.use_htmd,
        browser_backend: &crate::browser::BrowserBackend::from_config(config.web_browser_backend.as_deref()),
        n_batch,
        mcp_manager: mcp_manager.clone(),
        db: db.clone(),
        backend: &state.backend,
        chat_template_string: chat_template_string.as_deref(),
        proactive_compaction: config.proactive_compaction,
    };

    // Build vision context reference for tool response image injection
    #[cfg(feature = "vision")]
    let vision_ctx_ref: VisionCtxRef<'_> = state.vision_state.as_ref().map(|v| &v.context);
    #[cfg(not(feature = "vision"))]
    let vision_ctx_ref: VisionCtxRef<'_> = ();

    // Set abort callback so llama_decode checks the cancel flag mid-computation.
    // This makes cancel responsive even during long decode calls (32K+ context).
    extern "C" fn abort_cb(data: *mut std::ffi::c_void) -> bool {
        let flag = unsafe { &*(data as *const AtomicBool) };
        flag.load(Ordering::Relaxed)
    }
    let cancel_ptr = Arc::as_ptr(&cancel) as *mut std::ffi::c_void;
    unsafe { context.set_abort_callback(Some(abort_cb), cancel_ptr); }

    let loop_result = run_generation_loop(
        &mut gen, &cfg, &mut context, model, &mut sampler,
        &mut batch, &token_sender, &conversation_logger, &cancel,
        vision_ctx_ref,
    );

    // Clear abort callback before reusing or storing the context
    unsafe { context.set_abort_callback(None, std::ptr::null_mut()); }

    loop_result?;

    let token_pos = gen.token_pos;

    // Use llama.cpp internal perf timings (decode-only, matches llama-server measurement)
    let timings = context.timings();
    let gen_eval_ms = timings.t_eval_ms();
    let prompt_eval_ms_internal = timings.t_p_eval_ms();
    let n_eval = timings.n_eval() as usize;
    let n_p_eval = timings.n_p_eval() as usize;

    let prompt_tok_per_sec = if prompt_eval_ms_internal > 0.0 && n_p_eval > 0 {
        Some(n_p_eval as f64 / prompt_eval_ms_internal * 1000.0)
    } else {
        None
    };
    let gen_tok_per_sec = if gen_eval_ms > 0.0 && n_eval > 0 {
        Some(n_eval as f64 / gen_eval_ms * 1000.0)
    } else {
        None
    };

    // Also compute wall-clock for logging comparison
    let wall_gen_ms = gen_start.elapsed().as_secs_f64() * 1000.0;
    log_info!(
        &conversation_id,
        "Timing: prompt={:.1} tok/s ({} tokens in {:.0}ms), gen={:.1} tok/s ({} tokens in {:.0}ms, wall={:.0}ms)",
        prompt_tok_per_sec.unwrap_or(0.0),
        n_p_eval,
        prompt_eval_ms_internal,
        gen_tok_per_sec.unwrap_or(0.0),
        n_eval,
        gen_eval_ms,
        wall_gen_ms
    );

    // Log generation complete
    log_event(&conversation_id, "gen_done", &format!(
        "finish={}, tokens={}, {:.1} tok/s, {:.1}s, tool_calls={}",
        gen.finish_reason, n_eval, gen_tok_per_sec.unwrap_or(0.0),
        wall_gen_ms / 1000.0, gen.recent_commands.len()
    ));

    // Finish assistant message and persist metrics
    let was_cancelled = cancel.load(Ordering::Relaxed);
    {
        let mut logger = conversation_logger
            .lock()
            .map_err(|_| "Failed to lock conversation logger")?;

        // If cancelled mid-tool-call, strip the incomplete tool call XML to
        // prevent conversation corruption. Find the last complete text before
        // the unclosed <tool_call> tag.
        if was_cancelled {
            // Check if there's an unclosed tool_call tag
            let last_tool_open = gen.response.rfind("<tool_call>");
            let last_tool_close = gen.response.rfind("</tool_call>");
            let last_fn_close = gen.response.rfind("</function>");
            if let Some(open_pos) = last_tool_open {
                let is_unclosed = match last_tool_close {
                    Some(close_pos) => close_pos < open_pos,
                    None => true,
                };
                let is_fn_unclosed = match last_fn_close {
                    Some(close_pos) => close_pos < open_pos,
                    None => true,
                };
                if is_unclosed || is_fn_unclosed {
                    eprintln!("[CANCEL] Stripping incomplete tool call at pos {}", open_pos);
                    gen.response.truncate(open_pos);
                    gen.logger_synced_len = gen.logger_synced_len.min(open_pos);
                }
            }
        }

        let remaining = &gen.response[gen.logger_synced_len..];
        if !remaining.is_empty() {
            logger.log_token_bulk(remaining);
        }
        logger.set_token_counts(token_pos, context_size as i32);
        logger.finish_assistant_message();
        if was_cancelled {
            logger.log_message("system", "[Generation stopped by user]");
        }
        logger.log_metrics(prompt_tok_per_sec, gen_tok_per_sec, token_pos, max_total_tokens);
        logger.store_message_timings(
            prompt_tok_per_sec,
            gen_tok_per_sec,
            if gen_eval_ms > 0.0 { Some(gen_eval_ms) } else { None },
            if n_eval > 0 { Some(n_eval as i32) } else { None },
            if prompt_eval_ms_internal > 0.0 { Some(prompt_eval_ms_internal) } else { None },
            if n_p_eval > 0 { Some(n_p_eval as i32) } else { None },
        );
    }

    // Store context back into inference cache for KV cache reuse on next turn
    let total_cached = tokens.len() + gen.generated_token_ids.len();
    let gen_count = gen.generated_token_ids.len();
    let mut all_evaluated = tokens;
    all_evaluated.extend(gen.generated_token_ids);
    state.inference_cache = Some(InferenceCache {
        context,
        conversation_id: conversation_id.clone(),
        evaluated_tokens: all_evaluated,
        context_size,
        offload_kqv,
        flash_attention,
        cache_type_k,
        cache_type_v,
    });
    log_info!(
        &conversation_id,
        "Stored KV cache: {} total tokens ({} generated this turn)",
        total_cached,
        gen_count
    );

    // If the model stopped naturally (EOS) but was in an agentic task (tool calls made),
    // do a quick Y/N check to see if the task is actually complete.
    // This catches cases where the model emits EOS mid-task.
    log_event(&conversation_id, "task_check", &format!(
        "finish_reason={}, tool_response_tokens={}, commands={}",
        gen.finish_reason, gen.tool_response_tokens, gen.recent_commands.len()
    ));
    eprintln!("[TASK_CHECK] finish_reason={}, tool_response_tokens={}, recent_commands={}", gen.finish_reason, gen.tool_response_tokens, gen.recent_commands.len());
    if gen.finish_reason == "stop" && gen.tool_response_tokens > 0 {
        // Include the user's request + last ~800 chars of response for context.
        // More context helps the checker see partial task completion.
        let user_prefix = if user_message.len() > 300 {
            let mut end = 300;
            while end < user_message.len() && !user_message.is_char_boundary(end) { end += 1; }
            format!("{}...", &user_message[..end])
        } else {
            user_message.to_string()
        };
        let response_tail = if gen.response.len() > 800 {
            let mut start = gen.response.len() - 800;
            while start > 0 && !gen.response.is_char_boundary(start) { start += 1; }
            &gen.response[start..]
        } else {
            &gen.response
        };
        let check_text = format!("USER REQUEST: {user_prefix}\n\nASSISTANT RESPONSE TAIL:\n{response_tail}");
        let is_complete = quick_task_completion_check(
            model, &state.backend, state.chat_template_string.as_deref(), &conversation_id,
            &check_text,
        );
        if !is_complete {
            eprintln!("[TASK_CHECK] Y/N check said NO → setting finish_reason=yn_continue for auto-continue");
            log_event(&conversation_id, "yn_check", "NO → auto-continue");
            gen.finish_reason = "yn_continue".to_string();
        } else {
            log_event(&conversation_id, "yn_check", "YES → task complete");
        }
    }

    // Clear global status on generation end
    llama_chat_db::event_log::clear_global_status();

    Ok(GenerationOutput {
        response: gen.response.trim().to_string(),
        tokens_used: token_pos,
        max_tokens: context_size as i32,
        finish_reason: gen.finish_reason,
        prompt_tok_per_sec,
        gen_tok_per_sec,
        gen_eval_ms: if gen_eval_ms > 0.0 { Some(gen_eval_ms) } else { None },
        gen_tokens: if n_eval > 0 { Some(n_eval as i32) } else { None },
        prompt_eval_ms: if prompt_eval_ms_internal > 0.0 { Some(prompt_eval_ms_internal) } else { None },
        prompt_tokens: if n_p_eval > 0 { Some(n_p_eval as i32) } else { None },
        token_breakdown: Some(TokenBreakdown {
            system_prompt: system_prompt_token_count,
            tool_definitions: tool_def_token_count,
            conversation_messages: (prompt_tokens as i32 - system_prompt_token_count - tool_def_token_count).max(0),
            tool_calls_and_results: gen.tool_response_tokens,
            model_response: n_eval as i32,
        }),
    })
}

