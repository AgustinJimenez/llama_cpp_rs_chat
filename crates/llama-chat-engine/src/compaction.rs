//! Conversation compaction: automatically summarize old messages when
//! the conversation approaches the context window limit.
//!
//! Strategy (like OpenCode): mark old messages as `compacted=1` in the DB
//! and insert a summary message. The model only sees summaries + recent messages.
//! Original messages are preserved for the user to view.

use llama_chat_db::SharedDatabase;
use llama_chat_types::TokenData;
/// Send a status update to the UI via the token channel.
fn send_status(sender: Option<&tokio::sync::mpsc::UnboundedSender<TokenData>>, message: &str) {
    // Also log as event for polling-based UI
    llama_chat_db::event_log::set_global_status(message);
    if let Some(tx) = sender {
        let _ = tx.send(TokenData {
            token: String::new(),
            tokens_used: 0,
            max_tokens: 0,
            status: Some(message.to_string()),
            ..Default::default()
        });
    }
}

/// Minimum number of recent messages to preserve (not compacted).
const KEEP_RECENT_MESSAGES: usize = 6;

/// Context usage threshold (fraction) to trigger compaction.
/// Applied to the *available* context after subtracting system prompt + tool overhead.
const COMPACTION_THRESHOLD: f64 = 0.70;

/// Fallback overhead estimate when no conversation_context is cached yet.
const FALLBACK_OVERHEAD_TOKENS: usize = 1200;

/// Recursion guard for recompaction: prevents infinite loops.
static RECOMPACT_DEPTH: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Cheap check: does the conversation likely need compaction?
/// Uses chars/4 heuristic to avoid tokenizing. Returns true if compaction is likely needed.
#[allow(dead_code)]
pub fn should_compact(
    conversation_content: &str,
    context_size: u32,
    overhead_tokens: Option<i32>,
) -> bool {
    let estimated_tokens = conversation_content.len() / 4;
    let overhead = overhead_tokens
        .filter(|&o| o > 0)
        .map(|o| o as usize)
        .unwrap_or(FALLBACK_OVERHEAD_TOKENS);
    let available_context = (context_size as usize).saturating_sub(overhead);
    let threshold = (available_context as f64 * COMPACTION_THRESHOLD) as usize;
    estimated_tokens > threshold
}

/// Check if conversation needs compaction and perform it if so.
///
/// This checks the conversation text size against context limits.
/// If compaction is needed, it:
/// 1. Summarizes old messages using the model
/// 2. Marks old messages as `compacted=1` in the DB
/// 3. Inserts a summary message in the DB
///
/// The returned text already reflects the compacted state (from DB reload).
///
/// `force` — skip the usage-threshold check and always compact (used by the manual Compact button).
#[allow(clippy::too_many_arguments)]
pub fn maybe_compact_conversation(
    conversation_content: &str,
    context_size: u32,
    conversation_id: &str,
    db: &SharedDatabase,
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    chat_template_string: Option<&str>,
    overhead_tokens: Option<i32>,
    actual_token_pos: Option<usize>,
    force: bool,
    status_sender: Option<&tokio::sync::mpsc::UnboundedSender<llama_chat_types::TokenData>>,
) -> String {
    // Recursion guard: prevent infinite recompaction
    let depth = RECOMPACT_DEPTH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if depth >= 2 {
        RECOMPACT_DEPTH.store(0, std::sync::atomic::Ordering::Relaxed);
        eprintln!("[COMPACTION] Max recompaction depth reached, stopping");
        return conversation_content.to_string();
    }

    // Use the actual KV cache token position from the last generation if available.
    // This is what the model actually consumed — it accounts for tool output truncation
    // (verbose tool results are stored in DB for display but the model only sees summaries).
    // Falling back to tokenizing raw DB content would overcount dramatically when tools
    // produce large outputs (e.g. nim compiler logs: ~500K chars → ~125K estimated tokens
    // but model only saw ~10K tokens of truncated output).
    let estimated_tokens = if let Some(pos) = actual_token_pos {
        eprintln!("[COMPACTION] Using actual token_pos={} from last generation (raw DB would overcount)", pos);
        pos
    } else {
        // First turn: no prior generation, tokenize raw content as fallback
        model
            .str_to_token(conversation_content, llama_cpp_2::model::AddBos::Never)
            .map(|t| t.len())
            .unwrap_or(conversation_content.len() / 4)
    };
    // Use real overhead from conversation_context if available, else fallback
    let overhead = overhead_tokens
        .filter(|&o| o > 0)
        .map(|o| o as usize)
        .unwrap_or(FALLBACK_OVERHEAD_TOKENS);
    let available_context = (context_size as usize).saturating_sub(overhead);
    let threshold = (available_context as f64 * COMPACTION_THRESHOLD) as usize;

    eprintln!("[COMPACTION] Check: ~{} tokens, threshold={} (ctx={}, overhead={}{}), conv={}",
        estimated_tokens, threshold, context_size, overhead,
        if overhead_tokens.is_some() { " real" } else { " est" }, conversation_id);

    if !force && estimated_tokens < threshold {
        RECOMPACT_DEPTH.store(0, std::sync::atomic::Ordering::Relaxed);
        return conversation_content.to_string();
    }

    log_info!(
        conversation_id,
        "📦 Context compaction triggered: ~{} estimated tokens vs {} threshold ({}% of {})",
        estimated_tokens, threshold, (COMPACTION_THRESHOLD * 100.0) as u32, context_size
    );

    // Load messages from DB to find what to compact
    let messages = match db.get_messages(conversation_id) {
        Ok(msgs) => msgs,
        Err(e) => {
            log_warn!(conversation_id, "📦 Failed to load messages for compaction: {e}");
            return conversation_content.to_string();
        }
    };

    // Filter to non-compacted, non-system messages
    let non_compacted: Vec<(usize, &llama_chat_db::conversation::MessageRecord)> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| !m.compacted && m.role != "system")
        .collect();

    eprintln!("[COMPACTION] {} messages loaded, {} eligible for compaction", messages.len(), non_compacted.len());

    // Compact ALL non-system messages — the summary captures everything including the last
    // interaction. After force compaction the model sees only: system + tools + summary.
    let to_compact: Vec<&(usize, &llama_chat_db::conversation::MessageRecord)> =
        non_compacted.iter().collect();

    if to_compact.is_empty() {
        eprintln!("[COMPACTION] Skipping: nothing to compact");
        return conversation_content.to_string();
    }

    let total_chars: usize = to_compact.iter().map(|(_, m)| m.content.len()).sum();
    eprintln!("[COMPACTION] Will compact {} message(s) ({} chars total)", to_compact.len(), total_chars);

    // Build text of messages to summarize
    let old_text: String = to_compact.iter()
        .map(|(_, m)| format!("{}:\n{}", m.role.to_uppercase(), m.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    // Find the sequence_order of the last message to compact.
    // force=true: compact ALL (summary appears at end).
    // force=false: keep the last message uncompacted (summary appears mid-conversation).
    let target = if force {
        non_compacted.last()
    } else {
        let n = non_compacted.len();
        if n < 2 { None } else { non_compacted.get(n - 2) }
    };
    let up_to_sequence = match target {
        Some((_, m)) => m.sequence_order,
        None => {
            eprintln!("[COMPACTION] Could not determine sequence point, skipping");
            return conversation_content.to_string();
        }
    };

    eprintln!("[COMPACTION] Compacting {} messages ({} chars) up to seq {}", to_compact.len(), old_text.len(), up_to_sequence);
    log_info!(
        conversation_id,
        "📦 Compacting {} messages ({} chars) up to sequence {}",
        to_compact.len(), old_text.len(), up_to_sequence
    );

    // Summarize old messages using the model
    eprintln!("[COMPACTION] Running summarization on {} chars...", old_text.len());
    send_status(status_sender, "Compacting conversation...");
    let summary = match summarize_conversation(
        model, backend, &old_text, chat_template_string, conversation_id, context_size, status_sender,
    ) {
        Ok(s) => {
            eprintln!("[COMPACTION] Summarization succeeded: {} chars → {} chars", old_text.len(), s.len());
            s
        },
        Err(e) => {
            eprintln!("[COMPACTION] Summarization failed: {e}, using truncation fallback");
            old_text.chars().take(500).collect::<String>() + "\n[...older messages truncated...]"
        }
    };

    // Hard cap: summary must be much shorter than the original to actually free context.
    // Target: summary should be at most 30% of available context (in chars, ~4 chars/token).
    let max_summary_chars = (available_context * 4) * 30 / 100;
    let summary_with_task = if summary.len() > max_summary_chars {
        eprintln!("[COMPACTION] Summary too long ({} chars), truncating to {max_summary_chars} chars", summary.len());
        let truncated: String = summary.chars().take(max_summary_chars).collect();
        format!("{truncated}\n\n[...summary truncated for context space...]")
    } else {
        summary
    };

    // Persist to DB: record summary covering everything up to up_to_sequence.
    match db.compact_messages(conversation_id, up_to_sequence, &summary_with_task) {
        Ok(marked) => {
            eprintln!("[COMPACTION] DB compaction done: {marked} messages covered by summary");
        }
        Err(e) => {
            eprintln!("[COMPACTION] DB compaction failed: {e}");
        }
    }

    // Clear compaction status indicator
    llama_chat_db::event_log::clear_global_status();

    // Reload conversation text from DB (now reflects compaction)
    match db.get_conversation_as_text(conversation_id) {
        Ok(text) => {
            let new_estimated = model
                .str_to_token(&text, llama_cpp_2::model::AddBos::Never)
                .map(|t| t.len())
                .unwrap_or(text.len() / 4);
            log_info!(
                conversation_id,
                "📦 Compaction result: ~{} → ~{} estimated tokens (saved ~{})",
                estimated_tokens, new_estimated, estimated_tokens.saturating_sub(new_estimated)
            );

            // If first pass didn't free enough, recompact more aggressively
            if new_estimated > threshold {
                eprintln!(
                    "[COMPACTION] First pass insufficient: {new_estimated} > {threshold} threshold, recompacting..."
                );
                log_info!(
                    conversation_id,
                    "📦 Recompaction triggered: {} still > {} threshold",
                    new_estimated, threshold
                );
                return maybe_compact_conversation(
                    &text, context_size, conversation_id, db, model, backend,
                    chat_template_string, overhead_tokens, None, false, status_sender,
                );
            }

            // Reset recursion guard on successful completion
            RECOMPACT_DEPTH.store(0, std::sync::atomic::Ordering::Relaxed);
            text
        }
        Err(e) => {
            RECOMPACT_DEPTH.store(0, std::sync::atomic::Ordering::Relaxed);
            log_warn!(conversation_id, "📦 Failed to reload after compaction: {}", e);
            conversation_content.to_string()
        }
    }
}

/// Map-reduce summarization: split large text into chunks, summarize each,
/// then combine all chunk summaries into one final summary.
/// Uses a SINGLE reusable context to avoid CUDA memory fragmentation.
///
/// Chunk size and summary context are derived dynamically from `context_size`:
/// - input budget  = 75% of context (leaves 25% for output + prompt overhead)
/// - chunk_chars   = input_budget_tokens × 3  (~3 chars/token for chat text)
/// - summary_ctx   = min(context_size, 8192)  (no need for huge VRAM just to write a summary)
fn summarize_conversation(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    old_text: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
    context_size: u32,
    status_sender: Option<&tokio::sync::mpsc::UnboundedSender<TokenData>>,
) -> Result<String, String> {
    use super::command_executor::run_summary_pass_public;
    use crate::generation::create_fresh_context;
    use llama_chat_types::SamplerConfig;
    use std::num::NonZeroU32;

    // Summary context: use large context for fewer chunks + better GPU utilization.
    // 8K was far too small — resulted in 35+ passes for a 158K conversation.
    // 65K gives ~4-5 map chunks for the same conversation, 7x fewer passes.
    let summary_ctx = context_size.clamp(512, 65536);
    // Reserve tokens for output + prompt overhead. Use summary_ctx (not the main context_size)
    // so chunk_size stays within what the summary context can actually hold.
    let reserved = (summary_ctx / 4).clamp(256, 2048);
    let input_tokens = summary_ctx.saturating_sub(reserved).saturating_sub(64); // 64 for prompt template overhead
    let chunk_size_chars = (input_tokens as usize) * 3;

    eprintln!(
        "[COMPACTION] Dynamic sizing: model_ctx={context_size}, reserved={reserved}, input_tokens={input_tokens}, chunk_chars={chunk_size_chars}, summary_ctx={summary_ctx}"
    );

    if old_text.len() <= chunk_size_chars {
        send_status(status_sender, "Compacting conversation (0%)");
        return run_summary_pass_public(model, backend, old_text, chat_template_string, conversation_id);
    }

    // Create ONE summary context, reuse for all chunks (avoids CUDA memory fragmentation)
    let n_ctx = NonZeroU32::new(summary_ctx).unwrap();
    let config = SamplerConfig::default();
    let mut ctx = create_fresh_context(model, backend, n_ctx, true, &config)?;  // offload_kqv=true: KV cache on VRAM not CPU
    eprintln!("[COMPACTION] Created reusable summary context (n_ctx={summary_ctx}, kv_on_gpu=true)");

    let result = summarize_with_ctx(model, &mut ctx, old_text, chunk_size_chars, chat_template_string, conversation_id, summary_ctx as usize, reserved as usize, status_sender);

    // Drop the single context — only one alloc/free cycle
    drop(ctx);
    eprintln!("[COMPACTION] Summary context released");

    result
}

/// Inner map-reduce using a reusable context.
#[allow(clippy::too_many_arguments)]
fn summarize_with_ctx(
    model: &llama_cpp_2::model::LlamaModel,
    ctx: &mut llama_cpp_2::context::LlamaContext<'_>,
    old_text: &str,
    chunk_size: usize,
    chat_template_string: Option<&str>,
    conversation_id: &str,
    ctx_size: usize,
    max_tokens: usize,
    status_sender: Option<&tokio::sync::mpsc::UnboundedSender<TokenData>>,
) -> Result<String, String> {
    use super::command_executor::run_summary_reusing_ctx;

    // === MAP PHASE: split into chunks and summarize each ===
    let mut chunk_summaries = Vec::new();
    let mut pos = 0;
    let mut chunk_num = 0;
    let total_chunks = old_text.len().div_ceil(chunk_size);
    send_status(status_sender, "Compacting conversation (0%)");

    while pos < old_text.len() {
        let end = (pos + chunk_size).min(old_text.len());
        let end = (pos..=end).rev().find(|&i| old_text.is_char_boundary(i)).unwrap_or(end);
        let chunk = &old_text[pos..end];
        chunk_num += 1;
        let total_steps = total_chunks + 1; // +1 for reduce phase

        eprintln!("[COMPACTION] Map phase: chunk {chunk_num}/{total_chunks} ({} chars, chunk_size={chunk_size})...", chunk.len());

        match run_summary_reusing_ctx(model, ctx, chunk, chat_template_string, conversation_id, ctx_size, max_tokens) {
            Ok(summary) => {
                eprintln!("[COMPACTION] Chunk {chunk_num} → {} chars", summary.len());
                chunk_summaries.push(summary);
                let pct = (chunk_num * 100) / total_steps;
                send_status(status_sender, &format!("Compacting conversation ({pct}%)"));
            }
            Err(e) => {
                // Chunk too large for summary context (dense code content tokenizes at ~2 chars/token,
                // not the 3 chars/token estimate used for sizing). Split in half and summarize each part.
                eprintln!("[COMPACTION] Chunk {chunk_num} too large ({e}), splitting in half");
                let mid = chunk.len() / 2;
                let mid = (0..=mid).rev().find(|&i| chunk.is_char_boundary(i)).unwrap_or(mid);
                let half1 = run_summary_reusing_ctx(model, ctx, &chunk[..mid], chat_template_string, conversation_id, ctx_size, max_tokens)
                    .unwrap_or_else(|_| chunk[..mid.min(500)].to_string() + "...[truncated]");
                let half2 = run_summary_reusing_ctx(model, ctx, &chunk[mid..], chat_template_string, conversation_id, ctx_size, max_tokens)
                    .unwrap_or_else(|_| chunk[mid..].chars().take(500).collect::<String>() + "...[truncated]");
                let combined = format!("{half1}\n{half2}");
                eprintln!("[COMPACTION] Chunk {chunk_num} split → {} chars", combined.len());
                chunk_summaries.push(combined);
            }
        }

        pos = end;
    }

    // === REDUCE PHASE ===
    let combined = chunk_summaries.join("\n\n");
    eprintln!("[COMPACTION] Reduce: {} chunk summaries ({} chars) → final...", chunk_summaries.len(), combined.len());

    if combined.len() <= chunk_size {
        run_summary_reusing_ctx(model, ctx, &combined, chat_template_string, conversation_id, ctx_size, max_tokens)
    } else {
        // Combined summaries still too large — recurse (chunk_size is preserved)
        summarize_with_ctx(model, ctx, &combined, chunk_size, chat_template_string, conversation_id, ctx_size, max_tokens, status_sender)
    }
}

// ─── Mid-Task Incremental Compaction ─────────────────────────────────

/// Threshold: compact when tool outputs consume this fraction of available context.
const MID_TASK_THRESHOLD: f64 = 0.30;

/// Minimum tool calls in current turn before mid-task compaction can trigger.
const MIN_TOOL_CALLS_FOR_MID_TASK: usize = 3;

/// Check if tool outputs are consuming too much context and compact if so.
///
/// Unlike `maybe_compact_conversation` (which runs at generation start),
/// this runs DURING generation after each tool execution. It checks if
/// accumulated tool output is eating too much context and summarizes
/// older tool results in the DB for the next turn.
///
/// Returns Some(summary) if compaction happened, None otherwise.
#[allow(clippy::too_many_arguments)]
pub fn maybe_compact_mid_task(
    conversation_id: &str,
    db: &SharedDatabase,
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    chat_template_string: Option<&str>,
    tool_response_tokens: i32,
    tool_call_count: usize,
    context_size: u32,
    overhead_tokens: i32,
) -> Option<String> {
    // Need at least N tool calls before considering mid-task compaction
    if tool_call_count < MIN_TOOL_CALLS_FOR_MID_TASK {
        return None;
    }

    // Check if tool outputs exceed threshold of available context
    let overhead = if overhead_tokens > 0 { overhead_tokens as usize } else { FALLBACK_OVERHEAD_TOKENS };
    let available = (context_size as usize).saturating_sub(overhead);
    let threshold = (available as f64 * MID_TASK_THRESHOLD) as i32;

    if tool_response_tokens < threshold {
        return None;
    }

    eprintln!(
        "[COMPACTION] Mid-task triggered: {tool_response_tokens} tool tokens > {threshold} threshold ({tool_call_count} calls), conv={conversation_id}"
    );

    // Load recent non-compacted messages that are tool-related
    let messages = match db.get_messages(conversation_id) {
        Ok(msgs) => msgs,
        Err(_) => return None,
    };

    // Find assistant messages with tool calls (they contain <tool_call> or similar)
    // and their following tool results — these are candidates for compaction
    let non_compacted: Vec<_> = messages.iter()
        .enumerate()
        .filter(|(_, m)| !m.compacted)
        .collect();

    if non_compacted.len() <= KEEP_RECENT_MESSAGES + 1 {
        return None;
    }

    // Take all but the last KEEP_RECENT_MESSAGES messages for compaction
    let split = non_compacted.len() - KEEP_RECENT_MESSAGES;
    let to_compact: Vec<_> = non_compacted[..split].iter()
        .filter(|(_, m)| m.role != "system")
        .collect();

    if to_compact.is_empty() {
        return None;
    }

    // Build text of messages to summarize
    let old_text: String = to_compact.iter()
        .map(|(_, m)| format!("{}:\n{}", m.role.to_uppercase(), m.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    if old_text.len() < 200 {
        return None; // Not enough content to summarize
    }

    // Get sequence point: last message in to_compact (keep KEEP_RECENT_MESSAGES after it).
    let up_to_sequence = match to_compact.last() {
        Some((_, m)) => m.sequence_order,
        None => return None,
    };

    eprintln!(
        "[COMPACTION] Mid-task: summarizing {} messages ({} chars)",
        to_compact.len(), old_text.len()
    );

    // Summarize
    let summary = match summarize_conversation(model, backend, &old_text, chat_template_string, conversation_id, context_size, None) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[COMPACTION] Mid-task summarization failed: {e}");
            return None;
        }
    };

    // Persist to DB
    if let Err(e) = db.compact_messages(conversation_id, up_to_sequence, &summary) {
        eprintln!("[COMPACTION] Mid-task DB update failed: {e}");
        return None;
    }

    eprintln!(
        "[COMPACTION] Mid-task complete: {} messages compacted, summary={} chars",
        to_compact.len(), summary.len()
    );

    Some(summary)
}

/// Force compaction of a conversation regardless of context usage threshold.
///
/// Called by the manual "Compact" button in the UI. Uses context_size=1 so the
/// threshold evaluates to 0, guaranteeing the compaction logic runs.
pub fn force_compact_conversation(
    conversation_id: &str,
    db: &SharedDatabase,
    llama_state: &llama_chat_types::models::SharedLlamaState,
    status_sender: Option<&tokio::sync::mpsc::UnboundedSender<llama_chat_types::TokenData>>,
) -> Result<(), String> {
    let state_guard = llama_state.lock().map_err(|_| "Failed to lock LLaMA state")?;
    let state = state_guard.as_ref().ok_or("No model loaded")?;
    let model = state.model.as_ref().ok_or("No model loaded")?;
    // Prefer the actually-loaded context size (from InferenceCache) over the GGUF native
    // context_length, which may report a small value (e.g. 4096) even when loaded at 177K.
    let real_ctx = state.inference_cache.as_ref()
        .map(|c| c.context_size)
        .or(state.model_context_length)
        .unwrap_or(4096);

    let conversation_content = db
        .get_conversation_as_text(conversation_id)
        .map_err(|e| format!("DB error: {e}"))?;

    if conversation_content.trim().is_empty() {
        return Err("Conversation is empty, nothing to compact".into());
    }

    eprintln!("[COMPACTION] Force compaction: conv={conversation_id}, ctx={real_ctx}");

    // Pass real context size so summarize_conversation can compute proper chunk sizes.
    // force=true bypasses the threshold check so compaction always runs.
    maybe_compact_conversation(
        &conversation_content,
        real_ctx,
        conversation_id,
        db,
        model,
        &state.backend,
        state.chat_template_string.as_deref(),
        None,
        None,
        true, // force — skip threshold check
        status_sender,
    );

    Ok(())
}
