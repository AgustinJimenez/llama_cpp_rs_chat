
use llama_cpp_2::{
    context::LlamaContext,
    llama_batch::LlamaBatch,
    model::LlamaModel,
    sampling::LlamaSampler,
    token::LlamaToken,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

use llama_chat_types::*;
use crate::SharedConversationLogger;
use super::command_executor::{
    check_and_execute_command_with_tags, inject_output_tokens,
};
use super::stop_conditions::{check_stop_conditions, ExecBlockTracker};
use llama_chat_db::event_log::log_event;

#[path = "token_loop/shared.rs"]
mod shared;
pub(crate) use shared::{
    detect_repetition_loop, TokenGenConfig, TokenGenState, VisionCtxRef,
    REPETITION_CHECK_INTERVAL, REPETITION_CHECK_MIN_TOKENS, TOKEN_STALL_TIMEOUT,
};

#[path = "token_loop/watchdog.rs"]
mod watchdog;
use watchdog::WatchdogHandles;

/// Run the outer generation loop: generates tokens, detects/executes commands, resumes.
///
/// Returns the final response and token counts. The `context` is mutated in place
/// and should be stored back into the inference cache after this returns.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_generation_loop(
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

    let mut stall_checkpoint = Instant::now();
    let gen_start_time = Instant::now();

    let watchdog = WatchdogHandles::spawn(cancel.clone(), cfg.conversation_id.to_string());

    loop {
        let mut command_executed = false;
        let mut hit_stop_condition = false;
        let tokens_to_generate = std::cmp::min(2048, cfg.max_total_tokens - gen.total_tokens_generated);

        log_debug!(
            cfg.conversation_id,
            "Starting generation cycle: tokens_to_generate={}, total_tokens_generated={}",
            tokens_to_generate, gen.total_tokens_generated
        );

        'token: for i in 0..tokens_to_generate {
            if cancel.load(Ordering::Relaxed) {
                log_info!(cfg.conversation_id, "Generation cancelled by user");
                gen.finish_reason = "cancelled".to_string();
                hit_stop_condition = true;
                break 'token;
            }

            if i % 50 == 0 {
                log_debug!(cfg.conversation_id, "Generated {} tokens so far...", gen.total_tokens_generated);
            }

            // Stall detection: amortized check every 16 tokens (saves 2 syscalls/token)
            if i & 15 == 15 {
                let batch_elapsed = stall_checkpoint.elapsed();
                if batch_elapsed > TOKEN_STALL_TIMEOUT {
                    let secs = batch_elapsed.as_secs();
                    eprintln!("[STALL] Generation stalled: 16 tokens took {}s (loop_recoveries={})", secs, gen.loop_recoveries);
                    log_event(cfg.conversation_id, "stall", &format!("16 tokens took {}s", secs));
                    if gen.loop_recoveries < 1 {
                        gen.loop_recoveries += 1;
                        if let Some(ref sender) = token_sender {
                            let _ = sender.send(TokenData {
                                token: "\n\n[Generation stalled — retrying with different approach]".to_string(),
                                tokens_used: gen.total_tokens_generated,
                                max_tokens: cfg.max_total_tokens, status: None,
                                ..Default::default()
                            });
                        }
                        gen.finish_reason = "loop_recovery".to_string();
                    } else {
                        if let Some(ref sender) = token_sender {
                            let _ = sender.send(TokenData {
                                token: format!(
                                    "\n\n[Generation stalled — batch of 16 tokens took {}s. The model may be too large for your hardware.]",
                                    secs
                                ),
                                tokens_used: gen.total_tokens_generated,
                                max_tokens: cfg.max_total_tokens, status: None,
                                ..Default::default()
                            });
                        }
                        gen.finish_reason = "error".to_string();
                    }
                    hit_stop_condition = true;
                    break 'token;
                }
                stall_checkpoint = Instant::now();
            }

            // Wall-clock stall check BEFORE sample() — catches GPU hangs where `i` never increments.
            if stall_checkpoint.elapsed() > TOKEN_STALL_TIMEOUT {
                let secs = stall_checkpoint.elapsed().as_secs();
                eprintln!("[STALL] Pre-sample stall: no progress for {}s", secs);
                log_event(cfg.conversation_id, "stall", &format!("Pre-sample stall: {}s", secs));
                if let Some(ref sender) = token_sender {
                    let _ = sender.send(TokenData {
                        token: format!("\n\n[Generation stalled — no token produced for {}s]", secs),
                        tokens_used: gen.total_tokens_generated,
                        max_tokens: cfg.max_total_tokens, status: None,
                        ..Default::default()
                    });
                }
                gen.finish_reason = "error".to_string();
                hit_stop_condition = true;
                break 'token;
            }

            if i == 0 || gen.total_tokens_generated % 100 == 0 {
                log_debug!(cfg.conversation_id, "Sampling token {} (i={}) ...", gen.total_tokens_generated, i);
            }

            // Safety check: verify logits exist before sampling.
            let logits = context.get_logits();
            if logits.is_empty() {
                eprintln!("[FATAL] No logits available before sample() at token {}! n_outputs=0. Aborting generation.", gen.total_tokens_generated);
                log_event(cfg.conversation_id, "logits_empty", &format!("No logits at token {}", gen.total_tokens_generated));
                gen.finish_reason = "error".to_string();
                if let Some(ref sender) = token_sender {
                    let _ = sender.send(TokenData {
                        token: "\n\n[Error: No logits available — context may be corrupted. Please retry.]".to_string(),
                        tokens_used: gen.total_tokens_generated,
                        max_tokens: cfg.max_total_tokens, status: None,
                        ..Default::default()
                    });
                }
                hit_stop_condition = true;
                break 'token;
            }
            let next_token = sampler.sample(context, -1);
            watchdog.ping();
            stall_checkpoint = Instant::now();

            // Check if sample() timed out (CUDA sync deadlock detected by safe wrapper)
            if next_token == LlamaToken(-1) {
                extern "C" { fn llama_decode_safe_get_error() -> *const std::ffi::c_char; }
                let err = unsafe {
                    let ptr = llama_decode_safe_get_error();
                    if !ptr.is_null() { std::ffi::CStr::from_ptr(ptr).to_string_lossy().to_string() } else { String::new() }
                };
                if err.contains("timed out") {
                    eprintln!("[SAMPLE] CUDA sync deadlock detected — stopping generation gracefully");
                    log_event(cfg.conversation_id, "cuda_deadlock", &format!("sample() timeout at token {}", gen.total_tokens_generated));
                    gen.finish_reason = "cuda_deadlock".to_string();
                    if let Some(ref sender) = token_sender {
                        let _ = sender.send(TokenData {
                            token: "\n\n[CUDA sync issue — generation will continue automatically]".to_string(),
                            tokens_used: gen.total_tokens_generated,
                            max_tokens: cfg.max_total_tokens, status: None,
                            ..Default::default()
                        });
                    }
                    hit_stop_condition = true;
                    break 'token;
                }
            }

            if cancel.load(Ordering::Relaxed) {
                eprintln!("[WATCHDOG] Cancel detected after sample() returned — aborting generation");
                gen.finish_reason = "watchdog".to_string();
                if let Some(ref sender) = token_sender {
                    let _ = sender.send(TokenData {
                        token: "\n\n[Generation stalled — restarting]".to_string(),
                        tokens_used: gen.total_tokens_generated,
                        max_tokens: cfg.max_total_tokens, status: None,
                        ..Default::default()
                    });
                }
                hit_stop_condition = true;
                break 'token;
            }

            if next_token == model.token_eos() {
                log_debug!(
                    cfg.conversation_id,
                    "EOS token detected at position {} (in_exec_block: {})",
                    gen.total_tokens_generated, gen.exec_tracker.is_inside()
                );

                // Inline EOS interception: for agentic turns (tool calls were made),
                // ask the model if the response is complete. If not, it returns the
                // next few tokens directly — we inject those and continue seamlessly.
                // Cap at 3 retries to avoid infinite loops.
                if gen.tool_response_tokens > 0 && gen.eos_continue_count < 3 {
                    let check = super::sub_checks::inline_eos_probe(
                        model, context,
                        gen.token_pos, cfg.conversation_id,
                    );

                    if !check.is_complete && !check.continuation_tokens.is_empty() {
                        gen.eos_continue_count += 1;

                        // Push continuation text to response and stream it
                        gen.response.push_str(&check.continuation_text);
                        if let Some(ref sender) = token_sender {
                            let _ = sender.send(TokenData {
                                token: check.continuation_text.clone(),
                                tokens_used: gen.token_pos,
                                max_tokens: cfg.context_size as i32,
                                ..Default::default()
                            });
                        }

                        // Inject continuation tokens into the main KV cache
                        let mut injection_ok = true;
                        for &cont_tok in &check.continuation_tokens {
                            batch.clear();
                            if batch.add(cont_tok, gen.token_pos, &[0], true).is_err() {
                                injection_ok = false; break;
                            }
                            if context.decode(batch).is_err() {
                                injection_ok = false; break;
                            }
                            gen.token_pos += 1;
                            gen.total_tokens_generated += 1;
                        }

                        if injection_ok {
                            watchdog.ping();
                            stall_checkpoint = Instant::now();
                            continue 'token; // resume generation from continuation
                        }
                        // If injection failed, fall through and accept EOS below
                        log_event(cfg.conversation_id, "eos_inject_failed", "KV injection error — accepting EOS");
                    }
                } else if gen.eos_continue_count >= 3 {
                    log_event(cfg.conversation_id, "eos_accept", &format!(
                        "max retries ({}) reached", gen.eos_continue_count
                    ));
                    log_info!(cfg.conversation_id, "⚠️ EOS accepted: max continuation retries reached");
                }

                // Accept EOS — end generation
                #[allow(deprecated)]
                if let Ok(eos_str) = model.token_to_str(next_token, Special::Tokenize) {
                    gen.response.push_str(&eos_str);
                    if let Some(ref sender) = token_sender {
                        let _ = sender.send(TokenData {
                            token: eos_str,
                            tokens_used: gen.token_pos,
                            max_tokens: cfg.context_size as i32, status: None,
                            ..Default::default()
                        });
                    }
                }
                hit_stop_condition = true;
                break 'token;
            }

            batch.clear();
            batch.add(next_token, gen.token_pos, &[0], true)
                .map_err(|e| format!("Batch add failed at token {}: {e}", gen.total_tokens_generated))?;
            if i == 0 { log_debug!(cfg.conversation_id, "Decoding first token after cycle start..."); }
            if let Err(e) = context.decode(batch) {
                let err_str = format!("{e}");
                if err_str.contains("NoKvCacheSlot") || err_str.contains("no kv cache slot") {
                    eprintln!("[CTX_GUARD] NoKvCacheSlot at token {} — treating as finish_reason=length", gen.total_tokens_generated);
                    log_event(cfg.conversation_id, "context_guard", &format!("NoKvCacheSlot at token {}", gen.total_tokens_generated));
                    gen.finish_reason = "length".to_string();
                    hit_stop_condition = true;
                    break 'token;
                }
                if err_str.contains("Unknown(2)") || cancel.load(Ordering::Relaxed) {
                    log_info!(cfg.conversation_id, "Decode aborted by cancel callback at token {}", gen.total_tokens_generated);
                    gen.finish_reason = "cancelled".to_string();
                    hit_stop_condition = true;
                    break 'token;
                }
                return Err(format!("Decode failed at token {}: {e}", gen.total_tokens_generated));
            }

            watchdog.ping();

            // Log generated token for crash reproduction
            if let Ok(dump_dir) = std::env::var("LLAMA_CHAT_DATA_DIR") {
                let dump_path = format!("{}/logs/last_gen_tokens.txt", dump_dir);
                let entry = format!("{}\n", next_token.0);
                let _ = std::fs::OpenOptions::new().create(true).append(true).open(&dump_path)
                    .and_then(|mut f| std::io::Write::write_all(&mut f, entry.as_bytes()));
            }

            gen.token_pos += 1;
            gen.total_tokens_generated += 1;
            gen.generated_token_ids.push(next_token);

            // Context position guard: stop at 95% full
            let ctx_limit = cfg.context_size.saturating_sub(cfg.context_size / 20);
            if gen.token_pos as u32 >= ctx_limit {
                eprintln!("[CTX_GUARD] Context 95% full ({}/{}, limit={}) — stopping with finish_reason=length", gen.token_pos, cfg.context_size, ctx_limit);
                log_event(cfg.conversation_id, "context_guard", &format!("Context 95% full ({}/{})", gen.token_pos, cfg.context_size));
                gen.finish_reason = "length".to_string();
                hit_stop_condition = true;
                break 'token;
            }

            #[allow(deprecated)]
            let token_str = match model.token_to_str(next_token, Special::Tokenize) {
                Ok(s) => s,
                Err(e) => {
                    log_warn!(cfg.conversation_id, "Token {} can't be displayed: {}. Continuing.", next_token, e);
                    continue 'token;
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
                break 'token;
            }

            gen.response.push_str(&token_str);
            gen.exec_tracker.update(&token_str, gen.response.len());

            // Periodic repetition loop detection
            if gen.total_tokens_generated > REPETITION_CHECK_MIN_TOKENS
                && gen.total_tokens_generated % REPETITION_CHECK_INTERVAL == 0
                && detect_repetition_loop(&gen.response)
            {
                eprintln!("[LOOP_RECOVERY] Repetition loop detected at token {}, loop_recoveries={}", gen.total_tokens_generated, gen.loop_recoveries);
                log_event(cfg.conversation_id, "loop_recovery", &format!("Repetition loop at token {}", gen.total_tokens_generated));
                if gen.loop_recoveries < 1 {
                    gen.loop_recoveries += 1;
                    if let Some(ref sender) = token_sender {
                        let _ = sender.send(TokenData {
                            token: "\n\n[Repetition detected — retrying with different approach]".to_string(),
                            tokens_used: gen.token_pos,
                            max_tokens: cfg.context_size as i32, status: None,
                            ..Default::default()
                        });
                    }
                    gen.finish_reason = "loop_recovery".to_string();
                    hit_stop_condition = true;
                    break 'token;
                } else {
                    if let Some(ref sender) = token_sender {
                        let _ = sender.send(TokenData {
                            token: "\n\n[Generation stopped: repetition loop persists after recovery attempt]".to_string(),
                            tokens_used: gen.token_pos,
                            max_tokens: cfg.context_size as i32, status: None,
                            ..Default::default()
                        });
                    }
                    gen.finish_reason = "error".to_string();
                    hit_stop_condition = true;
                    break 'token;
                }
            }

            // Stream token to frontend with live tok/s
            if let Some(ref sender) = token_sender {
                let elapsed_secs = gen_start_time.elapsed().as_secs_f64();
                let live_tok_per_sec = if elapsed_secs > 0.1 {
                    Some(gen.total_tokens_generated as f64 / elapsed_secs)
                } else {
                    None
                };
                let _ = sender.send(TokenData {
                    token: token_str.clone(),
                    tokens_used: gen.token_pos,
                    max_tokens: cfg.context_size as i32,
                    gen_tok_per_sec: live_tok_per_sec,
                    gen_tokens: Some(gen.total_tokens_generated),
                    ..Default::default()
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

            // Check for and execute tool calls in the response.
            // Fast gate: only call the expensive detector when the new token
            // contains a character that could close a tool call block.
            let token_has_close_char = token_str.as_bytes().iter().any(|&b| b == b'>' || b == b']' || b == b'}');
            if token_has_close_char {
                watchdog.pause();
                let tool_check_result = check_and_execute_command_with_tags(
                    &gen.response, gen.last_exec_scan_pos, cfg.conversation_id, model, cfg.tags,
                    cfg.template_type,
                    &mut gen.recent_commands, &mut gen.consecutive_loop_blocks, token_sender, gen.token_pos, cfg.context_size,
                    Some(cancel.clone()), cfg.use_htmd, cfg.browser_backend,
                    cfg.mcp_manager.clone(), cfg.db.clone(),
                    cfg.backend, cfg.chat_template_string,
                );
                watchdog.resume();
                watchdog.ping();

                if let Some(mut exec_result) = tool_check_result? {
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

                    gen.tool_response_tokens += exec_result.model_tokens.len() as i32;
                    command_executed = true;

                    // Image summarization: if the agent requested a description (summary=<prompt>),
                    // run a vision sub-pass and inject the text description instead of raw images.
                    // Falls back to an informational hint when no vision model is loaded.
                    if !exec_result.response_images.is_empty() {
                        if let Some(prompt) = exec_result.image_summary_prompt.take() {
                            #[cfg(feature = "vision")]
                            if let Some(mtmd_ctx) = vision_ctx {
                                match super::tool_output::run_image_vision_summary(
                                    model, cfg.backend, mtmd_ctx,
                                    &exec_result.response_images, &prompt,
                                    cfg.conversation_id,
                                ) {
                                    Ok(description) => {
                                        eprintln!(
                                            "[IMAGE_SUMMARY] Described {} image(s): {} chars",
                                            exec_result.response_images.len(), description.len()
                                        );
                                        exec_result.response_images.clear();
                                        let suffix = format!("\n[Image content: {}]", description);
                                        if let Ok(suffix_tokens) = model.str_to_token(&suffix, llama_cpp_2::model::AddBos::Never) {
                                            exec_result.model_tokens.extend(suffix_tokens.iter().map(|t| t.0));
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("[IMAGE_SUMMARY] Vision summary failed ({e}), injecting raw image");
                                        // fall through to raw vision injection below
                                    }
                                }
                            }
                            #[cfg(feature = "vision")]
                            if !exec_result.response_images.is_empty() && vision_ctx.is_none() {
                                // Vision feature compiled but no mmproj loaded — drop images, add hint
                                eprintln!("[IMAGE_SUMMARY] No vision model loaded, dropping images");
                                exec_result.response_images.clear();
                                let hint = "\n[Image captured but vision model not loaded. Use ocr_screen to read text from the screen.]";
                                if let Ok(hint_tokens) = model.str_to_token(hint, llama_cpp_2::model::AddBos::Never) {
                                    exec_result.model_tokens.extend(hint_tokens.iter().map(|t| t.0));
                                }
                            }
                            #[cfg(not(feature = "vision"))]
                            {
                                let _ = &prompt; // suppress unused-variable warning (only used in vision path)
                                exec_result.response_images.clear();
                                let hint = "\n[Image captured but vision not compiled. Use ocr_screen to read text from the screen.]";
                                if let Ok(hint_tokens) = model.str_to_token(hint, llama_cpp_2::model::AddBos::Never) {
                                    exec_result.model_tokens.extend(hint_tokens.iter().map(|t| t.0));
                                }
                            }
                        }
                    }

                    // Choose injection path: vision (images + MtmdContext) or standard text tokens
                    #[cfg(feature = "vision")]
                    let used_vision = if !exec_result.response_images.is_empty() {
                        if let Some(mtmd_ctx) = vision_ctx {
                            eprintln!("[VISION] Injecting {} image(s) via vision pipeline...", exec_result.response_images.len());
                            match super::prompt_builder::inject_tool_response_with_vision(
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
                        watchdog.pause();
                        let inject_result = inject_output_tokens(
                            &exec_result.model_tokens, batch, context,
                            &mut gen.token_pos, cfg.conversation_id,
                        );
                        watchdog.resume();
                        watchdog.ping();
                        match inject_result {
                            Ok(()) => {},
                            Err(e) if e == "CONTEXT_EXHAUSTED" => {
                                eprintln!("[CTX_GUARD] Context exhausted during tool output injection — setting finish_reason=length");
                                log_event(cfg.conversation_id, "context_guard", "Context exhausted during tool output injection");
                                gen.finish_reason = "length".to_string();
                                hit_stop_condition = true;
                                break 'token;
                            },
                            Err(e) => return Err(e),
                        }
                    }

                    // Feed injected tokens to sampler so grammar/penalties stay in sync.
                    let injected_tokens: Vec<LlamaToken> = exec_result.model_tokens.iter().map(|&id| LlamaToken(id)).collect();
                    sampler.accept_many(&injected_tokens);
                    gen.generated_token_ids.extend(injected_tokens);

                    std::thread::sleep(std::time::Duration::from_millis(50));
                    context.synchronize();

                    if exec_result.output_block.contains("[INFINITE_LOOP_DETECTED]") {
                        eprintln!("[LOOP] Infinite loop detected — force-stopping generation");
                        log_event(cfg.conversation_id, "infinite_loop", "Force-stopped: model stuck in infinite tool call loop");
                        gen.finish_reason = "infinite_loop".to_string();
                        hit_stop_condition = true;
                        break 'token;
                    }

                    // Mid-task compaction
                    let conv_id_clean = cfg.conversation_id;
                    let cached_overhead = cfg.db.get_context_overhead_tokens(conv_id_clean);
                    if let Some(_summary) = super::compaction::maybe_compact_mid_task(
                        cfg.conversation_id,
                        &cfg.db,
                        model,
                        cfg.backend,
                        cfg.chat_template_string,
                        gen.tool_response_tokens,
                        gen.recent_commands.len(),
                        cfg.context_size,
                        cached_overhead,
                    ) {
                        // Compaction happened — stop this turn so the next turn starts
                        // with the compacted context. Without this break the generation
                        // continues injecting tool outputs until CONTEXT_EXHAUSTED.
                        eprintln!("[COMPACTION] Mid-task compaction fired — stopping generation so next turn uses compacted context");
                        log_event(cfg.conversation_id, "compaction", "mid-task compact → stopping generation for context reload");
                        gen.finish_reason = "length".to_string();
                        hit_stop_condition = true;
                        break 'token;
                    }
                    const PROACTIVE_COMPACT_INTERVAL: usize = 40;
                    if cfg.proactive_compaction
                        && gen.recent_commands.len() > 0
                        && gen.recent_commands.len() % PROACTIVE_COMPACT_INTERVAL == 0
                    {
                        eprintln!("[PROACTIVE_COMPACT] {} tool calls reached, forcing compaction cycle", gen.recent_commands.len());
                        log_event(cfg.conversation_id, "compaction", &format!("{} tool calls → proactive compact", gen.recent_commands.len()));
                        gen.finish_reason = "length".to_string();
                        hit_stop_condition = true;
                        break 'token;
                    }

                    hit_stop_condition = false;
                    gen.last_exec_scan_pos = gen.response.len();
                    gen.exec_tracker = ExecBlockTracker::new();
                    stall_checkpoint = Instant::now();
                    break 'token;
                }
            } // token_has_close_char
        } // 'token

        if hit_stop_condition {
            break;
        }
        if gen.total_tokens_generated >= cfg.max_total_tokens {
            gen.finish_reason = "length".to_string();
            break;
        }

        if !command_executed {
            log_debug!(cfg.conversation_id, "Continuing generation: no stop condition hit");
        }
    }

    watchdog.stop();
    Ok(())
}
