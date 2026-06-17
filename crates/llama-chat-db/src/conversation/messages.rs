// Message insert, streaming, and timing operations

use crate::{current_timestamp_millis, db_error, Database};
use rusqlite::params;

impl Database {
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

    /// Append an error message to a conversation so it survives page reload.
    /// Uses role="error" so the UI can render it distinctly from normal assistant output.
    pub fn append_error_message(&self, conversation_id: &str, error: &str) -> Result<(), String> {
        let seq = self.get_message_count(conversation_id)?;
        let ts = crate::current_timestamp_secs();
        self.insert_message(conversation_id, "error", error, ts, seq)?;
        Ok(())
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
}
