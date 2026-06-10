//! ConversationLogger — high-level streaming write interface on top of the Database.
//!
//! Wraps `Arc<Database>` and manages the per-generation streaming state:
//! accumulated content, WebSocket broadcast throttling, and periodic DB flushing
//! for crash recovery. Database CRUD lives in `conversation.rs`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{current_timestamp_secs, Database, StreamingUpdate};

const STREAM_BROADCAST_MIN_INTERVAL: Duration = Duration::from_millis(200);
const STREAM_BROADCAST_MIN_CHARS: usize = 64;
/// How often to persist streaming content to DB for crash recovery.
const DB_FLUSH_INTERVAL: Duration = Duration::from_secs(5);

/// SQLite-backed conversation logger.
///
/// Replaces the file-based `ConversationLogger`. One instance per active generation;
/// created by `ConversationLogger::new` (new conversation) or `from_existing` (resume).
pub struct ConversationLogger {
    pub(crate) db: Arc<Database>,
    conversation_id: String,
    current_message_id: Option<String>,
    /// Preserved after finish_assistant_message so timings can be stored.
    last_finished_message_id: Option<String>,
    accumulated_content: String,
    sequence_counter: i32,
    last_broadcast_at: Option<Instant>,
    last_broadcast_len: usize,
    /// Latest token position from generation (avoids re-tokenization in watchers)
    current_tokens_used: i32,
    /// Context size from generation (avoids re-tokenization in watchers)
    current_max_tokens: i32,
    /// Last time we flushed accumulated content to the DB (crash recovery)
    last_db_flush: Option<Instant>,
    /// Length of accumulated_content at last DB flush
    last_db_flush_len: usize,
}

impl ConversationLogger {
    /// Create a new conversation.
    pub fn new(db: Arc<Database>, system_prompt: Option<&str>) -> Result<Self, String> {
        let conversation_id = db.create_conversation()?;

        let sequence_counter = if let Some(sp) = system_prompt {
            // Insert system message
            let now = current_timestamp_secs();
            db.insert_message(&conversation_id, "system", sp, now, 0)?;
            1
        } else {
            0
        };

        Ok(Self {
            db,
            conversation_id,
            current_message_id: None,
            last_finished_message_id: None,
            accumulated_content: String::new(),
            sequence_counter,
            last_broadcast_at: None,
            last_broadcast_len: 0,
            current_tokens_used: 0,
            current_max_tokens: 0,
            last_db_flush: None,
            last_db_flush_len: 0,
        })
    }

    /// Load an existing conversation.
    pub fn from_existing(db: Arc<Database>, conversation_id: &str) -> Result<Self, String> {
        if !db.conversation_exists(conversation_id)? {
            return Err(format!("Conversation {conversation_id} not found"));
        }

        let sequence_counter = db.get_message_count(conversation_id)?;

        Ok(Self {
            db,
            conversation_id: conversation_id.to_string(),
            current_message_id: None,
            last_finished_message_id: None,
            accumulated_content: String::new(),
            sequence_counter,
            last_broadcast_at: None,
            last_broadcast_len: 0,
            current_tokens_used: 0,
            current_max_tokens: 0,
            last_db_flush: None,
            last_db_flush_len: 0,
        })
    }

    /// Log a complete message (typically a user message).
    pub fn log_message(&mut self, role: &str, message: &str) {
        self.log_message_with_tokens(role, message, None);
    }

    /// Log a message with an optional pre-computed token count.
    pub fn log_message_with_tokens(
        &mut self,
        role: &str,
        message: &str,
        token_count: Option<i32>,
    ) {
        let timestamp = current_timestamp_secs();
        let role_lower = role.to_lowercase();

        if let Err(e) = self.db.insert_message_with_tokens(
            &self.conversation_id,
            &role_lower,
            message,
            timestamp,
            self.sequence_counter,
            token_count,
        ) {
            sys_error!("Failed to log message: {}", e);
            return;
        }

        self.sequence_counter += 1;

        // Update conversation timestamp
        let _ = self.db.update_conversation_timestamp(&self.conversation_id);
    }

    /// Start streaming an assistant message.
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

    /// Update token counts from the generation loop (call before log_token).
    pub fn set_token_counts(&mut self, tokens_used: i32, max_tokens: i32) {
        self.current_tokens_used = tokens_used;
        self.current_max_tokens = max_tokens;
    }

    /// Append a token to the current streaming message.
    ///
    /// Only accumulates in memory + broadcasts via WebSocket (throttled).
    /// DB writes happen only at `finish_assistant_message()` — keeps generation non-blocking.
    pub fn log_token(&mut self, token: &str) {
        self.accumulated_content.push_str(token);

        if self.current_message_id.is_some() {
            // Throttle WebSocket broadcasts to avoid overwhelming clients
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
                    tokens_used: self.current_tokens_used,
                    max_tokens: self.current_max_tokens,
                    is_complete: false,
                });
            }
        }
    }

    /// Append a bulk chunk of token content and broadcast if needed.
    ///
    /// More efficient than per-token `log_token()` — called from periodic sync.
    pub fn log_token_bulk(&mut self, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        self.accumulated_content.push_str(chunk);

        if let Some(ref msg_id) = self.current_message_id {
            let now = Instant::now();
            let len = self.accumulated_content.len();
            let should_broadcast = match self.last_broadcast_at {
                None => true,
                Some(last_at) => {
                    now.duration_since(last_at) >= STREAM_BROADCAST_MIN_INTERVAL
                        && len.saturating_sub(self.last_broadcast_len) >= STREAM_BROADCAST_MIN_CHARS
                }
            };

            if should_broadcast {
                self.last_broadcast_at = Some(now);
                self.last_broadcast_len = len;
                self.db.broadcast_streaming_update(StreamingUpdate {
                    conversation_id: self.conversation_id.clone(),
                    partial_content: self.accumulated_content.clone(),
                    tokens_used: self.current_tokens_used,
                    max_tokens: self.current_max_tokens,
                    is_complete: false,
                });
            }

            // Periodic DB flush for crash recovery — save content so far
            let should_flush = match self.last_db_flush {
                None => true,
                Some(last) => {
                    now.duration_since(last) >= DB_FLUSH_INTERVAL && len > self.last_db_flush_len
                }
            };
            if should_flush {
                let conn = self.db.connection();
                let _ = conn.execute(
                    "UPDATE messages SET content = ?1 WHERE id = ?2",
                    rusqlite::params![&self.accumulated_content, msg_id],
                );
                self.last_db_flush = Some(now);
                self.last_db_flush_len = len;
            }
        }
    }

    /// Finish the current streaming message.
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
                tokens_used: self.current_tokens_used,
                max_tokens: self.current_max_tokens,
                is_complete: true,
            });
        }

        self.last_finished_message_id = self.current_message_id.take();
        self.accumulated_content.clear();

        // Update conversation timestamp
        let _ = self.db.update_conversation_timestamp(&self.conversation_id);
    }

    /// Store generation metrics in the logs table as a JSON entry.
    pub fn log_metrics(
        &self,
        prompt_tok_per_sec: Option<f64>,
        gen_tok_per_sec: Option<f64>,
        tokens_used: i32,
        max_tokens: i32,
    ) {
        let metrics = serde_json::json!({
            "prompt_tok_per_sec": prompt_tok_per_sec,
            "gen_tok_per_sec": gen_tok_per_sec,
            "tokens_used": tokens_used,
            "max_tokens": max_tokens,
        });
        if let Err(e) = self.db.insert_log(
            Some(&self.conversation_id),
            "metrics",
            &metrics.to_string(),
        ) {
            sys_error!("Failed to log metrics: {}", e);
        }
    }

    /// Store generation timing metrics on the last finished assistant message.
    pub fn store_message_timings(
        &self,
        prompt_tok_per_sec: Option<f64>,
        gen_tok_per_sec: Option<f64>,
        gen_eval_ms: Option<f64>,
        gen_tokens: Option<i32>,
        prompt_eval_ms: Option<f64>,
        prompt_tokens: Option<i32>,
    ) {
        if let Some(ref msg_id) = self.last_finished_message_id {
            if let Err(e) = self.db.update_message_timings(
                msg_id,
                prompt_tok_per_sec,
                gen_tok_per_sec,
                gen_eval_ms,
                gen_tokens,
                prompt_eval_ms,
                prompt_tokens,
            ) {
                sys_error!("Failed to store message timings: {}", e);
            }
        }
    }

    /// Get the conversation ID.
    pub fn get_conversation_id(&self) -> String {
        self.conversation_id.clone()
    }

    /// Get full conversation content as text (backward compatibility).
    pub fn get_full_conversation(&self) -> String {
        self.db
            .get_conversation_as_text(&self.conversation_id)
            .unwrap_or_default()
    }

    /// Load conversation from database (replaces load_conversation_from_file).
    pub fn load_conversation_from_file(&self) -> std::io::Result<String> {
        self.db
            .get_conversation_as_text(&self.conversation_id)
            .map_err(std::io::Error::other)
    }
}
