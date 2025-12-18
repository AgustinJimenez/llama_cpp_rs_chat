// Conversation and message database operations

use super::{
    current_timestamp_millis, current_timestamp_secs, db_error, generate_conversation_id, Database,
    StreamingUpdate,
};
use rusqlite::params;
use std::sync::Arc;
use std::time::{Duration, Instant};

// Import logging macros
use crate::sys_error;

/// Conversation metadata
#[derive(Debug, Clone)]
pub struct ConversationRecord {
    pub id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub system_prompt: Option<String>,
    pub title: Option<String>,
}

/// Message record from database
#[derive(Debug, Clone)]
pub struct MessageRecord {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub content: String,
    pub timestamp: u64,
    pub sequence_order: i32,
    pub is_streaming: bool,
}

impl Database {
    /// Create a new conversation
    pub fn create_conversation(&self, system_prompt: Option<&str>) -> Result<String, String> {
        let id = generate_conversation_id();
        let now = current_timestamp_millis();

        let conn = self.connection();
        conn.execute(
            "INSERT INTO conversations (id, created_at, updated_at, system_prompt, title)
             VALUES (?1, ?2, ?3, ?4, NULL)",
            params![id, now, now, system_prompt],
        )
        .map_err(db_error("create conversation"))?;

        Ok(id)
    }

    /// Create a conversation with a specific ID (for migration)
    pub fn create_conversation_with_id(
        &self,
        id: &str,
        system_prompt: Option<&str>,
        created_at: i64,
    ) -> Result<(), String> {
        let conn = self.connection();
        conn.execute(
            "INSERT INTO conversations (id, created_at, updated_at, system_prompt, title)
             VALUES (?1, ?2, ?3, ?4, NULL)",
            params![id, created_at, created_at, system_prompt],
        )
        .map_err(db_error("create conversation"))?;

        Ok(())
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
    pub fn get_conversation(&self, id: &str) -> Result<Option<ConversationRecord>, String> {
        let conn = self.connection();
        let result = conn.query_row(
            "SELECT id, created_at, updated_at, system_prompt, title
             FROM conversations WHERE id = ?1",
            [id],
            |row| {
                Ok(ConversationRecord {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    updated_at: row.get(2)?,
                    system_prompt: row.get(3)?,
                    title: row.get(4)?,
                })
            },
        );

        match result {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("Failed to get conversation: {}", e)),
        }
    }

    /// List all conversations (newest first)
    pub fn list_conversations(&self) -> Result<Vec<ConversationRecord>, String> {
        let conn = self.connection();
        let mut stmt = conn
            .prepare(
                "SELECT id, created_at, updated_at, system_prompt, title
                 FROM conversations ORDER BY created_at DESC",
            )
            .map_err(db_error("prepare statement"))?;

        let records = stmt
            .query_map([], |row| {
                Ok(ConversationRecord {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    updated_at: row.get(2)?,
                    system_prompt: row.get(3)?,
                    title: row.get(4)?,
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

    /// Insert a complete message
    pub fn insert_message(
        &self,
        conversation_id: &str,
        role: &str,
        content: &str,
        timestamp: u64,
        sequence_order: i32,
    ) -> Result<String, String> {
        let message_id = uuid::Uuid::new_v4().to_string();
        let conn = self.connection();

        conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content, timestamp, sequence_order, is_streaming)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
            params![message_id, conversation_id, role, content, timestamp as i64, sequence_order],
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

    /// Get all messages for a conversation (in order)
    pub fn get_messages(&self, conversation_id: &str) -> Result<Vec<MessageRecord>, String> {
        let conn = self.connection();
        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, role, content, timestamp, sequence_order, is_streaming
                 FROM messages WHERE conversation_id = ?1 ORDER BY sequence_order ASC",
            )
            .map_err(db_error("prepare statement"))?;

        let messages = stmt
            .query_map([conversation_id], |row| {
                Ok(MessageRecord {
                    id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    timestamp: row.get::<_, i64>(4)? as u64,
                    sequence_order: row.get(5)?,
                    is_streaming: row.get::<_, i32>(6)? != 0,
                })
            })
            .map_err(db_error("query messages"))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(messages)
    }

    /// Get conversation as text format (for backward compatibility with file-based format)
    pub fn get_conversation_as_text(&self, conversation_id: &str) -> Result<String, String> {
        let conv = self.get_conversation(conversation_id)?;
        let messages = self.get_messages(conversation_id)?;

        let mut text = String::new();

        // Add system prompt if present and not already stored as a system message.
        let has_system_message = messages.iter().any(|msg| msg.role == "system");
        if let Some(ref conv) = conv {
            if let Some(ref prompt) = conv.system_prompt {
                if !has_system_message {
                    text.push_str("SYSTEM:\n");
                    text.push_str(prompt);
                    text.push_str("\n\n");
                }
            }
        }

        // Add messages
        for msg in messages {
            let role_header = match msg.role.as_str() {
                "user" => "USER",
                "assistant" => "ASSISTANT",
                "system" => "SYSTEM",
                _ => &msg.role.to_uppercase(),
            };

            text.push_str(role_header);
            text.push_str(":\n");
            text.push_str(&msg.content);
            text.push_str("\n\n");
        }

        Ok(text)
    }
}

/// SQLite-backed conversation logger
/// Replaces the file-based ConversationLogger
pub struct ConversationLogger {
    db: Arc<Database>,
    conversation_id: String,
    current_message_id: Option<String>,
    accumulated_content: String,
    sequence_counter: i32,
    last_broadcast_at: Option<Instant>,
    last_broadcast_len: usize,
}

const STREAM_BROADCAST_MIN_INTERVAL: Duration = Duration::from_millis(200);
const STREAM_BROADCAST_MIN_CHARS: usize = 64;

impl ConversationLogger {
    /// Create a new conversation
    pub fn new(db: Arc<Database>, system_prompt: Option<&str>) -> Result<Self, String> {
        let conversation_id = db.create_conversation(system_prompt)?;

        let sequence_counter = if system_prompt.is_some() {
            // Insert system message
            let now = current_timestamp_secs();
            db.insert_message(&conversation_id, "system", system_prompt.unwrap(), now, 0)?;
            1
        } else {
            0
        };

        Ok(Self {
            db,
            conversation_id,
            current_message_id: None,
            accumulated_content: String::new(),
            sequence_counter,
            last_broadcast_at: None,
            last_broadcast_len: 0,
        })
    }

    /// Load an existing conversation
    pub fn from_existing(db: Arc<Database>, conversation_id: &str) -> Result<Self, String> {
        // Remove .txt extension if present (for backward compatibility)
        let id = conversation_id.trim_end_matches(".txt");

        if !db.conversation_exists(id)? {
            return Err(format!("Conversation {} not found", id));
        }

        let sequence_counter = db.get_message_count(id)?;

        Ok(Self {
            db,
            conversation_id: id.to_string(),
            current_message_id: None,
            accumulated_content: String::new(),
            sequence_counter,
            last_broadcast_at: None,
            last_broadcast_len: 0,
        })
    }

    /// Log a complete message (typically user message)
    pub fn log_message(&mut self, role: &str, message: &str) {
        let timestamp = current_timestamp_secs();
        let role_lower = role.to_lowercase();

        if let Err(e) = self.db.insert_message(
            &self.conversation_id,
            &role_lower,
            message,
            timestamp,
            self.sequence_counter,
        ) {
            sys_error!("Failed to log message: {}", e);
            return;
        }

        self.sequence_counter += 1;

        // Update conversation timestamp
        let _ = self.db.update_conversation_timestamp(&self.conversation_id);
    }

    /// Start streaming an assistant message
    pub fn start_assistant_message(&mut self) {
        let message_id = uuid::Uuid::new_v4().to_string();
        let timestamp = current_timestamp_secs();

        // Insert placeholder message with is_streaming = 1
        if let Err(e) = self.db.insert_streaming_message(
            &self.conversation_id,
            &message_id,
            timestamp,
            self.sequence_counter,
        ) {
            sys_error!("Failed to start streaming message: {}", e);
            return;
        }

        // Initialize streaming buffer
        if let Err(e) = self
            .db
            .init_streaming_buffer(&self.conversation_id, &message_id)
        {
            sys_error!("Failed to init streaming buffer: {}", e);
        }

        self.current_message_id = Some(message_id);
        self.accumulated_content.clear();
        self.sequence_counter += 1;
        self.last_broadcast_at = None;
        self.last_broadcast_len = 0;
    }

    /// Append a token to the current streaming message
    pub fn log_token(&mut self, token: &str) {
        self.accumulated_content.push_str(token);

        if self.current_message_id.is_some() {
            // Update streaming buffer
            if let Err(e) = self.db.update_streaming_buffer(
                &self.conversation_id,
                &self.accumulated_content,
                0, // tokens_used updated separately
                0, // max_tokens updated separately
            ) {
                sys_error!("Failed to update streaming buffer: {}", e);
            }

            // Broadcast to WebSocket subscribers with throttling to avoid disconnects.
            let now = Instant::now();
            let len = self.accumulated_content.len();
            let should_broadcast = match self.last_broadcast_at {
                None => true,
                Some(last_at) => {
                    let elapsed = now.duration_since(last_at);
                    elapsed >= STREAM_BROADCAST_MIN_INTERVAL
                        && len.saturating_sub(self.last_broadcast_len) >= STREAM_BROADCAST_MIN_CHARS
                }
            };

            if should_broadcast {
                self.last_broadcast_at = Some(now);
                self.last_broadcast_len = len;
                self.db.broadcast_streaming_update(StreamingUpdate {
                    conversation_id: self.conversation_id.clone(),
                    partial_content: self.accumulated_content.clone(),
                    tokens_used: 0,
                    max_tokens: 0,
                    is_complete: false,
                });
            }
        }
    }

    /// Finish the current streaming message
    pub fn finish_assistant_message(&mut self) {
        if let Some(ref msg_id) = self.current_message_id {
            // Update the message with final content
            if let Err(e) = self
                .db
                .finalize_streaming_message(msg_id, &self.accumulated_content)
            {
                sys_error!("Failed to finalize streaming message: {}", e);
            }

            // Clean up streaming buffer
            if let Err(e) = self.db.delete_streaming_buffer(&self.conversation_id) {
                sys_error!("Failed to clean streaming buffer: {}", e);
            }

            // Broadcast completion
            self.last_broadcast_at = Some(Instant::now());
            self.last_broadcast_len = self.accumulated_content.len();
            self.db.broadcast_streaming_update(StreamingUpdate {
                conversation_id: self.conversation_id.clone(),
                partial_content: self.accumulated_content.clone(),
                tokens_used: 0,
                max_tokens: 0,
                is_complete: true,
            });
        }

        self.current_message_id = None;
        self.accumulated_content.clear();

        // Update conversation timestamp
        let _ = self.db.update_conversation_timestamp(&self.conversation_id);
    }

    /// Get the conversation ID
    pub fn get_conversation_id(&self) -> String {
        // Return with .txt extension for backward compatibility
        format!("{}.txt", self.conversation_id)
    }

    /// Get full conversation content as text (backward compatibility)
    pub fn get_full_conversation(&self) -> String {
        self.db
            .get_conversation_as_text(&self.conversation_id)
            .unwrap_or_default()
    }

    /// Load conversation from database (replaces load_conversation_from_file)
    pub fn load_conversation_from_file(&self) -> std::io::Result<String> {
        self.db
            .get_conversation_as_text(&self.conversation_id)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_db() -> Arc<Database> {
        Arc::new(Database::new(":memory:").unwrap())
    }

    #[test]
    fn test_create_conversation() {
        let db = create_test_db();
        let id = db.create_conversation(Some("Test prompt")).unwrap();
        assert!(id.starts_with("chat_"));
        assert!(db.conversation_exists(&id).unwrap());
    }

    #[test]
    fn test_insert_and_get_messages() {
        let db = create_test_db();
        let conv_id = db.create_conversation(None).unwrap();

        db.insert_message(&conv_id, "user", "Hello", 1234567890, 0)
            .unwrap();
        db.insert_message(&conv_id, "assistant", "Hi there!", 1234567891, 1)
            .unwrap();

        let messages = db.get_messages(&conv_id).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "Hello");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].content, "Hi there!");
    }

    #[test]
    fn test_conversation_logger() {
        let db = create_test_db();
        let mut logger = ConversationLogger::new(db.clone(), Some("System prompt")).unwrap();

        logger.log_message("USER", "Hello");
        logger.start_assistant_message();
        logger.log_token("Hi ");
        logger.log_token("there!");
        logger.finish_assistant_message();

        let text = logger.get_full_conversation();
        assert!(text.contains("SYSTEM:\nSystem prompt"));
        assert!(text.contains("USER:\nHello"));
        assert!(text.contains("ASSISTANT:\nHi there!"));
    }

    #[test]
    fn test_delete_conversation() {
        let db = create_test_db();
        let id = db.create_conversation(None).unwrap();
        db.insert_message(&id, "user", "Test", 0, 0).unwrap();

        assert!(db.conversation_exists(&id).unwrap());
        db.delete_conversation(&id).unwrap();
        assert!(!db.conversation_exists(&id).unwrap());
    }
}
