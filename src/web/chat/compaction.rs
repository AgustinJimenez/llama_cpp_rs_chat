//! Conversation compaction: automatically summarize old messages when
//! the conversation approaches the context window limit.
//!
//! Strategy (like OpenCode): mark old messages as `compacted=1` in the DB
//! and insert a summary message. The model only sees summaries + recent messages.
//! Original messages are preserved for the user to view.

use crate::web::database::SharedDatabase;
use crate::web::models::TokenData;
use crate::{log_info, log_warn};

/// Send a status update to the UI via the token channel.
fn send_status(sender: Option<&tokio::sync::mpsc::UnboundedSender<TokenData>>, message: &str) {
    if let Some(tx) = sender {
        let _ = tx.send(TokenData {
            token: String::new(),
            tokens_used: 0,
            max_tokens: 0,
            status: Some(message.to_string()),
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

/// Check if conversation needs compaction and perform it if so.
///
/// This checks the conversation text size against context limits.
/// If compaction is needed, it:
/// 1. Summarizes old messages using the model
/// 2. Marks old messages as `compacted=1` in the DB
/// 3. Inserts a summary message in the DB
///
/// The returned text already reflects the compacted state (from DB reload).
pub fn maybe_compact_conversation(
    conversation_content: &str,
    context_size: u32,
    conversation_id: &str,
    db: &SharedDatabase,
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    chat_template_string: Option<&str>,
    overhead_tokens: Option<i32>,
    status_sender: Option<&tokio::sync::mpsc::UnboundedSender<crate::web::models::TokenData>>,
) -> String {
    // Estimate token count (~4 chars per token)
    let estimated_tokens = conversation_content.len() / 4;
    // Use real overhead from conversation_context if available, else fallback
    let overhead = overhead_tokens
        .filter(|&o| o > 0)
        .map(|o| o as usize)
        .unwrap_or(FALLBACK_OVERHEAD_TOKENS);
    let available_context = (context_size as usize).saturating_sub(overhead);
    let threshold = (available_context as f64 * COMPACTION_THRESHOLD) as usize;

    // Strip .txt suffix from conversation_id (logger adds it for backward compat)
    let conversation_id = conversation_id.trim_end_matches(".txt");

    eprintln!("[COMPACTION] Check: ~{} tokens, threshold={} (ctx={}, overhead={}{}), conv={}",
        estimated_tokens, threshold, context_size, overhead,
        if overhead_tokens.is_some() { " real" } else { " est" }, conversation_id);

    if estimated_tokens < threshold {
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
            log_warn!(conversation_id, "📦 Failed to load messages for compaction: {}", e);
            return conversation_content.to_string();
        }
    };

    // Filter to non-compacted, non-system messages
    let non_compacted: Vec<(usize, &crate::web::database::conversation::MessageRecord)> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| !m.compacted && m.role != "system")
        .collect();

    eprintln!("[COMPACTION] {} messages loaded, {} eligible for compaction", messages.len(), non_compacted.len());

    // Aggressive compaction: compact all large messages, keep only the last user message.
    // Find messages to compact: everything except the last user message.
    let last_user_idx = non_compacted.iter().rposition(|(_, m)| m.role == "user");
    let to_compact: Vec<&(usize, &crate::web::database::conversation::MessageRecord)> = non_compacted.iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != last_user_idx)
        .map(|(_, item)| item)
        .collect();

    if to_compact.is_empty() {
        eprintln!("[COMPACTION] Skipping: nothing to compact (only user messages)");
        return conversation_content.to_string();
    }

    let total_chars: usize = to_compact.iter().map(|(_, m)| m.content.len()).sum();
    eprintln!("[COMPACTION] Will compact {} message(s) ({} chars total), keeping last user message", to_compact.len(), total_chars);

    // Build text of messages to summarize
    let old_text: String = to_compact.iter()
        .map(|(_, m)| format!("{}:\n{}", m.role.to_uppercase(), m.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    // Use the old_messages slice for the sequence detection below
    let old_messages = &non_compacted[..non_compacted.len().saturating_sub(1).max(1)];

    // Find the sequence_order of the last old message for DB marking
    // We need to get sequence_order from the DB — use a query
    let up_to_sequence = match get_sequence_for_compaction(db, conversation_id, old_messages.len()) {
        Some(seq) => seq,
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
        model, backend, &old_text, chat_template_string, conversation_id, status_sender,
    ) {
        Ok(s) => {
            eprintln!("[COMPACTION] Summarization succeeded: {} chars → {} chars", old_text.len(), s.len());
            s
        },
        Err(e) => {
            eprintln!("[COMPACTION] Summarization failed: {}, using truncation fallback", e);
            old_text.chars().take(500).collect::<String>() + "\n[...older messages truncated...]"
        }
    };

    let summary_with_task = summary;

    // Persist to DB: mark old messages as compacted, insert summary
    let summary_sequence = up_to_sequence + 1; // Place summary right after compacted messages
    match db.compact_messages(conversation_id, up_to_sequence, &summary_with_task, summary_sequence) {
        Ok(marked) => {
            eprintln!("[COMPACTION] DB compaction done: {} messages marked as compacted, summary at seq {}", marked, summary_sequence);
        }
        Err(e) => {
            eprintln!("[COMPACTION] DB compaction failed: {}", e);
        }
    }

    // Reload conversation text from DB (now reflects compaction)
    match db.get_conversation_as_text(conversation_id) {
        Ok(text) => {
            let new_estimated = text.len() / 4;
            log_info!(
                conversation_id,
                "📦 Compaction result: ~{} → ~{} estimated tokens (saved ~{})",
                estimated_tokens, new_estimated, estimated_tokens.saturating_sub(new_estimated)
            );
            text
        }
        Err(e) => {
            log_warn!(conversation_id, "📦 Failed to reload after compaction: {}", e);
            conversation_content.to_string()
        }
    }
}

fn get_sequence_for_compaction(
    db: &SharedDatabase,
    conversation_id: &str,
    old_count: usize,
) -> Option<i32> {
    let messages = db.get_messages(conversation_id).ok()?;
    let non_compacted_non_system: Vec<_> = messages.iter()
        .filter(|m| !m.compacted && m.role != "system")
        .collect();

    if old_count >= non_compacted_non_system.len() {
        return None;
    }

    // We need the sequence_order of the last "old" message.
    // Since MessageRecord doesn't have sequence_order, compute it from position.
    // The messages are ordered by sequence_order ASC, so the Nth non-compacted
    // non-system message corresponds to a position in the full message list.
    let conn = db.connection();
    let mut stmt = conn.prepare(
        "SELECT sequence_order FROM messages WHERE conversation_id = ?1 AND COALESCE(compacted, 0) = 0 AND role != 'system' ORDER BY sequence_order ASC LIMIT 1 OFFSET ?2"
    ).ok()?;

    stmt.query_row(rusqlite::params![conversation_id, old_count - 1], |row| {
        row.get::<_, i32>(0)
    }).ok()
}

/// Map-reduce summarization: split large text into chunks, summarize each,
/// then combine all chunk summaries into one final summary.
/// Uses a SINGLE reusable context to avoid CUDA memory fragmentation.
fn summarize_conversation(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    old_text: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
    status_sender: Option<&tokio::sync::mpsc::UnboundedSender<TokenData>>,
) -> Result<String, String> {
    use super::command_executor::run_summary_pass_public;
    use crate::web::chat::generation::create_fresh_context;
    use crate::web::models::SamplerConfig;
    use std::num::NonZeroU32;

    const CHUNK_SIZE: usize = 10000;
    const SUMMARY_CTX: u32 = 4096;

    if old_text.len() <= CHUNK_SIZE {
        return run_summary_pass_public(model, backend, old_text, chat_template_string, conversation_id);
    }

    // Create ONE summary context, reuse for all chunks (avoids CUDA memory fragmentation)
    let n_ctx = NonZeroU32::new(SUMMARY_CTX).unwrap();
    let config = SamplerConfig::default();
    let mut ctx = create_fresh_context(model, backend, n_ctx, false, &config)?;
    eprintln!("[COMPACTION] Created reusable summary context ({})", SUMMARY_CTX);

    let result = summarize_with_ctx(model, &mut ctx, old_text, chat_template_string, conversation_id, status_sender);

    // Drop the single context — only one alloc/free cycle
    drop(ctx);
    eprintln!("[COMPACTION] Summary context released");

    result
}

/// Inner map-reduce using a reusable context.
fn summarize_with_ctx(
    model: &llama_cpp_2::model::LlamaModel,
    ctx: &mut llama_cpp_2::context::LlamaContext<'_>,
    old_text: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
    status_sender: Option<&tokio::sync::mpsc::UnboundedSender<TokenData>>,
) -> Result<String, String> {
    use super::command_executor::run_summary_reusing_ctx;
    const CHUNK_SIZE: usize = 10000;

    // === MAP PHASE: split into chunks and summarize each ===
    let mut chunk_summaries = Vec::new();
    let mut pos = 0;
    let mut chunk_num = 0;
    let total_chunks = (old_text.len() + CHUNK_SIZE - 1) / CHUNK_SIZE;

    while pos < old_text.len() {
        let end = (pos + CHUNK_SIZE).min(old_text.len());
        let end = (pos..=end).rev().find(|&i| old_text.is_char_boundary(i)).unwrap_or(end);
        let chunk = &old_text[pos..end];
        chunk_num += 1;

        let status_msg = format!("Compacting conversation ({}/{})", chunk_num, total_chunks);
        send_status(status_sender, &status_msg);
        eprintln!("[COMPACTION] Map phase: chunk {}/{} ({} chars)...", chunk_num, total_chunks, chunk.len());

        match run_summary_reusing_ctx(model, ctx, chunk, chat_template_string, conversation_id) {
            Ok(summary) => {
                eprintln!("[COMPACTION] Chunk {} → {} chars", chunk_num, summary.len());
                chunk_summaries.push(summary);
            }
            Err(e) => {
                eprintln!("[COMPACTION] Chunk {} failed: {}, truncating", chunk_num, e);
                chunk_summaries.push(chunk.chars().take(200).collect::<String>() + "...");
            }
        }

        pos = end;
    }

    // === REDUCE PHASE ===
    let combined = chunk_summaries.join("\n\n");
    send_status(status_sender, "Finalizing summary...");
    eprintln!("[COMPACTION] Reduce: {} summaries ({} chars) → final...", chunk_summaries.len(), combined.len());

    if combined.len() <= CHUNK_SIZE {
        run_summary_reusing_ctx(model, ctx, &combined, chat_template_string, conversation_id)
    } else {
        summarize_with_ctx(model, ctx, &combined, chat_template_string, conversation_id, status_sender)
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
    // Strip .txt suffix
    let conversation_id = conversation_id.trim_end_matches(".txt");

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
        "[COMPACTION] Mid-task triggered: {} tool tokens > {} threshold ({} calls), conv={}",
        tool_response_tokens, threshold, tool_call_count, conversation_id
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

    // Get sequence point for DB marking
    let up_to_sequence = match get_sequence_for_compaction(db, conversation_id, to_compact.len()) {
        Some(seq) => seq,
        None => return None,
    };

    eprintln!(
        "[COMPACTION] Mid-task: summarizing {} messages ({} chars)",
        to_compact.len(), old_text.len()
    );

    // Summarize
    let summary = match summarize_conversation(model, backend, &old_text, chat_template_string, conversation_id, None) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[COMPACTION] Mid-task summarization failed: {}", e);
            return None;
        }
    };

    // Persist to DB
    let summary_sequence = up_to_sequence + 1;
    if let Err(e) = db.compact_messages(conversation_id, up_to_sequence, &summary, summary_sequence) {
        eprintln!("[COMPACTION] Mid-task DB update failed: {}", e);
        return None;
    }

    eprintln!(
        "[COMPACTION] Mid-task complete: {} messages compacted, summary={} chars",
        to_compact.len(), summary.len()
    );

    Some(summary)
}
