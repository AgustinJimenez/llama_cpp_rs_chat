// SQLite database module for LLaMA Chat
// Replaces file-based storage with SQLite for conversations, config, and logs
//
// Integration status:
// - conversation::ConversationLogger - INTEGRATED and actively used
// - config - TODO: Integrate for persisting sampler configuration
// - migration - TODO: Integrate for one-time migration from file-based storage
// - schema - Database schema definitions
#![allow(dead_code)]

pub mod schema;
pub mod conversation;
pub mod config;
pub mod migration;

use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// Streaming update sent via broadcast channel for real-time WebSocket updates
#[derive(Clone, Debug)]
pub struct StreamingUpdate {
    pub conversation_id: String,
    pub partial_content: String,
    pub tokens_used: i32,
    pub max_tokens: i32,
    pub is_complete: bool,
}

/// Main database wrapper with connection pool and streaming broadcast
pub struct Database {
    conn: Mutex<Connection>,
    /// Broadcast channel for real-time streaming updates
    streaming_tx: broadcast::Sender<StreamingUpdate>,
}

/// Shared database type for passing across async boundaries
pub type SharedDatabase = Arc<Database>;

/// Helper function to create standardized database error messages
///
/// Usage: `.map_err(db_error("create conversation"))?`
pub fn db_error(context: &str) -> impl Fn(rusqlite::Error) -> String + '_ {
    move |e| format!("Failed to {}: {}", context, e)
}

impl Database {
    /// Create a new database connection and initialize schema
    pub fn new(db_path: &str) -> Result<Self, String> {
        let conn = Connection::open(db_path)
            .map_err(db_error("open database"))?;

        // Enable foreign keys
        conn.execute("PRAGMA foreign_keys = ON", [])
            .map_err(db_error("enable foreign keys"))?;

        // Initialize schema
        schema::initialize(&conn)?;

        // Create broadcast channel with buffer for 1000 messages
        let (streaming_tx, _) = broadcast::channel(1000);

        Ok(Self {
            conn: Mutex::new(conn),
            streaming_tx,
        })
    }

    /// Get a reference to the connection (locked)
    pub fn connection(&self) -> std::sync::MutexGuard<Connection> {
        self.conn.lock().expect("Database lock poisoned")
    }

    /// Subscribe to streaming updates for WebSocket handlers
    pub fn subscribe_streaming(&self) -> broadcast::Receiver<StreamingUpdate> {
        self.streaming_tx.subscribe()
    }

    /// Broadcast a streaming update to all WebSocket subscribers
    pub fn broadcast_streaming_update(&self, update: StreamingUpdate) {
        // Ignore send errors (no subscribers)
        let _ = self.streaming_tx.send(update);
    }
}

/// Get current timestamp in milliseconds since Unix epoch
pub fn current_timestamp_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Get current timestamp in seconds since Unix epoch
pub fn current_timestamp_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Generate a new conversation ID with timestamp format: chat_YYYY-MM-DD-HH-mm-ss-SSS
pub fn generate_conversation_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();

    let total_secs = now.as_secs();
    let millis = now.subsec_millis();

    // Calculate date/time components
    let days_since_epoch = total_secs / 86400;
    let time_of_day = total_secs % 86400;

    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Calculate year/month/day from days since epoch
    let mut year = 1970;
    let mut remaining_days = days_since_epoch as i64;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }

    let days_in_months: [i64; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1;
    for days in days_in_months.iter() {
        if remaining_days < *days {
            break;
        }
        remaining_days -= *days;
        month += 1;
    }

    let day = remaining_days + 1;

    format!(
        "chat_{:04}-{:02}-{:02}-{:02}-{:02}-{:02}-{:03}",
        year, month, day, hours, minutes, seconds, millis
    )
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_conversation_id() {
        let id = generate_conversation_id();
        assert!(id.starts_with("chat_"));
        assert_eq!(id.len(), 28); // chat_YYYY-MM-DD-HH-mm-ss-SSS
    }

    #[test]
    fn test_timestamp_functions() {
        let millis = current_timestamp_millis();
        let secs = current_timestamp_secs();
        assert!(millis > 0);
        assert!(secs > 0);
        assert_eq!(millis / 1000, secs as i64);
    }
}
