// Compaction summary queries and conversation text retrieval

use crate::{db_error, Database};
use rusqlite::{params, OptionalExtension};

use super::{CompactionSummaryRecord, MessageRecord};

impl Database {
    /// Load all compaction summaries for a conversation in ascending coverage order.
    pub fn get_compaction_summaries(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<CompactionSummaryRecord>, String> {
        let conn = self.connection();
        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, covers_from_sequence, covers_to_sequence, message_count, summary_text, created_at \
                 FROM compaction_summaries WHERE conversation_id = ?1 ORDER BY covers_to_sequence ASC",
            )
            .map_err(db_error("prepare compaction summaries"))?;

        let records = stmt
            .query_map([conversation_id], |row| {
                Ok(CompactionSummaryRecord {
                    id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    covers_from_sequence: row.get(2)?,
                    covers_to_sequence: row.get(3)?,
                    message_count: row.get(4)?,
                    summary_text: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })
            .map_err(db_error("query compaction summaries"))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(records)
    }

    /// Get all messages for a conversation (in order), with compaction metadata applied.
    ///
    /// Messages that fall within a compaction summary's range have `compacted=true`.
    /// A synthetic `role='system'` record is injected after each compacted range so the
    /// UI can render the summary divider at the right position.
    pub fn get_messages(&self, conversation_id: &str) -> Result<Vec<MessageRecord>, String> {
        let summaries = self.get_compaction_summaries(conversation_id)?;

        let conn = self.connection();
        let mut stmt = conn
            .prepare(
                "SELECT role, content, timestamp, prompt_tok_per_sec, gen_tok_per_sec, gen_eval_ms, gen_tokens, \
                 prompt_eval_ms, prompt_tokens, sequence_order \
                 FROM messages WHERE conversation_id = ?1 ORDER BY sequence_order ASC",
            )
            .map_err(db_error("prepare messages"))?;

        let raw: Vec<MessageRecord> = stmt
            .query_map([conversation_id], |row| {
                Ok(MessageRecord {
                    role: row.get(0)?,
                    content: row.get(1)?,
                    timestamp: row.get::<_, i64>(2).unwrap_or(0) as u64,
                    prompt_tok_per_sec: row.get(3)?,
                    gen_tok_per_sec: row.get(4)?,
                    gen_eval_ms: row.get(5)?,
                    gen_tokens: row.get(6)?,
                    prompt_eval_ms: row.get(7)?,
                    prompt_tokens: row.get(8)?,
                    compacted: false,
                    sequence_order: row.get(9).unwrap_or(0),
                })
            })
            .map_err(db_error("query messages"))?
            .filter_map(|r| r.ok())
            .collect();

        if summaries.is_empty() {
            return Ok(raw);
        }

        // Merge raw messages with synthetic summary records.
        // Summaries are already sorted by covers_to_sequence ASC.
        let mut output = Vec::with_capacity(raw.len() + summaries.len());
        let mut sum_iter = summaries.iter().peekable();

        for mut msg in raw {
            // Inject summaries that end before this message's sequence position.
            while let Some(s) = sum_iter.peek() {
                if s.covers_to_sequence < msg.sequence_order {
                    output.push(s.as_message_record());
                    sum_iter.next();
                } else {
                    break;
                }
            }

            // Mark non-system messages that fall within any summary range as compacted.
            if msg.role != "system" {
                msg.compacted = summaries.iter().any(|s| {
                    s.covers_from_sequence <= msg.sequence_order
                        && msg.sequence_order <= s.covers_to_sequence
                });
            }

            output.push(msg);
        }

        // Inject any remaining summaries (edge case: summary past last message).
        for s in sum_iter {
            output.push(s.as_message_record());
        }

        Ok(output)
    }

    /// Get conversation as text format for the prompt builder.
    ///
    /// Strategy: find the most recent compaction summary, emit it as a SYSTEM: block,
    /// then emit only the messages that come after its covered range.
    /// If no summary exists, emit all messages from the beginning.
    ///
    /// System prompt and tool definitions are injected separately by the
    /// prompt builder (templates.rs), NOT from conversation text.
    pub fn get_conversation_as_text(&self, conversation_id: &str) -> Result<String, String> {
        let conn = self.connection();

        // Find the latest summary (highest covers_to_sequence).
        let latest = conn
            .query_row(
                "SELECT covers_from_sequence, covers_to_sequence, message_count, summary_text \
                 FROM compaction_summaries WHERE conversation_id = ?1 \
                 ORDER BY covers_to_sequence DESC LIMIT 1",
                [conversation_id],
                |row| {
                    Ok((
                        row.get::<_, i32>(0)?,
                        row.get::<_, i32>(1)?,
                        row.get::<_, i32>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(db_error("get latest compaction summary"))?;

        let mut text = String::new();

        // Emit the summary as a SYSTEM: block so template parsers can inject it into the
        // system prompt (same wire format as before, just sourced from the new table).
        let start_after = if let Some((_, covers_to, message_count, summary_text)) = &latest {
            text.push_str("SYSTEM:\n");
            text.push_str(&format!(
                "[Conversation summary — {} earlier messages compacted]\n{}",
                message_count, summary_text
            ));
            text.push_str("\n\n");
            *covers_to
        } else {
            0
        };

        // Emit user/assistant messages that come after the summarized range.
        let mut stmt = conn
            .prepare(
                "SELECT role, content FROM messages \
                 WHERE conversation_id = ?1 AND sequence_order > ?2 AND role != 'system' \
                 ORDER BY sequence_order ASC",
            )
            .map_err(db_error("prepare conversation text query"))?;

        let rows = stmt
            .query_map(params![conversation_id, start_after], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(db_error("query conversation text"))?;

        for row in rows.filter_map(|r| r.ok()) {
            let (role, content) = row;
            let role_header = match role.as_str() {
                "user" => "USER",
                "assistant" => "ASSISTANT",
                other => other,
            };
            text.push_str(role_header);
            text.push_str(":\n");
            text.push_str(&content);
            text.push_str("\n\n");
        }

        Ok(text)
    }
}
