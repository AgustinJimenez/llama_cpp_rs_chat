//! Conversation compaction: automatically summarize old messages when
//! the conversation approaches the context window limit.
//!
//! Strategy (like OpenCode): mark old messages as `compacted=1` in the DB
//! and insert a summary message. The model only sees summaries + recent messages.
//! Original messages are preserved for the user to view.

use crate::web::database::SharedDatabase;
use crate::{log_info, log_warn};

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

    // Check for oversized single messages (e.g., a huge assistant response with many tool calls).
    // Even with few messages, if one exceeds 50% of available context, summarize it.
    let half_available = available_context / 2;
    let oversized: Vec<(usize, &crate::web::database::conversation::MessageRecord)> = non_compacted.iter()
        .filter(|(_, m)| m.content.len() / 4 > half_available)
        .cloned()
        .collect();

    if !oversized.is_empty() {
        eprintln!("[COMPACTION] Found {} oversized message(s), compacting regardless of message count", oversized.len());
        // Summarize oversized messages and mark them as compacted
        for (_, msg) in &oversized {
            let summary = match summarize_conversation(model, backend, &msg.content, chat_template_string, conversation_id) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[COMPACTION] Oversized message summarization failed: {}", e);
                    continue;
                }
            };

            // Find sequence_order for this message and compact it
            let conn = db.connection();
            let seq: Option<i32> = conn.query_row(
                "SELECT sequence_order FROM messages WHERE conversation_id = ?1 AND role = ?2 AND LENGTH(content) = ?3 AND COALESCE(compacted, 0) = 0 ORDER BY sequence_order DESC LIMIT 1",
                rusqlite::params![conversation_id, msg.role, msg.content.len()],
                |row| row.get(0),
            ).ok();

            if let Some(seq) = seq {
                // Mark the oversized message as compacted
                let _ = conn.execute(
                    "UPDATE messages SET compacted = 1 WHERE conversation_id = ?1 AND sequence_order = ?2",
                    rusqlite::params![conversation_id, seq],
                );
                // Insert summary right after it
                let summary_seq = seq + 1;
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let _ = conn.execute(
                    "INSERT INTO messages (id, conversation_id, role, content, timestamp, sequence_order, is_streaming, compacted) VALUES (?1, ?2, 'system', ?3, ?4, ?5, 0, 0)",
                    rusqlite::params![
                        uuid::Uuid::new_v4().to_string(),
                        conversation_id,
                        format!("[Conversation summary — oversized message compacted]\n{}", summary),
                        timestamp,
                        summary_seq,
                    ],
                );
                eprintln!("[COMPACTION] Oversized message (seq={}) compacted: {} chars → {} chars", seq, msg.content.len(), summary.len());
            }
        }

        // Reload and return
        return db.get_conversation_as_text(conversation_id).unwrap_or_else(|_| conversation_content.to_string());
    }

    if non_compacted.len() <= KEEP_RECENT_MESSAGES + 1 {
        eprintln!("[COMPACTION] Skipping: only {} msgs, need > {}", non_compacted.len(), KEEP_RECENT_MESSAGES);
        return conversation_content.to_string();
    }

    let split_point = non_compacted.len() - KEEP_RECENT_MESSAGES;
    let old_messages = &non_compacted[..split_point];

    // Build text of old messages to summarize
    let old_text: String = old_messages.iter()
        .map(|(_, m)| format!("{}:\n{}", m.role.to_uppercase(), m.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    // Find the sequence_order of the last old message for DB marking
    // We need to get sequence_order from the DB — use a query
    let up_to_sequence = match get_sequence_for_compaction(db, conversation_id, old_messages.len()) {
        Some(seq) => seq,
        None => {
            eprintln!("[COMPACTION] Could not determine sequence point, skipping");
            return conversation_content.to_string();
        }
    };

    eprintln!("[COMPACTION] Compacting {} old messages, keeping {} recent", old_messages.len(), KEEP_RECENT_MESSAGES);
    log_info!(
        conversation_id,
        "📦 Compacting {} messages ({} chars) up to sequence {}, keeping {} recent",
        old_messages.len(), old_text.len(), up_to_sequence, KEEP_RECENT_MESSAGES
    );

    // Summarize old messages using the model
    let summary = match summarize_conversation(
        model, backend, &old_text, chat_template_string, conversation_id,
    ) {
        Ok(s) => s,
        Err(e) => {
            log_warn!(conversation_id, "📦 Summarization failed: {}, using truncation fallback", e);
            old_text.chars().take(500).collect::<String>() + "\n[...older messages truncated...]"
        }
    };

    // Persist to DB: mark old messages as compacted, insert summary
    let summary_sequence = up_to_sequence + 1; // Place summary right after compacted messages
    match db.compact_messages(conversation_id, up_to_sequence, &summary, summary_sequence) {
        Ok(marked) => {
            log_info!(conversation_id, "📦 DB compaction: {} messages marked, summary inserted at seq {}", marked, summary_sequence);
        }
        Err(e) => {
            log_warn!(conversation_id, "📦 DB compaction failed: {}", e);
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

fn summarize_conversation(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    old_text: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
) -> Result<String, String> {
    use super::command_executor::run_summary_pass_public;

    // If old text is very large, take beginning + end
    let max_chars = 8000;
    let text_to_summarize = if old_text.len() > max_chars {
        let half = max_chars / 2;
        let start = &old_text[..half];
        let end_start = old_text.len() - half;
        let end = &old_text[end_start..];
        format!("{}\n[...{} chars omitted...]\n{}", start, old_text.len() - max_chars, end)
    } else {
        old_text.to_string()
    };

    run_summary_pass_public(model, backend, &text_to_summarize, chat_template_string, conversation_id)
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
    let summary = match summarize_conversation(model, backend, &old_text, chat_template_string, conversation_id) {
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
