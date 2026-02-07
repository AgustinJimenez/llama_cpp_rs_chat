use llama_cpp_2::{
    context::params::LlamaContextParams,
    context::LlamaContext,
    llama_batch::LlamaBatch,
    model::{AddBos, Special},
    sampling::LlamaSampler,
    token::LlamaToken,
};
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

use super::super::config::load_config;
use super::super::database::Database;
use super::super::model_manager::load_model;
use super::super::models::*;
use super::command_executor::{
    check_and_execute_command_with_tags, inject_output_tokens, stream_command_output,
};
use super::stop_conditions::check_stop_conditions;
use super::templates::apply_system_prompt_by_type_with_tags;
use super::tool_tags::get_tool_tags_for_model;
use crate::{log_debug, log_info, log_warn, sys_debug, sys_error, sys_warn};

// Constants for LLaMA configuration
const CONTEXT_SIZE: u32 = 32768;
const MODEL_PATH: &str =
    "/app/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf";

/// Create a sampler based on the configuration
fn create_sampler(config: &SamplerConfig, conversation_id: &str) -> LlamaSampler {
    match config.sampler_type.as_str() {
        "Temperature" => {
            log_info!(
                conversation_id,
                "Using Temperature sampler: temp={}, top_p={}, top_k={}",
                config.temperature,
                config.top_p,
                config.top_k
            );
            // Chain: temp → top_k → top_p → dist (must end with a terminal sampler)
            LlamaSampler::chain_simple([
                LlamaSampler::temp(config.temperature as f32),
                LlamaSampler::top_k(config.top_k as i32),
                LlamaSampler::top_p(config.top_p as f32, 1),
                LlamaSampler::dist(1234),
            ])
        }
        "Mirostat" => {
            log_info!(
                conversation_id,
                "Using Mirostat sampler: tau={}, eta={}",
                config.mirostat_tau,
                config.mirostat_eta
            );
            LlamaSampler::mirostat(
                0,    // n_vocab
                1234, // seed
                config.mirostat_tau as f32,
                config.mirostat_eta as f32,
                100, // m
            )
        }
        _ => {
            log_info!(conversation_id, "Using Greedy sampler (default)");
            LlamaSampler::greedy()
        }
    }
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
    db: &Database,
    cancel: Arc<AtomicBool>,
) -> Result<(String, i32, i32), String> {
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
    let config = load_config(db);
    let model_path = config.model_path.as_deref().unwrap_or(MODEL_PATH);
    let stop_tokens = config
        .stop_tokens
        .clone()
        .unwrap_or_else(get_common_stop_tokens);

    // Ensure model is loaded
    load_model(llama_state.clone(), model_path).await?;

    // Now use the shared state for generation (mutable for inference cache)
    let mut state_guard = llama_state
        .lock()
        .map_err(|_| "Failed to lock LLaMA state")?;
    let state = state_guard.as_mut().ok_or("LLaMA state not initialized")?;
    let model = state.model.as_ref().ok_or("No model loaded")?;

    // Get context size: prefer user config, fallback to model's context_length, then default
    let requested_context_size = config
        .context_size
        .or(state.model_context_length)
        .unwrap_or(CONTEXT_SIZE);

    let context_size = requested_context_size;

    log_info!(
        &conversation_id,
        "Using context size: {} (model max: {:?})",
        context_size,
        state.model_context_length
    );

    // Create sampler based on configuration
    let mut sampler = create_sampler(&config, &conversation_id);

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

    // Look up model-specific tool tags based on general.name from GGUF metadata
    let tags = get_tool_tags_for_model(general_name.as_deref());
    log_info!(&conversation_id, "=== TEMPLATE DEBUG ===");
    log_info!(&conversation_id, "Template type: {:?}", template_type);
    log_info!(&conversation_id, "General name: {:?}", general_name);
    log_info!(&conversation_id, "Tool tags: exec_open={}, exec_close={}", tags.exec_open, tags.exec_close);
    log_info!(&conversation_id, "System prompt type: {:?}", config.system_prompt_type);
    log_info!(
        &conversation_id,
        "Conversation content:\n{}",
        conversation_content
    );

    // Use the 3-system prompt dispatcher with model-specific tool tags
    let prompt = apply_system_prompt_by_type_with_tags(
        &conversation_content,
        config.system_prompt_type.clone(),
        template_type.as_deref(),
        chat_template_string.as_deref(),
        config.system_prompt.as_deref(),
        tags,
    )?;
    log_info!(&conversation_id, "=== FINAL PROMPT BEING SENT TO MODEL ===");
    log_info!(&conversation_id, "{}", prompt);
    log_info!(
        &conversation_id,
        "=== END PROMPT (length: {} chars) ===",
        prompt.len()
    );

    // Tokenize
    log_debug!(&conversation_id, "Step 2: Starting tokenization...");
    let tokens = model
        .str_to_token(&prompt, AddBos::Never)
        .map_err(|e| format!("Tokenization failed: {e}"))?;
    log_debug!(
        &conversation_id,
        "Step 2 complete: Tokenized to {} tokens",
        tokens.len()
    );

    // Context parameters
    let n_ctx = NonZeroU32::new(context_size).expect("Context size must be non-zero");
    let offload_kqv = state.gpu_layers.unwrap_or(0) > 0;
    if offload_kqv {
        log_info!(
            &conversation_id,
            "⚡ KV cache on GPU ({} layers offloaded)",
            state.gpu_layers.unwrap_or(0)
        );
    }

    // Try to reuse cached inference context for KV cache reuse
    let cached = state.inference_cache.take();
    let (mut context, skip_tokens) = match cached {
        Some(cache)
            if cache.conversation_id == conversation_id
                && cache.context_size == context_size
                && cache.offload_kqv == offload_kqv =>
        {
            // Cache hit: find common prefix between cached and new tokens
            let common_len = cache
                .evaluated_tokens
                .iter()
                .zip(tokens.iter())
                .take_while(|(a, b)| a == b)
                .count();

            let mut ctx = cache.context;

            if common_len < cache.evaluated_tokens.len() {
                // Conversation diverged (e.g., message edited/deleted).
                // Clear KV cache entries from the divergence point onward.
                log_info!(
                    &conversation_id,
                    "KV cache diverged at token {}, clearing {} stale entries",
                    common_len,
                    cache.evaluated_tokens.len() - common_len
                );
                let _ = ctx.clear_kv_cache_seq(
                    Some(0),
                    Some(common_len as u32),
                    None,
                );
            }

            log_info!(
                &conversation_id,
                "♻️ Reusing KV cache: {} of {} prompt tokens already evaluated, {} new",
                common_len,
                tokens.len(),
                tokens.len() - common_len
            );
            (ctx, common_len)
        }
        _ => {
            // Cache miss: create fresh context
            drop(cached);
            log_debug!(
                &conversation_id,
                "Step 3: Creating fresh context (size={}K tokens)...",
                context_size / 1024
            );
            let ctx_params = LlamaContextParams::default()
                .with_n_ctx(Some(n_ctx))
                .with_offload_kqv(offload_kqv);

            // SAFETY: We erase the lifetime to 'static so the context can be stored
            // in InferenceCache. The model MUST outlive the context — enforced by
            // clearing inference_cache before any model drop in model_manager.rs.
            let ctx = unsafe {
                let real_ctx = model
                    .new_context(&state.backend, ctx_params)
                    .map_err(|e| format!("Context creation failed: {e}"))?;
                std::mem::transmute::<LlamaContext<'_>, LlamaContext<'static>>(real_ctx)
            };
            log_debug!(&conversation_id, "Step 3 complete: Fresh context created");
            (ctx, 0)
        }
    };

    // Evaluate only the NEW tokens (skip those already in KV cache)
    let new_tokens = &tokens[skip_tokens..];
    const PROMPT_BATCH_CAP: usize = 2048;
    let prompt_tokens = tokens.len();
    let batch_cap = PROMPT_BATCH_CAP;

    // Check cancellation before expensive prompt decode
    if cancel.load(Ordering::Relaxed) {
        return Err("Cancelled".to_string());
    }

    if !new_tokens.is_empty() {
        let new_chunks = new_tokens.len().div_ceil(batch_cap);
        log_debug!(
            &conversation_id,
            "Step 5: Decoding {} new prompt tokens in {} chunks (skipped {})...",
            new_tokens.len(),
            new_chunks,
            skip_tokens
        );

        let mut batch = LlamaBatch::new(batch_cap, 1);
        for chunk_idx in 0..new_chunks {
            let start = chunk_idx * batch_cap;
            let end = std::cmp::min(start + batch_cap, new_tokens.len());

            batch.clear();
            for (offset, &token) in new_tokens[start..end].iter().enumerate() {
                let pos = skip_tokens + start + offset;
                let is_last = pos == prompt_tokens - 1;
                batch
                    .add(token, pos as i32, &[0], is_last)
                    .map_err(|e| format!("Batch add failed at prompt token {pos}: {e}"))?;
            }

            context.decode(&mut batch).map_err(|e| {
                format!(
                    "Prompt decode failed (chunk {}/{}): {}",
                    chunk_idx + 1,
                    new_chunks,
                    e
                )
            })?;
        }
        log_debug!(
            &conversation_id,
            "Step 5 complete: Prompt decode successful"
        );
    } else {
        log_info!(
            &conversation_id,
            "Step 5: All {} prompt tokens already in KV cache, skipping decode",
            prompt_tokens
        );
    }

    let mut batch = LlamaBatch::new(batch_cap, 1);

    // Start assistant message in conversation log (enables streaming broadcast)
    {
        let mut logger = conversation_logger
            .lock()
            .map_err(|_| "Failed to lock conversation logger")?;
        logger.start_assistant_message();
    }

    // Generate response
    let mut response = String::new();
    let mut token_pos = tokens.len() as i32;
    let mut total_tokens_generated = 0;
    let mut generated_token_ids: Vec<LlamaToken> = Vec::new();

    // Calculate max tokens based on remaining context space
    let remaining_context = (context_size as i32) - token_pos - 128;
    let max_total_tokens = remaining_context.max(512);

    log_info!(
        &conversation_id,
        "Context size: {}, Prompt tokens: {}, Max tokens to generate: {}",
        context_size,
        token_pos,
        max_total_tokens
    );

    sys_debug!(
        "[GENERATION] About to start token generation loop. token_sender is {}",
        if token_sender.is_some() {
            "SOME"
        } else {
            "NONE"
        }
    );

    // Track position in response after last executed command to prevent re-matching
    let mut last_exec_scan_pos: usize = 0;

    // Outer loop to handle command execution and continuation
    loop {
        let mut command_executed = false;
        let mut hit_stop_condition = false;

        // Inner loop for token generation
        let tokens_to_generate = std::cmp::min(2048, max_total_tokens - total_tokens_generated);

        log_debug!(
            &conversation_id,
            "Starting generation cycle: tokens_to_generate={}, total_tokens_generated={}",
            tokens_to_generate,
            total_tokens_generated
        );

        for i in 0..tokens_to_generate {
            // Check cancellation every 4 tokens
            if i % 4 == 0 && cancel.load(Ordering::Relaxed) {
                log_info!(&conversation_id, "Generation cancelled by user");
                hit_stop_condition = true;
                break;
            }

            // Sample next token
            if i % 50 == 0 {
                log_debug!(
                    &conversation_id,
                    "Generated {} tokens so far...",
                    total_tokens_generated
                );
            }

            // Extra logging around the 150 token mark
            if (145..=155).contains(&total_tokens_generated) {
                log_debug!(
                    &conversation_id,
                    "Token #{}: About to sample...",
                    total_tokens_generated
                );
            }

            let next_token = sampler.sample(&context, -1);

            if (145..=155).contains(&total_tokens_generated) {
                log_debug!(
                    &conversation_id,
                    "Token #{}: Sampled token ID {}",
                    total_tokens_generated,
                    next_token
                );
            }

            // Check for end-of-sequence token
            if next_token == model.token_eos() {
                log_debug!(
                    &conversation_id,
                    "Stopping generation - EOS token detected (token ID: {}) at position {}",
                    next_token,
                    total_tokens_generated
                );
                hit_stop_condition = true;
                break;
            }

            // Add token to batch and decode
            if (145..=155).contains(&total_tokens_generated) {
                log_debug!(
                    &conversation_id,
                    "Token #{}: About to add to batch and decode...",
                    total_tokens_generated
                );
            }

            batch.clear();
            batch.add(next_token, token_pos, &[0], true).map_err(|e| {
                format!(
                    "Batch add failed at token {total_tokens_generated}: {e}"
                )
            })?;

            context
                .decode(&mut batch)
                .map_err(|e| format!("Decode failed at token {total_tokens_generated}: {e}"))?;

            if (145..=155).contains(&total_tokens_generated) {
                log_debug!(
                    &conversation_id,
                    "Token #{}: Decode successful",
                    total_tokens_generated
                );
            }

            token_pos += 1;
            total_tokens_generated += 1;
            generated_token_ids.push(next_token);

            // Convert to string for display
            let token_str = match model.token_to_str(next_token, Special::Tokenize) {
                Ok(s) => s,
                Err(e) => {
                    log_warn!(
                        &conversation_id,
                        "Token {} can't be displayed as UTF-8: {}. Continuing generation.",
                        next_token,
                        e
                    );
                    continue;
                }
            };

            if (145..=155).contains(&total_tokens_generated) {
                log_debug!(
                    &conversation_id,
                    "Token #{}: Converted to string: {:?}",
                    total_tokens_generated,
                    token_str
                );
            }

            // Check for stop sequences using helper function
            if (145..=155).contains(&total_tokens_generated) {
                log_debug!(
                    &conversation_id,
                    "Token #{}: Checking {} stop tokens...",
                    total_tokens_generated,
                    stop_tokens.len()
                );
            }

            let stop_result = check_stop_conditions(&response, &token_str, &stop_tokens);

            if stop_result.should_stop {
                let partial_to_remove = stop_result.partial_to_remove;
                if let Some(stop_token) = stop_result.matched_token.as_deref() {
                    log_debug!(
                        &conversation_id,
                        "Stopping generation due to stop token {:?} (remove {} chars)",
                        stop_token,
                        partial_to_remove
                    );
                }
                if (145..=155).contains(&total_tokens_generated) {
                    log_debug!(
                        &conversation_id,
                        "Token #{}: Should stop = true, breaking loop",
                        total_tokens_generated
                    );
                }
                if partial_to_remove > 0 {
                    let new_len = response.len().saturating_sub(partial_to_remove);
                    response.truncate(new_len);
                }
                hit_stop_condition = true;
                break;
            }

            if (145..=155).contains(&total_tokens_generated) {
                log_debug!(
                    &conversation_id,
                    "Token #{}: No stop condition, adding token to response",
                    total_tokens_generated
                );
            }

            // Add token to response
            response.push_str(&token_str);

            // Stream token
            if let Some(ref sender) = token_sender {
                let token_data = TokenData {
                    token: token_str.clone(),
                    tokens_used: token_pos,
                    max_tokens: context_size as i32,
                };
                match sender.send(token_data) {
                    Ok(()) => {
                        if total_tokens_generated <= 5 || total_tokens_generated % 50 == 0 {
                            sys_debug!(
                                "[GENERATION] Token #{} sent via channel: {:?}",
                                total_tokens_generated,
                                token_str.chars().take(20).collect::<String>()
                            );
                        }
                    }
                    Err(e) => {
                        sys_error!(
                            "[GENERATION] ERROR: Failed to send token #{} via channel: {}",
                            total_tokens_generated,
                            e
                        );
                    }
                }
            } else if total_tokens_generated == 1 {
                sys_warn!(
                    "[GENERATION] WARNING: token_sender is None, tokens not being streamed!"
                );
            }

            // Log token (with current token counts for WebSocket watchers)
            {
                let mut logger = conversation_logger
                    .lock()
                    .map_err(|_| "Failed to lock conversation logger")?;
                logger.set_token_counts(token_pos, context_size as i32);
                logger.log_token(&token_str);
            }
        }

        // Check for and execute any commands in the response (using model-specific tags)
        if let Some(exec_result) =
            check_and_execute_command_with_tags(&response, last_exec_scan_pos, &conversation_id, model, tags)?
        {
            // 1. Log to conversation file (CRITICAL: prevents infinite loops)
            {
                let mut logger = conversation_logger
                    .lock()
                    .map_err(|_| "Failed to lock conversation logger")?;
                logger.log_token(&exec_result.output_block);
            }

            // 2. Add to response string
            response.push_str(&exec_result.output_block);

            // 3. Stream to frontend
            stream_command_output(
                &exec_result.output_block,
                &token_sender,
                token_pos,
                context_size,
            );

            // 4. Inject output tokens into LLM context
            inject_output_tokens(
                &exec_result.output_tokens,
                &mut batch,
                &mut context,
                &mut token_pos,
                &conversation_id,
            )?;

            generated_token_ids.extend(exec_result.output_tokens.iter().map(|&id| LlamaToken(id)));
            command_executed = true;
            // CRITICAL: Reset stop condition so generation continues after command output
            hit_stop_condition = false;
            // Update scan position to end of response (past the injected output)
            last_exec_scan_pos = response.len();
            log_info!(
                &conversation_id,
                "✅ Command executed, output injected, scan position updated to {}",
                last_exec_scan_pos
            );
        }

        // Break conditions
        if hit_stop_condition || total_tokens_generated >= max_total_tokens {
            log_debug!(&conversation_id, "Breaking generation loop:");
            log_debug!(
                &conversation_id,
                "  hit_stop_condition: {}",
                hit_stop_condition
            );
            log_debug!(
                &conversation_id,
                "  total_tokens_generated: {}",
                total_tokens_generated
            );
            log_debug!(&conversation_id, "  max_total_tokens: {}", max_total_tokens);
            log_debug!(
                &conversation_id,
                "  Reached max? {}",
                total_tokens_generated >= max_total_tokens
            );
            break;
        }

        if !command_executed {
            log_debug!(
                &conversation_id,
                "Continuing generation: no stop condition hit"
            );
        }
    }

    log_debug!(&conversation_id, "Exited generation loop. Final stats:");
    log_debug!(
        &conversation_id,
        "  Total tokens generated: {}",
        total_tokens_generated
    );
    log_debug!(
        &conversation_id,
        "  Response length: {} chars",
        response.len()
    );

    // Finish assistant message
    let was_cancelled = cancel.load(Ordering::Relaxed);
    {
        let mut logger = conversation_logger
            .lock()
            .map_err(|_| "Failed to lock conversation logger")?;
        logger.finish_assistant_message();
        if was_cancelled {
            logger.log_message("system", "[Generation stopped by user]");
        }
    }

    // Store context back into inference cache for KV cache reuse on next turn
    let total_cached = tokens.len() + generated_token_ids.len();
    let gen_count = generated_token_ids.len();
    let mut all_evaluated = tokens;
    all_evaluated.extend(generated_token_ids);
    state.inference_cache = Some(InferenceCache {
        context,
        conversation_id: conversation_id.clone(),
        evaluated_tokens: all_evaluated,
        context_size,
        offload_kqv,
    });
    log_info!(
        &conversation_id,
        "Stored KV cache: {} total tokens ({} generated this turn)",
        total_cached,
        gen_count
    );

    Ok((response.trim().to_string(), token_pos, max_total_tokens))
}
