
use llama_cpp_2::{
    context::params::{KvCacheType, LlamaContextParams},
    context::LlamaContext,
    llama_batch::LlamaBatch,
    model::{AddBos, LlamaModel},
    sampling::LlamaSampler,
    token::LlamaToken,
};
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

use super::super::config::load_config_for_conversation;
use super::super::database::Database;
use super::super::model_manager::load_model;
use super::super::models::*;
use super::command_executor::{
    check_and_execute_command_with_tags, inject_output_tokens,
};
use super::stop_conditions::{check_stop_conditions, ExecBlockTracker};
use super::templates::{apply_system_prompt_by_type_with_tags, get_behavioral_system_prompt};
use super::jinja_templates::get_available_tools_openai_with_mcp;
use super::tool_tags::{default_tags, derive_tool_tags_from_pairs, get_tool_tags_for_model, try_get_tool_tags_for_model, ToolTags};
use super::sampler::create_sampler;
use crate::{log_debug, log_info, log_warn, sys_debug};

// Constants for LLaMA configuration
const CONTEXT_SIZE: u32 = 32768;

/// Parse a KV cache type string (from config) into the llama-cpp-2 enum.
fn parse_kv_cache_type(s: &str) -> KvCacheType {
    match s.to_lowercase().as_str() {
        "f32" => KvCacheType::F32,
        "f16" => KvCacheType::F16,
        "q8_0" => KvCacheType::Q8_0,
        "q4_0" => KvCacheType::Q4_0,
        "q4_1" => KvCacheType::Q4_1,
        "q5_0" => KvCacheType::Q5_0,
        "q5_1" => KvCacheType::Q5_1,
        _ => KvCacheType::F16, // default
    }
}
const MODEL_PATH: &str =
    "/app/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf";

/// Build LlamaContextParams from config, applying all context-level settings.
fn build_context_params(
    n_ctx: NonZeroU32,
    offload_kqv: bool,
    config: &SamplerConfig,
) -> LlamaContextParams {
    let mut params = LlamaContextParams::default()
        .with_n_ctx(Some(n_ctx))
        .with_offload_kqv(offload_kqv)
        .with_type_k(parse_kv_cache_type(&config.cache_type_k))
        .with_type_v(parse_kv_cache_type(&config.cache_type_v))
        .with_n_batch(config.n_batch)
        .with_n_ubatch(config.n_ubatch)
        .with_no_perf(false); // Enable internal perf timing (matches llama-server)

    if config.flash_attention {
        params = params.with_flash_attention_policy(1);
    }
    if config.n_threads > 0 {
        params = params.with_n_threads(config.n_threads);
    }
    if config.n_threads_batch > 0 {
        params = params.with_n_threads_batch(config.n_threads_batch);
    }
    if config.rope_freq_base > 0.0 {
        params = params.with_rope_freq_base(config.rope_freq_base);
    }
    if config.rope_freq_scale > 0.0 {
        params = params.with_rope_freq_scale(config.rope_freq_scale);
    }
    params
}

/// Special conversation ID for warmup cache (system prompt pre-evaluation).
pub const WARMUP_CONVERSATION_ID: &str = "__warmup__";

/// Pre-evaluate the system prompt into the KV cache after model load.
///
/// Creates a context, tokenizes just the system prompt portion, evaluates it,
/// and stores the result in `inference_cache` so the first real generation
/// can skip re-evaluating those tokens.
pub fn warmup_system_prompt(
    llama_state: SharedLlamaState,
    db: &Database,
) -> Result<(), String> {
    use super::super::config::{load_config, get_resolved_system_prompt};

    let config = load_config(db);
    let system_prompt = get_resolved_system_prompt(db, &Some(llama_state.clone()));

    let system_prompt = match system_prompt {
        Some(p) if !p.is_empty() => p,
        _ => {
            sys_debug!("[WARMUP] No system prompt configured, skipping warmup");
            return Ok(());
        }
    };

    let mut state_guard = llama_state
        .lock()
        .map_err(|_| "Failed to lock LLaMA state")?;
    let state = state_guard.as_mut().ok_or("LLaMA state not initialized")?;
    let model = state.model.as_ref().ok_or("No model loaded")?;

    let context_size = config.context_size.unwrap_or_else(|| {
        state
            .model_context_length
            .map(|ctx| ctx.min(CONTEXT_SIZE))
            .unwrap_or(CONTEXT_SIZE)
    });

    // Build a minimal conversation with just the system prompt
    let conversation_content = format!("SYSTEM:\n{}\n\n", system_prompt);

    let template_type = state.chat_template_type.clone();
    let chat_template_string = state.chat_template_string.clone();
    let general_name = state.general_name.clone();

    let tags = get_tool_tags_for_model(general_name.as_deref()).with_overrides(
        config.tool_tag_exec_open.as_deref(),
        config.tool_tag_exec_close.as_deref(),
        config.tool_tag_output_open.as_deref(),
        config.tool_tag_output_close.as_deref(),
    );

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

    // Format using the same template as generation (no MCP tools for warmup)
    let prompt = apply_system_prompt_by_type_with_tags(
        &conversation_content,
        template_type.as_deref(),
        chat_template_string.as_deref(),
        &tags,
        &bos_text,
        &eos_text,
        None,
    )?;

    // Tokenize
    let tokens = model
        .str_to_token(&prompt, AddBos::Never)
        .map_err(|e| format!("Warmup tokenization failed: {e}"))?;

    if tokens.is_empty() {
        sys_debug!("[WARMUP] Empty token list, skipping warmup");
        return Ok(());
    }

    // Create context with the same parameters generation would use
    let n_ctx = NonZeroU32::new(context_size).expect("Context size must be non-zero");
    let offload_kqv = state.gpu_layers.unwrap_or(0) > 0;
    let flash_attention = config.flash_attention;
    let cache_type_k = config.cache_type_k.clone();
    let cache_type_v = config.cache_type_v.clone();

    let ctx_params = build_context_params(n_ctx, offload_kqv, &config);

    let start = Instant::now();
    let mut context = unsafe {
        let real_ctx = model
            .new_context(&state.backend, ctx_params)
            .map_err(|e| format!("Warmup context creation failed: {e}"))?;
        std::mem::transmute::<LlamaContext<'_>, LlamaContext<'static>>(real_ctx)
    };

    // Evaluate system prompt tokens in batches
    const BATCH_CAP: usize = 2048;
    let n_chunks = tokens.len().div_ceil(BATCH_CAP);
    let mut batch = LlamaBatch::new(BATCH_CAP, 1);

    for chunk_idx in 0..n_chunks {
        let start_tok = chunk_idx * BATCH_CAP;
        let end_tok = std::cmp::min(start_tok + BATCH_CAP, tokens.len());

        batch.clear();
        for (offset, &token) in tokens[start_tok..end_tok].iter().enumerate() {
            let pos = start_tok + offset;
            let is_last = pos == tokens.len() - 1;
            batch
                .add(token, pos as i32, &[0], is_last)
                .map_err(|e| format!("Warmup batch add failed: {e}"))?;
        }

        context.decode(&mut batch).map_err(|e| {
            format!("Warmup decode failed (chunk {}/{}): {e}", chunk_idx + 1, n_chunks)
        })?;
    }

    let elapsed = start.elapsed();
    let tok_per_sec = tokens.len() as f64 / elapsed.as_secs_f64();
    eprintln!(
        "[WORKER] System prompt warmup: {} tokens evaluated in {:.2}s ({:.1} tok/s)",
        tokens.len(),
        elapsed.as_secs_f64(),
        tok_per_sec
    );

    // Store in inference cache for reuse by first generation
    state.inference_cache = Some(InferenceCache {
        context,
        conversation_id: WARMUP_CONVERSATION_ID.to_string(),
        evaluated_tokens: tokens,
        context_size,
        offload_kqv,
        flash_attention,
        cache_type_k,
        cache_type_v,
    });

    Ok(())
}

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
    pub token_breakdown: Option<super::super::models::TokenBreakdown>,
}

/// Mutable state tracked across the token generation loop.
struct TokenGenState {
    response: String,
    token_pos: i32,
    total_tokens_generated: i32,
    generated_token_ids: Vec<LlamaToken>,
    logger_synced_len: usize,
    last_logger_sync: Instant,
    exec_tracker: ExecBlockTracker,
    recent_commands: Vec<String>,
    last_exec_scan_pos: usize,
    /// Why generation stopped: "stop", "length", "cancelled", "tool_calls", "error".
    finish_reason: String,
    /// Accumulated tokens from tool call responses injected into context.
    tool_response_tokens: i32,
}

/// Read-only configuration for the token generation loop.
#[allow(dead_code)]
struct TokenGenConfig<'a> {
    conversation_id: &'a str,
    tags: &'a ToolTags,
    template_type: Option<&'a str>,
    stop_tokens: &'a [String],
    context_size: u32,
    max_total_tokens: i32,
    web_search_provider: Option<&'a str>,
    web_search_api_key: Option<&'a str>,
    use_rtk: bool,
    use_htmd: bool,
    browser_backend: &'a crate::web::browser::BrowserBackend,
    n_batch: u32,
    mcp_manager: Option<Arc<crate::web::mcp::McpManager>>,
    db: super::super::database::SharedDatabase,
    backend: &'a llama_cpp_2::llama_backend::LlamaBackend,
    chat_template_string: Option<&'a str>,
}

/// Vision context reference for tool response image injection.
/// When the `vision` feature is enabled, this carries an `Option<&MtmdContext>`.
/// Otherwise it's a zero-size unit type so the parameter compiles away.
#[cfg(feature = "vision")]
type VisionCtxRef<'a> = Option<&'a llama_cpp_2::mtmd::MtmdContext>;
#[cfg(not(feature = "vision"))]
type VisionCtxRef<'a> = ();

/// Stall detection: if a single token takes longer than this, abort generation.
const TOKEN_STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

// Tool call round limit removed — context window is the natural limit.

/// Minimum tokens generated before repetition detection kicks in.
const REPETITION_CHECK_MIN_TOKENS: i32 = 500;

/// Check every N tokens for repetition loops.
const REPETITION_CHECK_INTERVAL: i32 = 256;

/// Detect repetition loops by measuring trigram diversity in the tail of the response.
///
/// When a model enters a degenerate loop (e.g., "1a1b1c1d..." repeating), the
/// character-level diversity drops dramatically. We measure the ratio of unique
/// 3-character sequences (trigrams) to total trigrams in the last 500 chars.
/// Normal text/code has >30% unique trigrams; repetitive garbage has <15%.
fn detect_repetition_loop(text: &str) -> bool {
    const TAIL_LEN: usize = 2000;
    const THRESHOLD: f64 = 0.10; // 10% unique trigrams = definitely repeating

    if text.len() < TAIL_LEN {
        return false;
    }

    // Work on the tail bytes directly — avoids panicking on multi-byte UTF-8 boundaries
    let bytes = text.as_bytes();
    let start = bytes.len() - TAIL_LEN;
    let tail = &bytes[start..];
    let total_trigrams = tail.len().saturating_sub(2);
    if total_trigrams == 0 {
        return false;
    }

    let mut seen = std::collections::HashSet::with_capacity(128);
    for i in 0..total_trigrams {
        seen.insert([tail[i], tail[i + 1], tail[i + 2]]);
    }

    let ratio = seen.len() as f64 / total_trigrams as f64;
    ratio < THRESHOLD
}

/// Run the outer generation loop: generates tokens, detects/executes commands, resumes.
///
/// Returns the final response and token counts. The `context` is mutated in place
/// and should be stored back into the inference cache after this returns.
#[allow(clippy::too_many_arguments)]
fn run_generation_loop(
    gen: &mut TokenGenState,
    cfg: &TokenGenConfig<'_>,
    context: &mut LlamaContext<'static>,
    model: &LlamaModel,
    sampler: &mut LlamaSampler,
    batch: &mut LlamaBatch,
    token_sender: &Option<mpsc::UnboundedSender<TokenData>>,
    conversation_logger: &SharedConversationLogger,
    cancel: &Arc<AtomicBool>,
    #[allow(unused_variables)]
    vision_ctx: VisionCtxRef<'_>,
) -> Result<(), String> {
    #[allow(deprecated)]
    use llama_cpp_2::model::Special;

    log_debug!(
        cfg.conversation_id,
        "Tool tags: exec_open={:?}, exec_close={:?}, output_open={:?}, output_close={:?}",
        cfg.tags.exec_open, cfg.tags.exec_close, cfg.tags.output_open, cfg.tags.output_close
    );
    log_debug!(cfg.conversation_id, "Stop tokens configured: {:?}", cfg.stop_tokens);
    log_debug!(cfg.conversation_id, "EOS token ID: {}", model.token_eos());

    // tool_call_rounds tracking removed — no limit
    let mut stall_checkpoint = Instant::now();

    loop {
        let mut command_executed = false;
        let mut hit_stop_condition = false;
        let tokens_to_generate = std::cmp::min(2048, cfg.max_total_tokens - gen.total_tokens_generated);

        log_debug!(
            cfg.conversation_id,
            "Starting generation cycle: tokens_to_generate={}, total_tokens_generated={}",
            tokens_to_generate, gen.total_tokens_generated
        );

        for i in 0..tokens_to_generate {
            if i % 4 == 0 && cancel.load(Ordering::Relaxed) {
                log_info!(cfg.conversation_id, "Generation cancelled by user");
                gen.finish_reason = "cancelled".to_string();
                hit_stop_condition = true;
                break;
            }

            if i % 50 == 0 {
                log_debug!(cfg.conversation_id, "Generated {} tokens so far...", gen.total_tokens_generated);
            }

            // Stall detection: amortized check every 16 tokens (saves 2 syscalls/token)
            if i & 15 == 15 {
                let batch_elapsed = stall_checkpoint.elapsed();
                if batch_elapsed > TOKEN_STALL_TIMEOUT {
                    let secs = batch_elapsed.as_secs();
                    log_info!(cfg.conversation_id, "Generation stalled: 16 tokens took {}s. Aborting.", secs);
                    if let Some(ref sender) = token_sender {
                        let _ = sender.send(TokenData {
                            token: format!(
                                "\n\n[Generation stalled — batch of 16 tokens took {}s. The model may be too large for your hardware.]",
                                secs
                            ),
                            tokens_used: gen.total_tokens_generated,
                            max_tokens: cfg.max_total_tokens,
                        });
                    }
                    gen.finish_reason = "error".to_string();
                    hit_stop_condition = true;
                    break;
                }
                stall_checkpoint = Instant::now();
            }

            let next_token = sampler.sample(context, -1);

            if next_token == model.token_eos() {
                log_debug!(
                    cfg.conversation_id,
                    "EOS token detected at position {} (in_exec_block: {})",
                    gen.total_tokens_generated, gen.exec_tracker.is_inside()
                );
                // Append EOS token text so RAW view shows where model stopped
                #[allow(deprecated)]
                if let Ok(eos_str) = model.token_to_str(next_token, Special::Tokenize) {
                    gen.response.push_str(&eos_str);
                    if let Some(ref sender) = token_sender {
                        let _ = sender.send(TokenData {
                            token: eos_str,
                            tokens_used: gen.token_pos,
                            max_tokens: cfg.context_size as i32,
                        });
                    }
                }
                hit_stop_condition = true;
                break;
            }

            batch.clear();
            batch.add(next_token, gen.token_pos, &[0], true)
                .map_err(|e| format!("Batch add failed at token {}: {e}", gen.total_tokens_generated))?;
            context.decode(batch)
                .map_err(|e| format!("Decode failed at token {}: {e}", gen.total_tokens_generated))?;

            gen.token_pos += 1;
            gen.total_tokens_generated += 1;
            gen.generated_token_ids.push(next_token);

            #[allow(deprecated)]
            let token_str = match model.token_to_str(next_token, Special::Tokenize) {
                Ok(s) => s,
                Err(e) => {
                    log_warn!(cfg.conversation_id, "Token {} can't be displayed: {}. Continuing.", next_token, e);
                    continue;
                }
            };

            if gen.total_tokens_generated <= 10 {
                log_debug!(cfg.conversation_id, "Token #{}: id={}, str={:?}", gen.total_tokens_generated, next_token, token_str);
            }

            // Check for stop sequences
            let stop_result = check_stop_conditions(&gen.response, &token_str, cfg.stop_tokens, gen.exec_tracker.is_inside());
            if stop_result.should_stop {
                if stop_result.partial_to_remove > 0 {
                    let new_len = gen.response.len().saturating_sub(stop_result.partial_to_remove);
                    gen.response.truncate(new_len);
                }
                hit_stop_condition = true;
                break;
            }

            gen.response.push_str(&token_str);
            gen.exec_tracker.update(&token_str, gen.response.len());

            // Periodic repetition loop detection
            if gen.total_tokens_generated > REPETITION_CHECK_MIN_TOKENS
                && gen.total_tokens_generated % REPETITION_CHECK_INTERVAL == 0
                && detect_repetition_loop(&gen.response)
            {
                log_info!(
                    cfg.conversation_id,
                    "🛑 Repetition loop detected at token {}, stopping generation",
                    gen.total_tokens_generated
                );
                if let Some(ref sender) = token_sender {
                    let _ = sender.send(TokenData {
                        token: "\n\n[Generation stopped: repetition loop detected]".to_string(),
                        tokens_used: gen.token_pos,
                        max_tokens: cfg.context_size as i32,
                    });
                }
                gen.finish_reason = "error".to_string();
                hit_stop_condition = true;
                break;
            }

            // Stream token to frontend
            if let Some(ref sender) = token_sender {
                let _ = sender.send(TokenData {
                    token: token_str.clone(),
                    tokens_used: gen.token_pos,
                    max_tokens: cfg.context_size as i32,
                });
            }

            // Periodic sync to logger (every 200ms)
            if gen.last_logger_sync.elapsed() >= std::time::Duration::from_millis(200) {
                if let Ok(mut logger) = conversation_logger.lock() {
                    logger.set_token_counts(gen.token_pos, cfg.context_size as i32);
                    let new_content = &gen.response[gen.logger_synced_len..];
                    if !new_content.is_empty() {
                        logger.log_token_bulk(new_content);
                    }
                    gen.logger_synced_len = gen.response.len();
                }
                gen.last_logger_sync = Instant::now();
            }

            // Check for and execute commands in the response.
            // Fast gate: only call the expensive detector when the new token
            // contains a character that could close a tool call block.
            // This skips ~90% of tokens (no '>', ']', or '}').
            let token_has_close_char = token_str.as_bytes().iter().any(|&b| b == b'>' || b == b']' || b == b'}');
            if token_has_close_char {
            if let Some(exec_result) = check_and_execute_command_with_tags(
                &gen.response, gen.last_exec_scan_pos, cfg.conversation_id, model, cfg.tags,
                cfg.template_type, cfg.web_search_provider, cfg.web_search_api_key,
                &mut gen.recent_commands, token_sender, gen.token_pos, cfg.context_size,
                Some(cancel.clone()), cfg.use_rtk, cfg.use_htmd, cfg.browser_backend,
                cfg.mcp_manager.clone(), cfg.db.clone(),
                cfg.backend, cfg.chat_template_string,
            )? {
                // Sync accumulated content + command output to logger
                {
                    let mut logger = conversation_logger.lock()
                        .map_err(|_| "Failed to lock conversation logger")?;
                    logger.set_token_counts(gen.token_pos, cfg.context_size as i32);
                    let pending = &gen.response[gen.logger_synced_len..];
                    if !pending.is_empty() {
                        logger.log_token_bulk(pending);
                    }
                    logger.log_token(&exec_result.output_block);
                }

                gen.response.push_str(&exec_result.output_block);
                gen.logger_synced_len = gen.response.len();

                // Choose injection path: vision (images + MtmdContext) or standard text tokens
                #[cfg(feature = "vision")]
                let used_vision = if !exec_result.response_images.is_empty() {
                    if let Some(mtmd_ctx) = vision_ctx {
                        eprintln!("[VISION] Injecting {} image(s) via vision pipeline...", exec_result.response_images.len());
                        match inject_tool_response_with_vision(
                            &exec_result, mtmd_ctx, context,
                            &mut gen.token_pos, cfg.n_batch, cfg.conversation_id,
                        ) {
                            Ok(()) => {
                                eprintln!("[VISION] Vision injection succeeded, token_pos={}", gen.token_pos);
                                true
                            }
                            Err(e) => {
                                eprintln!("[VISION] Vision injection failed: {e}, falling back to text");
                                false
                            }
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };
                #[cfg(not(feature = "vision"))]
                let used_vision = false;

                if !used_vision {
                    log_info!(cfg.conversation_id, "Injecting {} output tokens into context...", exec_result.model_tokens.len());
                    inject_output_tokens(
                        &exec_result.model_tokens, batch, context,
                        &mut gen.token_pos, cfg.conversation_id,
                    )?;
                }

                gen.tool_response_tokens += exec_result.model_tokens.len() as i32;
                gen.generated_token_ids.extend(exec_result.model_tokens.iter().map(|&id| LlamaToken(id)));
                command_executed = true;
                // tool call round (no limit)
                hit_stop_condition = false;
                gen.last_exec_scan_pos = gen.response.len();
                // Reset exec block tracker after tool execution — the tool call
                // block is now closed (result injected), so we must allow stop
                // tokens to fire again for the model's continuation text.
                gen.exec_tracker = ExecBlockTracker::new();
                break;
            }
            } // token_has_close_char
        }

        if hit_stop_condition {
            break;
        }
        if gen.total_tokens_generated >= cfg.max_total_tokens {
            gen.finish_reason = "length".to_string();
            break;
        }

        if false {
            // Tool call round limit removed — let the model work until it's done
            // (context window is the natural limit)
            break;
        }

        if !command_executed {
            log_debug!(cfg.conversation_id, "Continuing generation: no stop condition hit");
        }
    }

    Ok(())
}

/// Evaluate tokenized prompt through the model, reusing KV cache when possible.
///
/// Returns `(context, skip_count)` where `skip_count` is how many tokens were
/// already in the cache and didn't need re-evaluation.
#[allow(clippy::too_many_arguments)]
fn evaluate_text_prompt(
    inference_cache: &mut Option<InferenceCache>,
    model: &LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    tokens: &[LlamaToken],
    conversation_id: &str,
    context_size: u32,
    offload_kqv: bool,
    flash_attention: bool,
    cache_type_k: &str,
    cache_type_v: &str,
    config: &SamplerConfig,
    batch_cap: usize,
) -> Result<(LlamaContext<'static>, usize), String> {
    let n_ctx = NonZeroU32::new(context_size).expect("Context size must be non-zero");

    let cached = inference_cache.take();
    let (mut ctx, skip_tokens) = match cached {
        Some(cache)
            if (cache.conversation_id == conversation_id
                || cache.conversation_id == WARMUP_CONVERSATION_ID)
                && cache.context_size == context_size
                && cache.offload_kqv == offload_kqv
                && cache.flash_attention == flash_attention
                && cache.cache_type_k == cache_type_k
                && cache.cache_type_v == cache_type_v =>
        {
            let common_len = cache.evaluated_tokens.iter()
                .zip(tokens.iter()).take_while(|(a, b)| a == b).count();

            if common_len < cache.evaluated_tokens.len() {
                log_info!(conversation_id, "KV cache diverged at token {} (cached {}), starting fresh",
                    common_len, cache.evaluated_tokens.len());
                drop(cache.context);
                let ctx = create_fresh_context(model, backend, n_ctx, offload_kqv, config)?;
                (ctx, 0)
            } else {
                log_info!(conversation_id, "♻️ Reusing KV cache: {} of {} prompt tokens already evaluated",
                    common_len, tokens.len());
                (cache.context, common_len)
            }
        }
        _ => {
            drop(cached);
            log_debug!(conversation_id, "Creating fresh context (size={}K tokens)...", context_size / 1024);
            let ctx = create_fresh_context(model, backend, n_ctx, offload_kqv, config)?;
            (ctx, 0)
        }
    };

    // Reset perf counters so timings cover only this turn (not accumulated from cache)
    ctx.reset_timings();

    // Evaluate only new tokens (skip those already in KV cache)
    let new_tokens = &tokens[skip_tokens..];
    if !new_tokens.is_empty() {
        let n_chunks = new_tokens.len().div_ceil(batch_cap);
        log_debug!(conversation_id, "Decoding {} new prompt tokens in {} chunks (skipped {})...",
            new_tokens.len(), n_chunks, skip_tokens);

        let mut batch = LlamaBatch::new(batch_cap, 1);
        for chunk_idx in 0..n_chunks {
            let start = chunk_idx * batch_cap;
            let end = std::cmp::min(start + batch_cap, new_tokens.len());

            batch.clear();
            for (offset, &token) in new_tokens[start..end].iter().enumerate() {
                let pos = skip_tokens + start + offset;
                let is_last = pos == tokens.len() - 1;
                batch.add(token, pos as i32, &[0], is_last)
                    .map_err(|e| format!("Batch add failed at prompt token {pos}: {e}"))?;
            }

            ctx.decode(&mut batch).map_err(|e| {
                format!("Prompt decode failed (chunk {}/{}): {e}", chunk_idx + 1, n_chunks)
            })?;
        }
    } else {
        log_info!(conversation_id, "All {} prompt tokens already in KV cache, skipping decode", tokens.len());
    }

    Ok((ctx, skip_tokens))
}

/// Create a fresh LlamaContext with transmuted 'static lifetime for cache storage.
pub(super) fn create_fresh_context(
    model: &LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    n_ctx: NonZeroU32,
    offload_kqv: bool,
    config: &SamplerConfig,
) -> Result<LlamaContext<'static>, String> {
    let ctx_params = build_context_params(n_ctx, offload_kqv, config);
    unsafe {
        let real_ctx = model
            .new_context(backend, ctx_params)
            .map_err(|e| format!("Context creation failed: {e}"))?;
        Ok(std::mem::transmute::<LlamaContext<'_>, LlamaContext<'static>>(real_ctx))
    }
}

/// Resolve ToolTags from config: model name lookup → saved tag_pairs → old override fields.
///
/// Known models always use their native tags (model name lookup).
/// Saved tag_pairs are only used for unknown models (custom user configuration).
fn resolve_tool_tags(config: &SamplerConfig, general_name: Option<&str>) -> ToolTags {
    // Priority 1: Model name lookup (always correct for known models)
    if let Some(tags) = try_get_tool_tags_for_model(general_name) {
        return tags;
    }
    // Priority 2: Derive from saved tag_pairs (user-edited in UI, for unknown models)
    if let Some(pairs) = &config.tag_pairs {
        if let Some(tags) = derive_tool_tags_from_pairs(pairs) {
            return tags;
        }
    }
    // Priority 3: Old override fields + default tags (backward compat)
    default_tags().with_overrides(
        config.tool_tag_exec_open.as_deref(),
        config.tool_tag_exec_close.as_deref(),
        config.tool_tag_output_open.as_deref(),
        config.tool_tag_output_close.as_deref(),
    )
}

/// Generate response from LLaMA model with streaming support.
///
/// Handles token generation, stop conditions, command execution, and conversation logging.
/// Supports multiple sampling strategies and automatic context size validation.
pub async fn generate_llama_response(
    user_message: &str,
    llama_state: SharedLlamaState,
    conversation_logger: SharedConversationLogger,
    token_sender: Option<mpsc::UnboundedSender<TokenData>>,
    skip_user_logging: bool,
    db: super::super::database::SharedDatabase,
    cancel: Arc<AtomicBool>,
    image_data: Option<&[String]>,
    mcp_manager: Option<Arc<crate::web::mcp::McpManager>>,
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
        logger.log_message("USER", user_message);
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
    let conversation_content = {
        let logger = conversation_logger
            .lock()
            .map_err(|_| "Failed to lock conversation logger")?;
        logger
            .load_conversation_from_file()
            .unwrap_or_else(|_| logger.get_full_conversation())
    };

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

    // Compute token breakdown by category (system prompt, tool defs, conversation).
    // Tokenizes each section independently (~0.1ms each, pure CPU).
    let system_prompt_text = get_behavioral_system_prompt();
    let system_prompt_token_count = model
        .str_to_token(&system_prompt_text, AddBos::Never)
        .map(|t| t.len() as i32)
        .unwrap_or(0);
    let tools_json = serde_json::to_string(
        &get_available_tools_openai_with_mcp(mcp_tools_ref)
    ).unwrap_or_default();
    let tool_def_token_count = model
        .str_to_token(&tools_json, AddBos::Never)
        .map(|t| t.len() as i32)
        .unwrap_or(0);
    log_info!(
        &conversation_id,
        "Token breakdown: system_prompt={}, tool_definitions={}",
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

        let (ctx, _skip_tokens) = evaluate_text_prompt(
            &mut state.inference_cache, model, &state.backend,
            &tokens, &conversation_id, context_size,
            offload_kqv, flash_attention, &cache_type_k, &cache_type_v,
            &config, batch_cap,
        )?;
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
        last_exec_scan_pos: 0,
        finish_reason: "stop".to_string(),
        tool_response_tokens: 0,
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
        browser_backend: &crate::web::browser::BrowserBackend::from_config(config.web_browser_backend.as_deref()),
        n_batch,
        mcp_manager: mcp_manager.clone(),
        db: db.clone(),
        backend: &state.backend,
        chat_template_string: chat_template_string.as_deref(),
    };

    // Build vision context reference for tool response image injection
    #[cfg(feature = "vision")]
    let vision_ctx_ref: VisionCtxRef<'_> = state.vision_state.as_ref().map(|v| &v.context);
    #[cfg(not(feature = "vision"))]
    let vision_ctx_ref: VisionCtxRef<'_> = ();

    run_generation_loop(
        &mut gen, &cfg, &mut context, model, &mut sampler,
        &mut batch, &token_sender, &conversation_logger, &cancel,
        vision_ctx_ref,
    )?;

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

    // Finish assistant message and persist metrics
    let was_cancelled = cancel.load(Ordering::Relaxed);
    {
        let mut logger = conversation_logger
            .lock()
            .map_err(|_| "Failed to lock conversation logger")?;
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

/// Inject a tool response with images into the model context via the vision pipeline.
///
/// Instead of plain text token injection, this tokenizes the model_block text with
/// `<__media__>` markers using MtmdContext, creates bitmaps from the raw image bytes,
/// and evaluates the combined text+image chunks into the existing context.
#[cfg(feature = "vision")]
fn inject_tool_response_with_vision(
    exec_result: &super::command_executor::CommandExecutionResult,
    mtmd_ctx: &llama_cpp_2::mtmd::MtmdContext,
    context: &mut LlamaContext<'static>,
    token_pos: &mut i32,
    n_batch: u32,
    conversation_id: &str,
) -> Result<(), String> {
    use llama_cpp_2::mtmd::{MtmdBitmap, MtmdInputText};

    let n_images = exec_result.response_images.len();
    log_info!(
        conversation_id,
        "🖼️ Vision injection: {} image(s) with model_block ({} chars)",
        n_images, exec_result.model_block.len()
    );

    // Prepend <__media__> markers (one per image) to the model block text.
    // The markers tell mtmd where to insert image embeddings in the token stream.
    let markers = "<__media__>\n".repeat(n_images);
    let text_with_markers = format!("{markers}{}", exec_result.model_block);

    // Create bitmaps from raw image bytes
    let bitmaps: Vec<MtmdBitmap> = exec_result.response_images.iter().enumerate().map(|(i, bytes)| {
        log_debug!(conversation_id, "Creating vision bitmap {} from {} bytes", i, bytes.len());
        MtmdBitmap::from_buffer(mtmd_ctx, bytes)
            .map_err(|e| format!("Failed to create image bitmap {i}: {e}"))
    }).collect::<Result<Vec<_>, String>>()?;
    let bitmap_refs: Vec<&MtmdBitmap> = bitmaps.iter().collect();

    // Tokenize text + images into chunks via MtmdContext
    let text_input = MtmdInputText {
        text: text_with_markers,
        add_special: false, // no BOS — we're mid-generation, not starting a new prompt
        parse_special: true, // parse special tokens like <|im_end|> in template wrapping
    };
    let chunks = mtmd_ctx.tokenize(text_input, &bitmap_refs)
        .map_err(|e| format!("Vision tokenization of tool response failed: {e}"))?;
    let n_chunk_tokens = chunks.total_tokens();
    log_info!(
        conversation_id,
        "Vision tokenized tool response: {} chunks, {} total tokens",
        chunks.len(), n_chunk_tokens
    );

    // Evaluate all chunks (text tokens + image embeddings) into the existing context
    let n_past = chunks.eval_chunks(mtmd_ctx, context, *token_pos, 0, n_batch as i32, true)
        .map_err(|e| format!("Vision eval_chunks for tool response failed: {e}"))?;
    log_info!(
        conversation_id,
        "Vision eval_chunks complete: token_pos {} → {}",
        *token_pos, n_past
    );
    *token_pos = n_past;

    Ok(())
}

/// Insert `<__media__>` markers into a formatted prompt, just before the
/// last occurrence of the user's message text. One marker per image tells
/// the mtmd tokenizer where each image's embeddings go in the token stream.
#[cfg(feature = "vision")]
fn inject_media_markers(prompt: &str, user_message: &str, count: usize) -> String {
    let markers = "<__media__>\n".repeat(count);
    // Find the last occurrence of the user message in the prompt
    if let Some(pos) = prompt.rfind(user_message) {
        let mut result = String::with_capacity(prompt.len() + markers.len());
        result.push_str(&prompt[..pos]);
        result.push_str(&markers);
        result.push_str(&prompt[pos..]);
        result
    } else {
        // Fallback: prepend markers to the entire prompt
        format!("{markers}{prompt}")
    }
}

/// Generate a short title for a conversation using the loaded model.
///
/// Uses a temporary context (does NOT touch the inference cache) so the
/// main conversation's KV cache is preserved. Generates up to 30 tokens
/// with temperature 0.7.
pub fn generate_title_text(
    llama_state: &SharedLlamaState,
    prompt: &str,
) -> Result<String, String> {
    let mut state_guard = llama_state
        .lock()
        .map_err(|_| "Failed to lock LLaMA state")?;
    let state = state_guard.as_mut().ok_or("No model loaded")?;
    let model = state.model.as_ref().ok_or("No model loaded")?;

    // Format a minimal [system, user] prompt using the model's chat template
    let system_msg = "Generate a concise title (3-6 words) for this conversation. Respond with ONLY the title, nothing else.";

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

    let formatted_prompt = if let Some(ref template_str) = state.chat_template_string {
        // Use Jinja template (no tools, no behavioral prompt)
        use super::jinja_templates::{apply_native_chat_template, ChatMessage};
        let messages = vec![
            ChatMessage { role: "system".into(), content: system_msg.into(), tool_calls: None },
            ChatMessage { role: "user".into(), content: prompt.into(), tool_calls: None },
        ];
        apply_native_chat_template(template_str, messages, None, None, true, &bos_text, &eos_text)
            .unwrap_or_else(|_| format!("SYSTEM:\n{system_msg}\n\nUSER:\n{prompt}\n\nASSISTANT:\n"))
    } else {
        // Simple fallback format
        format!("SYSTEM:\n{system_msg}\n\nUSER:\n{prompt}\n\nASSISTANT:\n")
    };

    // Tokenize
    let tokens = model
        .str_to_token(&formatted_prompt, AddBos::Never)
        .map_err(|e| format!("Title tokenization failed: {e}"))?;

    eprintln!("[WORKER] Title generation: {} prompt tokens", tokens.len());

    // Create a temporary context (small size, doesn't touch inference_cache)
    let title_ctx_size = 2048u32;
    let n_ctx = NonZeroU32::new(title_ctx_size).unwrap();
    let offload_kqv = state.gpu_layers.unwrap_or(0) > 0;
    let config = SamplerConfig::default();
    let mut ctx = create_fresh_context(model, &state.backend, n_ctx, offload_kqv, &config)?;

    // Evaluate prompt tokens in batches
    let batch_cap = 512usize;
    let n_chunks = tokens.len().div_ceil(batch_cap);
    let mut batch = LlamaBatch::new(batch_cap, 1);
    for chunk_idx in 0..n_chunks {
        let start = chunk_idx * batch_cap;
        let end = std::cmp::min(start + batch_cap, tokens.len());
        batch.clear();
        for (offset, &token) in tokens[start..end].iter().enumerate() {
            let pos = (start + offset) as i32;
            let is_last = start + offset == tokens.len() - 1;
            batch.add(token, pos, &[0], is_last)
                .map_err(|e| format!("Title batch add failed: {e}"))?;
        }
        ctx.decode(&mut batch)
            .map_err(|e| format!("Title prompt decode failed: {e}"))?;
    }

    // Create a simple sampler: temp(0.7) + dist
    let mut sampler = LlamaSampler::chain_simple(vec![
        LlamaSampler::temp(0.7),
        LlamaSampler::dist(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u32)
                .unwrap_or(42),
        ),
    ]);

    // Generate up to 30 tokens
    let mut title = String::new();
    let mut token_pos = tokens.len() as i32;
    let eos_token = model.token_eos();

    for _ in 0..30 {
        let next_token = sampler.sample(&ctx, -1);
        if next_token == eos_token {
            break;
        }

        #[allow(deprecated)]
        let token_str = model
            .token_to_str(next_token, Special::Tokenize)
            .unwrap_or_default();

        // Stop on newlines (title should be single line)
        if token_str.contains('\n') {
            break;
        }

        title.push_str(&token_str);

        batch.clear();
        batch.add(next_token, token_pos, &[0], true)
            .map_err(|e| format!("Title gen batch add failed: {e}"))?;
        ctx.decode(&mut batch)
            .map_err(|e| format!("Title gen decode failed: {e}"))?;
        token_pos += 1;
    }

    // Drop the temporary context (inference_cache untouched)
    drop(ctx);
    drop(state_guard);

    let result = title.trim().to_string();
    eprintln!("[WORKER] Title generated: {:?}", result);
    Ok(result)
}
