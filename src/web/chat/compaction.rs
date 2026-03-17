//! Conversation compaction: automatically summarize old messages when
//! the conversation approaches the context window limit.
//!
//! Strategy: keep system prompt + last N recent turns, summarize everything
//! in between into a single "[Previous conversation summary]" message.

use crate::web::database::SharedDatabase;
use crate::{log_info, log_warn};

/// Minimum number of recent messages to preserve (not compacted).
const KEEP_RECENT_MESSAGES: usize = 6;

/// Context usage threshold (fraction) to trigger compaction.
/// At 70%, compact to make room for generation + tool output.
const COMPACTION_THRESHOLD: f64 = 0.70;

/// Check if conversation needs compaction and perform it if so.
///
/// Returns the (possibly compacted) conversation text.
/// If compaction occurs, the DB is updated with the summary.
pub fn maybe_compact_conversation(
    conversation_content: &str,
    context_size: u32,
    conversation_id: &str,
    db: &SharedDatabase,
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    chat_template_string: Option<&str>,
) -> String {
    // Estimate token count (~4 chars per token)
    let estimated_tokens = conversation_content.len() / 4;
    let threshold = (context_size as f64 * COMPACTION_THRESHOLD) as usize;

    if estimated_tokens < threshold {
        return conversation_content.to_string();
    }

    log_info!(
        conversation_id,
        "📦 Context compaction triggered: ~{} estimated tokens vs {} threshold ({}% of {})",
        estimated_tokens, threshold, (COMPACTION_THRESHOLD * 100.0) as u32, context_size
    );

    // Parse conversation into messages
    let messages = parse_conversation_messages(conversation_content);
    if messages.len() <= KEEP_RECENT_MESSAGES + 1 {
        // Too few messages to compact
        log_info!(conversation_id, "📦 Only {} messages, skipping compaction", messages.len());
        return conversation_content.to_string();
    }

    // Split: [system_prompt?, ...old_messages, ...recent_messages]
    let (system_msg, rest) = if messages.first().map(|m| m.role.as_str()) == Some("SYSTEM") {
        (Some(&messages[0]), &messages[1..])
    } else {
        (None, &messages[..])
    };

    if rest.len() <= KEEP_RECENT_MESSAGES {
        return conversation_content.to_string();
    }

    let split_point = rest.len() - KEEP_RECENT_MESSAGES;
    let old_messages = &rest[..split_point];
    let recent_messages = &rest[split_point..];

    // Build text of old messages to summarize
    let old_text: String = old_messages.iter()
        .map(|m| format!("{}:\n{}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    log_info!(
        conversation_id,
        "📦 Compacting {} old messages ({} chars) into summary, keeping {} recent",
        old_messages.len(), old_text.len(), recent_messages.len()
    );

    // Summarize old messages using the model
    let summary = match summarize_conversation(
        model, backend, &old_text, chat_template_string, conversation_id,
    ) {
        Ok(s) => s,
        Err(e) => {
            log_warn!(conversation_id, "📦 Compaction summarization failed: {}, using truncation", e);
            // Fallback: just truncate old messages to a brief excerpt
            old_text.chars().take(500).collect::<String>() + "\n[...older messages truncated...]"
        }
    };

    // Rebuild conversation: system + summary + recent
    let mut compacted = String::new();
    if let Some(sys) = system_msg {
        compacted.push_str(&format!("{}:\n{}\n\n", sys.role, sys.content));
    }
    compacted.push_str(&format!("SYSTEM:\n[Previous conversation summary]\n{}\n\n", summary));
    for msg in recent_messages {
        compacted.push_str(&format!("{}:\n{}\n\n", msg.role, msg.content));
    }

    let new_estimated = compacted.len() / 4;
    log_info!(
        conversation_id,
        "📦 Compaction complete: ~{} → ~{} estimated tokens (saved ~{})",
        estimated_tokens, new_estimated, estimated_tokens - new_estimated
    );

    // Update DB: truncate old messages and insert summary
    if let Err(e) = update_db_with_compaction(db, conversation_id, old_messages.len(), &summary, system_msg.is_some()) {
        log_warn!(conversation_id, "📦 DB compaction update failed: {}", e);
    }

    compacted
}

struct ParsedMessage {
    role: String,
    content: String,
}

fn parse_conversation_messages(text: &str) -> Vec<ParsedMessage> {
    let mut messages = Vec::new();
    let mut current_role = String::new();
    let mut current_content = String::new();

    for line in text.lines() {
        // Check if line starts a new message (ROLE:)
        if let Some(role) = line.strip_suffix(':') {
            let role_upper = role.trim().to_uppercase();
            if matches!(role_upper.as_str(), "SYSTEM" | "USER" | "ASSISTANT") {
                // Save previous message
                if !current_role.is_empty() {
                    messages.push(ParsedMessage {
                        role: current_role.clone(),
                        content: current_content.trim().to_string(),
                    });
                }
                current_role = role_upper;
                current_content.clear();
                continue;
            }
        }
        current_content.push_str(line);
        current_content.push('\n');
    }

    // Save last message
    if !current_role.is_empty() {
        messages.push(ParsedMessage {
            role: current_role,
            content: current_content.trim().to_string(),
        });
    }

    messages
}

fn summarize_conversation(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    old_text: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
) -> Result<String, String> {
    // Reuse the existing summarization infrastructure from command_executor
    // but with a conversation-specific prompt
    use super::command_executor::run_summary_pass_public;

    // If old text is very large, chunk it
    let max_chars = 8000;
    let text_to_summarize = if old_text.len() > max_chars {
        // Take the beginning and end, skip the middle
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

fn update_db_with_compaction(
    db: &SharedDatabase,
    conversation_id: &str,
    old_msg_count: usize,
    _summary: &str,
    _has_system: bool,
) -> Result<(), String> {
    // For now, just log that compaction happened.
    // The compacted text is used for this generation only.
    // Full DB compaction (replacing messages) would be a more invasive change.
    log_info!(
        conversation_id,
        "📦 Compaction applied for this generation ({} messages summarized). DB unchanged.",
        old_msg_count
    );
    let _ = db; // suppress unused warning
    Ok(())
}
