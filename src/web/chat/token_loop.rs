
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

use super::super::models::*;
use super::command_executor::{
    check_and_execute_command_with_tags, inject_output_tokens,
};
use super::stop_conditions::{check_stop_conditions, ExecBlockTracker};
use crate::{log_debug, log_info, log_warn};
use crate::web::event_log::log_event;

/// Mutable state tracked across the token generation loop.
pub(super) struct TokenGenState {
    pub response: String,
    pub token_pos: i32,
    pub total_tokens_generated: i32,
    pub generated_token_ids: Vec<LlamaToken>,
    pub logger_synced_len: usize,
    pub last_logger_sync: Instant,
    pub exec_tracker: ExecBlockTracker,
    pub recent_commands: Vec<String>,
    pub consecutive_loop_blocks: usize,
    pub last_exec_scan_pos: usize,
    /// Why generation stopped: "stop", "length", "cancelled", "tool_calls", "error".
    pub finish_reason: String,
    /// Accumulated tokens from tool call responses injected into context.
    pub tool_response_tokens: i32,
    /// Number of loop recovery attempts (max 1 before giving up).
    pub loop_recoveries: u32,
}

/// Read-only configuration for the token generation loop.
#[allow(dead_code)]
pub(super) struct TokenGenConfig<'a> {
    pub conversation_id: &'a str,
    pub tags: &'a super::tool_tags::ToolTags,
    pub template_type: Option<&'a str>,
    pub stop_tokens: &'a [String],
    pub context_size: u32,
    pub max_total_tokens: i32,
    pub web_search_provider: Option<&'a str>,
    pub web_search_api_key: Option<&'a str>,
    pub use_rtk: bool,
    pub use_htmd: bool,
    pub browser_backend: &'a crate::web::browser::BrowserBackend,
    pub n_batch: u32,
    pub mcp_manager: Option<Arc<crate::web::mcp::McpManager>>,
    pub db: super::super::database::SharedDatabase,
    pub backend: &'a llama_cpp_2::llama_backend::LlamaBackend,
    pub chat_template_string: Option<&'a str>,
    pub proactive_compaction: bool,
}

/// Vision context reference for tool response image injection.
/// When the `vision` feature is enabled, this carries an `Option<&MtmdContext>`.
/// Otherwise it's a zero-size unit type so the parameter compiles away.
#[cfg(feature = "vision")]
pub(super) type VisionCtxRef<'a> = Option<&'a llama_cpp_2::mtmd::MtmdContext>;
#[cfg(not(feature = "vision"))]
pub(super) type VisionCtxRef<'a> = ();

/// Stall detection: if 16 tokens take longer than this, abort generation.
/// Set high enough to accommodate large contexts (65K+ at ~0.2 tok/s = 80s for 16 tokens).
const TOKEN_STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

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
pub(super) fn run_generation_loop(
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
    let gen_start_time = Instant::now();

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
            if cancel.load(Ordering::Relaxed) {
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
                    eprintln!("[STALL] Generation stalled: 16 tokens took {}s (loop_recoveries={})", secs, gen.loop_recoveries);
                    log_event(cfg.conversation_id, "stall", &format!("16 tokens took {}s", secs));
                    if gen.loop_recoveries < 1 {
                        // First stall: try recovery
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
                    break;
                }
                stall_checkpoint = Instant::now();
            }

            // Wall-clock stall check BEFORE sample() — if sample() itself blocks
            // (e.g. VRAM oversubscription, GPU hang), the per-16-token check above
            // never fires because `i` doesn't increment.
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
                break;
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
                            max_tokens: cfg.context_size as i32, status: None,
                            ..Default::default()
                        });
                    }
                }
                hit_stop_condition = true;
                break;
            }

            batch.clear();
            batch.add(next_token, gen.token_pos, &[0], true)
                .map_err(|e| format!("Batch add failed at token {}: {e}", gen.total_tokens_generated))?;
            if let Err(e) = context.decode(batch) {
                let err_str = format!("{e}");
                if err_str.contains("NoKvCacheSlot") || err_str.contains("no kv cache slot") {
                    eprintln!("[CTX_GUARD] NoKvCacheSlot at token {} — treating as finish_reason=length", gen.total_tokens_generated);
                    log_event(cfg.conversation_id, "context_guard", &format!("NoKvCacheSlot at token {}", gen.total_tokens_generated));
                    gen.finish_reason = "length".to_string();
                    hit_stop_condition = true;
                    break;
                }
                // Abort callback triggered during decode (cancel while stuck in llama_decode)
                if err_str.contains("Unknown(2)") || cancel.load(Ordering::Relaxed) {
                    log_info!(cfg.conversation_id, "Decode aborted by cancel callback at token {}", gen.total_tokens_generated);
                    gen.finish_reason = "cancelled".to_string();
                    hit_stop_condition = true;
                    break;
                }
                return Err(format!("Decode failed at token {}: {e}", gen.total_tokens_generated));
            }

            gen.token_pos += 1;
            gen.total_tokens_generated += 1;
            gen.generated_token_ids.push(next_token);

            // Context position guard: if we've consumed >95% of context, stop gracefully.
            // This catches recurrent/hybrid models (Mamba/Jamba) where llama_decode returns
            // success but internally fails (logs "failed to find a memory slot").
            let ctx_limit = cfg.context_size.saturating_sub(cfg.context_size / 20);
            if gen.token_pos as u32 >= ctx_limit {
                eprintln!("[CTX_GUARD] Context 95% full ({}/{}, limit={}) — stopping with finish_reason=length", gen.token_pos, cfg.context_size, ctx_limit);
                log_event(cfg.conversation_id, "context_guard", &format!("Context 95% full ({}/{})", gen.token_pos, cfg.context_size));
                gen.finish_reason = "length".to_string();
                hit_stop_condition = true;
                break;
            }

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
                eprintln!("[LOOP_RECOVERY] Repetition loop detected at token {}, loop_recoveries={}", gen.total_tokens_generated, gen.loop_recoveries);
                log_event(cfg.conversation_id, "loop_recovery", &format!("Repetition loop at token {}", gen.total_tokens_generated));
                if gen.loop_recoveries < 1 {
                    // First loop: try to recover by auto-continuing with corrective message
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
                    break;
                } else {
                    // Already tried recovery once, stop for real
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
                    break;
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

            // Check for and execute commands in the response.
            // Fast gate: only call the expensive detector when the new token
            // contains a character that could close a tool call block.
            // This skips ~90% of tokens (no '>', ']', or '}').
            let token_has_close_char = token_str.as_bytes().iter().any(|&b| b == b'>' || b == b']' || b == b'}');
            if token_has_close_char {
            if let Some(exec_result) = check_and_execute_command_with_tags(
                &gen.response, gen.last_exec_scan_pos, cfg.conversation_id, model, cfg.tags,
                cfg.template_type, cfg.web_search_provider, cfg.web_search_api_key,
                &mut gen.recent_commands, &mut gen.consecutive_loop_blocks, token_sender, gen.token_pos, cfg.context_size,
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
                    match inject_output_tokens(
                        &exec_result.model_tokens, batch, context,
                        &mut gen.token_pos, cfg.conversation_id,
                    ) {
                        Ok(()) => {},
                        Err(e) if e == "CONTEXT_EXHAUSTED" => {
                            eprintln!("[CTX_GUARD] Context exhausted during tool output injection — setting finish_reason=length");
                            log_event(cfg.conversation_id, "context_guard", "Context exhausted during tool output injection");
                            gen.finish_reason = "length".to_string();
                            hit_stop_condition = true;
                            break;
                        },
                        Err(e) => return Err(e),
                    }
                }

                gen.tool_response_tokens += exec_result.model_tokens.len() as i32;
                gen.generated_token_ids.extend(exec_result.model_tokens.iter().map(|&id| LlamaToken(id)));
                command_executed = true;

                // Force-stop on infinite loop detection
                if exec_result.output_block.contains("[INFINITE_LOOP_DETECTED]") {
                    eprintln!("[LOOP] Infinite loop detected — force-stopping generation");
                    log_event(cfg.conversation_id, "infinite_loop", "Force-stopped: model stuck in infinite tool call loop");
                    gen.finish_reason = "infinite_loop".to_string();
                    hit_stop_condition = true;
                    break;
                }

                // Mid-task compaction: if tool outputs are eating too much context,
                // summarize older tool results in DB for the next turn.
                let conv_id_clean = cfg.conversation_id.trim_end_matches(".txt");
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
                    // Compaction happened — DB updated for next turn.
                    // Current generation continues normally.
                }
                // Proactive compaction: every 30 tool calls, force auto-continue
                // to compact conversation and free context space.
                const PROACTIVE_COMPACT_INTERVAL: usize = 30;
                if cfg.proactive_compaction
                    && gen.recent_commands.len() > 0
                    && gen.recent_commands.len() % PROACTIVE_COMPACT_INTERVAL == 0
                {
                    eprintln!("[PROACTIVE_COMPACT] {} tool calls reached, forcing compaction cycle", gen.recent_commands.len());
                    log_event(cfg.conversation_id, "compaction", &format!("{} tool calls → proactive compact", gen.recent_commands.len()));
                    gen.finish_reason = "length".to_string();
                    hit_stop_condition = true;
                    break;
                }

                // tool call round (no limit)
                hit_stop_condition = false;
                gen.last_exec_scan_pos = gen.response.len();
                // Reset exec block tracker after tool execution — the tool call
                // block is now closed (result injected), so we must allow stop
                // tokens to fire again for the model's continuation text.
                gen.exec_tracker = ExecBlockTracker::new();
                stall_checkpoint = Instant::now(); // Reset after tool execution
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
