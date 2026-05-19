// Conversation and message database operations

pub use crate::logger::ConversationLogger;

use super::{
    current_timestamp_millis, current_timestamp_secs, db_error, generate_conversation_id, Database,
};
use rusqlite::{params, OptionalExtension};
use serde_json;
use std::sync::Arc;

#[path = "conversation/compaction.rs"]
mod compaction;


/// Conversation metadata
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ConversationRecord {
    pub id: String,
    pub title: String,
    pub system_prompt: Option<String>,
    pub provider_id: Option<String>,
    pub provider_session_id: Option<String>,
}

/// Message record from database
#[derive(Debug, Clone)]
pub struct MessageRecord {
    pub role: String,
    pub content: String,
    pub timestamp: u64,
    pub prompt_tok_per_sec: Option<f64>,
    pub gen_tok_per_sec: Option<f64>,
    pub gen_eval_ms: Option<f64>,
    pub gen_tokens: Option<i32>,
    pub prompt_eval_ms: Option<f64>,
    pub prompt_tokens: Option<i32>,
    /// True if this message has been compacted (summarized). The model skips these.
    pub compacted: bool,
    /// DB sequence_order for this message — used for precise truncation on edit.
    pub sequence_order: i32,
}

/// A compaction summary — records which message range has been summarized.
#[derive(Debug, Clone)]
pub struct CompactionSummaryRecord {
    pub id: String,
    pub conversation_id: String,
    pub covers_from_sequence: i32,
    pub covers_to_sequence: i32,
    pub message_count: i32,
    pub summary_text: String,
    pub created_at: i64,
}

impl CompactionSummaryRecord {
    /// Convert to a synthetic MessageRecord for UI rendering.
    pub fn as_message_record(&self) -> MessageRecord {
        MessageRecord {
            role: "system".to_string(),
            content: format!(
                "[Conversation summary — {} earlier messages compacted]\n{}",
                self.message_count, self.summary_text
            ),
            timestamp: self.created_at as u64,
            prompt_tok_per_sec: None,
            gen_tok_per_sec: None,
            gen_eval_ms: None,
            gen_tokens: None,
            prompt_eval_ms: None,
            prompt_tokens: None,
            compacted: false,
            sequence_order: self.covers_to_sequence,
        }
    }
}

impl Database {
    /// Create a new conversation and snapshot the current global config into conversation_config.
    pub fn create_conversation(&self, system_prompt: Option<&str>) -> Result<String, String> {
        let id = generate_conversation_id();
        let now = current_timestamp_millis();

        {
            let conn = self.connection();
            conn.execute(
                "INSERT INTO conversations (id, created_at, updated_at, system_prompt, title, provider_id, provider_session_id)
                 VALUES (?1, ?2, ?3, ?4, NULL, NULL, NULL)",
                params![id, now, now, system_prompt],
            )
            .map_err(db_error("create conversation"))?;
        } // Release lock before loading config

        // Snapshot current global config for this conversation
        let global_config = self.load_config();
        let _ = self.save_conversation_config(&id, &global_config);

        Ok(id)
    }

    /// Check if a conversation exists
    pub fn conversation_exists(&self, id: &str) -> Result<bool, String> {
        let conn = self.connection();
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM conversations WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .map_err(db_error("check conversation"))?;

        Ok(count > 0)
    }

    /// Get conversation by ID
    #[allow(dead_code)]
    pub fn get_conversation(&self, id: &str) -> Result<Option<ConversationRecord>, String> {
        let conn = self.connection();
        let result = conn.query_row(
            "SELECT id, COALESCE(title, ''), system_prompt, provider_id, provider_session_id FROM conversations WHERE id = ?1",
            [id],
            |row| {
                Ok(ConversationRecord {
                    id: row.get(0)?,
                    title: row.get::<_, String>(1).unwrap_or_default(),
                    system_prompt: row.get(2)?,
                    provider_id: row.get(3)?,
                    provider_session_id: row.get(4)?,
                })
            },
        );

        match result {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("Failed to get conversation: {e}")),
        }
    }

    /// List all conversations (newest first)
    pub fn list_conversations(&self) -> Result<Vec<ConversationRecord>, String> {
        let conn = self.connection();
        let mut stmt = conn
            .prepare(
                "SELECT id, COALESCE(title, ''), system_prompt, provider_id, provider_session_id FROM conversations ORDER BY created_at DESC",
            )
            .map_err(db_error("prepare statement"))?;

        let records = stmt
            .query_map([], |row| {
                Ok(ConversationRecord {
                    id: row.get(0)?,
                    title: row.get::<_, String>(1).unwrap_or_default(),
                    system_prompt: row.get(2)?,
                    provider_id: row.get(3)?,
                    provider_session_id: row.get(4)?,
                })
            })
            .map_err(db_error("query conversations"))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(records)
    }

    /// Delete a conversation (cascades to messages)
    pub fn delete_conversation(&self, id: &str) -> Result<(), String> {
        let conn = self.connection();

        // Delete streaming buffer first
        conn.execute(
            "DELETE FROM streaming_buffer WHERE conversation_id = ?1",
            [id],
        )
        .map_err(db_error("delete streaming buffer"))?;

        // Delete messages (should cascade but be explicit)
        conn.execute("DELETE FROM messages WHERE conversation_id = ?1", [id])
            .map_err(db_error("delete messages"))?;

        // Delete conversation
        conn.execute("DELETE FROM conversations WHERE id = ?1", [id])
            .map_err(db_error("delete conversation"))?;

        Ok(())
    }

    /// Update conversation timestamp
    pub fn update_conversation_timestamp(&self, id: &str) -> Result<(), String> {
        let conn = self.connection();
        conn.execute(
            "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
            params![current_timestamp_millis(), id],
        )
        .map_err(db_error("update conversation timestamp"))?;

        Ok(())
    }

    /// Update conversation title
    pub fn update_conversation_title(&self, id: &str, title: &str) -> Result<(), String> {
        let conn = self.connection();
        conn.execute(
            "UPDATE conversations SET title = ?1 WHERE id = ?2",
            params![title, id],
        )
        .map_err(db_error("update conversation title"))?;
        Ok(())
    }

    /// Get conversation title (returns None if not set)
    pub fn get_conversation_title(&self, id: &str) -> Result<Option<String>, String> {
        let conn = self.connection();
        let result = conn.query_row(
            "SELECT title FROM conversations WHERE id = ?1",
            [id],
            |row| row.get::<_, Option<String>>(0),
        );
        match result {
            Ok(title) => Ok(title),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("Failed to get conversation title: {e}")),
        }
    }

    /// Persist a provider-side session handle for a conversation.
    pub fn set_conversation_provider_session_id(
        &self,
        id: &str,
        provider_id: Option<&str>,
        provider_session_id: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.connection();
        conn.execute(
            "UPDATE conversations SET provider_id = ?1, provider_session_id = ?2 WHERE id = ?3",
            params![provider_id, provider_session_id, id],
        )
        .map_err(db_error("update conversation provider session id"))?;
        Ok(())
    }

    /// Get the provider identity + session handle for a conversation.
    pub fn get_conversation_provider_session(
        &self,
        id: &str,
    ) -> Result<(Option<String>, Option<String>), String> {
        let conn = self.connection();
        let result = conn.query_row(
            "SELECT provider_id, provider_session_id FROM conversations WHERE id = ?1",
            [id],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?)),
        );
        match result {
            Ok(provider_session) => Ok(provider_session),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok((None, None)),
            Err(e) => Err(format!("Failed to get conversation provider session id: {e}")),
        }
    }

    // ─── Message Queue (for injecting user messages during remote provider generation) ───

    /// Push a message onto the queue for a conversation.
    pub fn queue_message(&self, conversation_id: &str, content: &str) -> Result<(), String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let conn = self.connection();
        conn.execute(
            "INSERT INTO message_queue (conversation_id, content, created_at) VALUES (?1, ?2, ?3)",
            params![conversation_id, content, now],
        ).map_err(db_error("queue message"))?;
        Ok(())
    }

    /// Pop all queued messages for a conversation (returns and deletes them).
    pub fn pop_queued_messages(&self, conversation_id: &str) -> Vec<String> {
        let conn = self.connection();
        let mut stmt = match conn.prepare(
            "SELECT id, content FROM message_queue WHERE conversation_id = ?1 ORDER BY id ASC"
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows: Vec<(i64, String)> = stmt
            .query_map([conversation_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .ok()
            .map(|iter| iter.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        if !rows.is_empty() {
            let ids: Vec<String> = rows.iter().map(|(id, _)| id.to_string()).collect();
            let _ = conn.execute(
                &format!("DELETE FROM message_queue WHERE id IN ({})", ids.join(",")),
                [],
            );
        }
        rows.into_iter().map(|(_, content)| content).collect()
    }

    /// Insert a complete message
    pub fn insert_message(
        &self,
        conversation_id: &str,
        role: &str,
        content: &str,
        timestamp: u64,
        sequence_order: i32,
    ) -> Result<String, String> {
        self.insert_message_with_tokens(conversation_id, role, content, timestamp, sequence_order, None)
    }

    /// Insert a message with an optional pre-computed token count.
    pub fn insert_message_with_tokens(
        &self,
        conversation_id: &str,
        role: &str,
        content: &str,
        timestamp: u64,
        sequence_order: i32,
        token_count: Option<i32>,
    ) -> Result<String, String> {
        let message_id = uuid::Uuid::new_v4().to_string();
        let conn = self.connection();

        conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content, timestamp, sequence_order, is_streaming, token_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7)",
            params![message_id, conversation_id, role, content, timestamp as i64, sequence_order, token_count],
        )
        .map_err(db_error("insert message"))?;

        Ok(message_id)
    }

    /// Insert a streaming message placeholder
    pub fn insert_streaming_message(
        &self,
        conversation_id: &str,
        message_id: &str,
        timestamp: u64,
        sequence_order: i32,
    ) -> Result<(), String> {
        let conn = self.connection();

        conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content, timestamp, sequence_order, is_streaming)
             VALUES (?1, ?2, 'assistant', '', ?3, ?4, 1)",
            params![message_id, conversation_id, timestamp as i64, sequence_order],
        )
        .map_err(db_error("insert streaming message"))?;

        Ok(())
    }

    /// Initialize streaming buffer for a conversation
    pub fn init_streaming_buffer(
        &self,
        conversation_id: &str,
        message_id: &str,
    ) -> Result<(), String> {
        let conn = self.connection();
        let now = current_timestamp_millis();

        conn.execute(
            "INSERT OR REPLACE INTO streaming_buffer
             (conversation_id, message_id, partial_content, tokens_used, max_tokens, updated_at)
             VALUES (?1, ?2, '', 0, 0, ?3)",
            params![conversation_id, message_id, now],
        )
        .map_err(db_error("init streaming buffer"))?;

        Ok(())
    }

    /// Update streaming buffer with new content
    #[allow(dead_code)]
    pub fn update_streaming_buffer(
        &self,
        conversation_id: &str,
        partial_content: &str,
        tokens_used: i32,
        max_tokens: i32,
    ) -> Result<(), String> {
        let conn = self.connection();
        let now = current_timestamp_millis();

        conn.execute(
            "UPDATE streaming_buffer
             SET partial_content = ?1, tokens_used = ?2, max_tokens = ?3, updated_at = ?4
             WHERE conversation_id = ?5",
            params![
                partial_content,
                tokens_used,
                max_tokens,
                now,
                conversation_id
            ],
        )
        .map_err(db_error("update streaming buffer"))?;

        Ok(())
    }

    /// Finalize a streaming message (copy content, clear is_streaming flag)
    pub fn finalize_streaming_message(
        &self,
        message_id: &str,
        content: &str,
    ) -> Result<(), String> {
        let conn = self.connection();

        conn.execute(
            "UPDATE messages SET content = ?1, is_streaming = 0 WHERE id = ?2",
            params![content, message_id],
        )
        .map_err(db_error("finalize streaming message"))?;

        Ok(())
    }

    /// Store generation timing metrics and token count on a message row.
    pub fn update_message_timings(
        &self,
        message_id: &str,
        prompt_tok_per_sec: Option<f64>,
        gen_tok_per_sec: Option<f64>,
        gen_eval_ms: Option<f64>,
        gen_tokens: Option<i32>,
        prompt_eval_ms: Option<f64>,
        prompt_tokens: Option<i32>,
    ) -> Result<(), String> {
        let conn = self.connection();
        conn.execute(
            "UPDATE messages SET prompt_tok_per_sec = ?1, gen_tok_per_sec = ?2, gen_eval_ms = ?3, gen_tokens = ?4, prompt_eval_ms = ?5, prompt_tokens = ?6, token_count = ?4 WHERE id = ?7",
            params![prompt_tok_per_sec, gen_tok_per_sec, gen_eval_ms, gen_tokens, prompt_eval_ms, prompt_tokens, message_id],
        )
        .map_err(db_error("update message timings"))?;
        Ok(())
    }

    /// Delete streaming buffer for a conversation
    pub fn delete_streaming_buffer(&self, conversation_id: &str) -> Result<(), String> {
        let conn = self.connection();

        conn.execute(
            "DELETE FROM streaming_buffer WHERE conversation_id = ?1",
            [conversation_id],
        )
        .map_err(db_error("delete streaming buffer"))?;

        Ok(())
    }

    /// Get message count for a conversation
    pub fn get_message_count(&self, conversation_id: &str) -> Result<i32, String> {
        let conn = self.connection();
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1",
                [conversation_id],
                |row| row.get(0),
            )
            .map_err(db_error("get message count"))?;

        Ok(count)
    }

    /// Delete all messages with sequence_order >= from_sequence.
    /// Also deletes any compaction summaries that cover messages in the deleted range
    /// (so the model automatically reverts to full context from the nearest surviving summary).
    /// Returns the number of deleted messages.
    pub fn truncate_messages(&self, conversation_id: &str, from_sequence: i32) -> Result<usize, String> {
        let conn = self.connection();

        // Any summary whose range ends at or after from_sequence is invalidated: it covers
        // messages that are about to be deleted, so the model must fall back to raw messages.
        conn.execute(
            "DELETE FROM compaction_summaries WHERE conversation_id = ?1 AND covers_to_sequence >= ?2",
            rusqlite::params![conversation_id, from_sequence],
        )
        .map_err(db_error("delete invalidated summaries"))?;

        let deleted = conn
            .execute(
                "DELETE FROM messages WHERE conversation_id = ?1 AND sequence_order >= ?2",
                rusqlite::params![conversation_id, from_sequence],
            )
            .map_err(db_error("truncate messages"))?;

        Ok(deleted)
    }

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

#[cfg(test)]
mod tests;
