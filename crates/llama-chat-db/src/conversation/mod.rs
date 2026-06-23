// Conversation and message database operations

pub use crate::logger::ConversationLogger;

use super::{current_timestamp_millis, db_error, generate_conversation_id, Database};
use rusqlite::params;

mod compaction;
mod messages;
mod queries;

/// Conversation metadata
#[derive(Debug, Clone)]
pub struct ConversationRecord {
    pub id: String,
    pub title: String,
    pub worker_id: Option<String>,
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
    /// Structured parts JSON (remote provider only). None for local-model messages.
    pub parts: Option<String>,
    /// LLM-generated short title (≤50 chars). Set by background title gen; user messages only.
    pub title: Option<String>,
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
                "[Compacted history — {} messages. This is REAL work already done. Tasks marked COMPLETE were actually executed — do NOT repeat them.]\n{}",
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
            parts: None,
            title: None,
        }
    }
}

impl Database {
    /// Create a new conversation.
    pub fn create_conversation(&self) -> Result<String, String> {
        let id = generate_conversation_id();
        let now = current_timestamp_millis();

        {
            let conn = self.connection();
            conn.execute(
                "INSERT INTO conversations (id, created_at, updated_at) VALUES (?1, ?2, ?3)",
                params![id, now, now],
            )
            .map_err(db_error("create conversation"))?;
        }

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
            "SELECT id, COALESCE(title, ''), worker_id, provider_id, provider_session_id FROM conversations WHERE id = ?1",
            [id],
            |row| {
                Ok(ConversationRecord {
                    id: row.get(0)?,
                    title: row.get::<_, String>(1).unwrap_or_default(),
                    worker_id: row.get(2)?,
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
                "SELECT id, COALESCE(title, ''), worker_id, provider_id, provider_session_id FROM conversations ORDER BY created_at DESC",
            )
            .map_err(db_error("prepare statement"))?;

        let records = stmt
            .query_map([], |row| {
                Ok(ConversationRecord {
                    id: row.get(0)?,
                    title: row.get::<_, String>(1).unwrap_or_default(),
                    worker_id: row.get(2)?,
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
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        );
        match result {
            Ok(provider_session) => Ok(provider_session),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok((None, None)),
            Err(e) => Err(format!(
                "Failed to get conversation provider session id: {e}"
            )),
        }
    }

    /// Get the worker binding for a conversation. NULL means "default worker".
    pub fn get_conversation_worker_id(&self, id: &str) -> Result<Option<String>, String> {
        let conn = self.connection();
        let result = conn.query_row(
            "SELECT worker_id FROM conversations WHERE id = ?1",
            [id],
            |row| row.get::<_, Option<String>>(0),
        );
        match result {
            Ok(worker_id) => Ok(worker_id),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("Failed to get conversation worker id: {e}")),
        }
    }

    /// Set or clear the worker binding for a conversation. None means "default worker".
    pub fn set_conversation_worker_id(
        &self,
        id: &str,
        worker_id: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.connection();
        conn.execute(
            "UPDATE conversations SET worker_id = ?1 WHERE id = ?2",
            params![worker_id, id],
        )
        .map_err(db_error("update conversation worker id"))?;
        Ok(())
    }

    /// Clear worker binding for all conversations bound to the given worker.
    pub fn clear_worker_id_for_worker(&self, worker_id: &str) -> Result<usize, String> {
        let conn = self.connection();
        let updated = conn
            .execute(
                "UPDATE conversations SET worker_id = NULL WHERE worker_id = ?1",
                [worker_id],
            )
            .map_err(db_error("clear worker binding for conversations"))?;
        Ok(updated)
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
        )
        .map_err(db_error("queue message"))?;
        Ok(())
    }

    /// Pop all queued messages for a conversation (returns and deletes them).
    pub fn pop_queued_messages(&self, conversation_id: &str) -> Vec<String> {
        let conn = self.connection();
        let mut stmt = match conn.prepare(
            "SELECT id, content FROM message_queue WHERE conversation_id = ?1 ORDER BY id ASC",
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
}

#[cfg(test)]
mod tests;
