//! CRUD operations for the conversation_context table.
//!
//! Stores cached system prompt and tool definition metadata per conversation,
//! enabling accurate token budgeting for compaction and context management.

use super::{db_error, Database};
use rusqlite::params;

/// Cached context metadata for a conversation.
#[allow(dead_code)]
pub struct ConversationContext {
    pub conversation_id: String,
    pub system_prompt_text: Option<String>,
    pub system_prompt_tokens: i32,
    pub tool_definitions_json: Option<String>,
    pub tool_definitions_tokens: i32,
    pub content_hash: Option<String>,
    pub updated_at: i64,
}

impl Database {
    /// Get the cached conversation context, if any.
    #[allow(dead_code)]
    pub fn get_conversation_context(&self, conversation_id: &str) -> Option<ConversationContext> {
        let conn = self.connection();
        conn.query_row(
            "SELECT conversation_id, system_prompt_text, system_prompt_tokens, tool_definitions_json, tool_definitions_tokens, content_hash, updated_at FROM conversation_context WHERE conversation_id = ?1",
            [conversation_id],
            |row| {
                Ok(ConversationContext {
                    conversation_id: row.get(0)?,
                    system_prompt_text: row.get(1)?,
                    system_prompt_tokens: row.get(2)?,
                    tool_definitions_json: row.get(3)?,
                    tool_definitions_tokens: row.get(4)?,
                    content_hash: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            },
        )
        .ok()
    }

    /// Insert or update conversation context (upsert).
    pub fn upsert_conversation_context(
        &self,
        conversation_id: &str,
        system_prompt_text: &str,
        system_prompt_tokens: i32,
        tool_definitions_json: &str,
        tool_definitions_tokens: i32,
        content_hash: &str,
    ) -> Result<(), String> {
        let conn = self.connection();
        let now = super::current_timestamp_millis() / 1000;
        conn.execute(
            "INSERT OR REPLACE INTO conversation_context (conversation_id, system_prompt_text, system_prompt_tokens, tool_definitions_json, tool_definitions_tokens, content_hash, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![conversation_id, system_prompt_text, system_prompt_tokens, tool_definitions_json, tool_definitions_tokens, content_hash, now],
        )
        .map_err(db_error("upsert conversation context"))?;
        Ok(())
    }

    /// Get the actual token_pos from the most recent generation for a conversation.
    /// This is the real KV cache position — what the model actually had in context.
    /// Returns None if no generation has completed yet.
    pub fn get_last_generation_token_pos(&self, conversation_id: &str) -> Option<usize> {
        let conn = self.connection();
        let message: Option<String> = conn.query_row(
            "SELECT message FROM logs WHERE conversation_id = ?1 AND level = 'metrics' ORDER BY timestamp DESC LIMIT 1",
            [conversation_id],
            |row| row.get(0),
        )
        .ok()?;
        let v: serde_json::Value = serde_json::from_str(&message?).ok()?;
        let tokens_used = v["tokens_used"].as_i64()?;
        if tokens_used > 0 { Some(tokens_used as usize) } else { None }
    }

    /// Get total overhead tokens (system prompt + tool definitions) for a conversation.
    /// Returns 0 if no context has been cached yet.
    pub fn get_context_overhead_tokens(&self, conversation_id: &str) -> i32 {
        let conn = self.connection();
        conn.query_row(
            "SELECT system_prompt_tokens + tool_definitions_tokens FROM conversation_context WHERE conversation_id = ?1",
            [conversation_id],
            |row| row.get::<_, i32>(0),
        )
        .unwrap_or(0)
    }

    /// Get the content hash for dirty-checking.
    #[allow(dead_code)]
    pub fn get_context_hash(&self, conversation_id: &str) -> Option<String> {
        let conn = self.connection();
        conn.query_row(
            "SELECT content_hash FROM conversation_context WHERE conversation_id = ?1",
            [conversation_id],
            |row| row.get(0),
        )
        .ok()
    }
}
