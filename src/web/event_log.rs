//! Async event log for conversation debugging.
//! Captures ALL events (generation, tool calls, errors, etc.) per conversation.
//! Uses a lock-free channel for zero-cost logging on the hot path.
//! A background thread drains the channel and batch-writes to the DB.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::OnceLock;

const MAX_EVENTS_PER_CONVERSATION: usize = 1000;
/// Batch DB writes: flush after this many queued events or on timeout.
const DB_BATCH_SIZE: usize = 20;
const DB_FLUSH_INTERVAL_MS: u64 = 500;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ConversationEvent {
    pub timestamp: i64,
    pub event_type: String,
    pub message: String,
}

// In-memory store for live UI queries
static EVENT_STORE: OnceLock<Mutex<HashMap<String, Vec<ConversationEvent>>>> = OnceLock::new();

fn store() -> &'static Mutex<HashMap<String, Vec<ConversationEvent>>> {
    EVENT_STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

// Async channel for DB persistence (fire-and-forget from callers)
static LOG_SENDER: OnceLock<std::sync::mpsc::Sender<(String, ConversationEvent)>> = OnceLock::new();

/// Initialize event logging with a DB reference for persistence.
/// Spawns a background thread that batch-writes events to the DB.
pub fn init_event_log(db: super::database::SharedDatabase) {
    let (tx, rx) = std::sync::mpsc::channel::<(String, ConversationEvent)>();
    LOG_SENDER.get_or_init(|| tx);

    // Background writer thread — drains channel and batch-inserts to DB
    std::thread::Builder::new()
        .name("event-log-writer".into())
        .spawn(move || {
            let mut batch: Vec<(String, ConversationEvent)> = Vec::with_capacity(DB_BATCH_SIZE);
            loop {
                // Block on first event, then drain remaining
                match rx.recv_timeout(std::time::Duration::from_millis(DB_FLUSH_INTERVAL_MS)) {
                    Ok(event) => batch.push(event),
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
                // Drain any additional queued events
                while let Ok(event) = rx.try_recv() {
                    batch.push(event);
                    if batch.len() >= DB_BATCH_SIZE { break; }
                }
                // Flush batch to DB
                if !batch.is_empty() {
                    flush_to_db(&db, &batch);
                    batch.clear();
                }
            }
            // Final flush on shutdown
            if !batch.is_empty() {
                flush_to_db(&db, &batch);
            }
        })
        .ok();
}

fn flush_to_db(db: &super::database::SharedDatabase, batch: &[(String, ConversationEvent)]) {
    let conn = db.connection();
    // Use a transaction for batch efficiency
    if conn.execute_batch("BEGIN").is_err() { return; }
    for (conv_id, event) in batch {
        let level = format!("event:{}", event.event_type);
        let _ = conn.execute(
            "INSERT INTO logs (conversation_id, level, message, timestamp) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![conv_id, level, event.message, event.timestamp / 1000],
        );
    }
    let _ = conn.execute_batch("COMMIT");
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Log an event. This is lock-free on the DB path (channel send).
/// Only takes a brief mutex for the in-memory store.
pub fn log_event(conversation_id: &str, event_type: &str, message: &str) {
    let conv_id = conversation_id.trim_end_matches(".txt").to_string();
    let event = ConversationEvent {
        timestamp: now_ms(),
        event_type: event_type.to_string(),
        message: message.to_string(),
    };

    // In-memory store (fast, for live UI)
    if let Ok(mut map) = store().lock() {
        let events = map.entry(conv_id.clone()).or_default();
        events.push(event.clone());
        if events.len() > MAX_EVENTS_PER_CONVERSATION {
            events.drain(..events.len() - MAX_EVENTS_PER_CONVERSATION);
        }
    }

    // Async DB write via channel (non-blocking)
    if let Some(tx) = LOG_SENDER.get() {
        let _ = tx.send((conv_id, event));
    }
}

// Global status message (for compaction progress visible via API polling)
static GLOBAL_STATUS: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn global_status() -> &'static Mutex<Option<String>> {
    GLOBAL_STATUS.get_or_init(|| Mutex::new(None))
}

pub fn set_global_status(message: &str) {
    if let Ok(mut s) = global_status().lock() {
        *s = if message.is_empty() { None } else { Some(message.to_string()) };
    }
}

pub fn clear_global_status() {
    if let Ok(mut s) = global_status().lock() {
        *s = None;
    }
}

pub fn get_global_status() -> Option<String> {
    global_status().lock().ok().and_then(|s| s.clone())
}

pub fn get_events(conversation_id: &str) -> Vec<ConversationEvent> {
    let conv_id = conversation_id.trim_end_matches(".txt");

    // Try in-memory first (current session)
    let mem_events = store()
        .lock()
        .ok()
        .and_then(|map| map.get(conv_id).cloned())
        .unwrap_or_default();

    if !mem_events.is_empty() {
        return mem_events;
    }

    // Fall back to DB (previous sessions)
    let events = load_events_from_db(conv_id);
    if !events.is_empty() {
        // Cache in memory for subsequent calls
        if let Ok(mut map) = store().lock() {
            map.insert(conv_id.to_string(), events.clone());
        }
        return events;
    }

    Vec::new()
}

fn load_events_from_db(conv_id: &str) -> Vec<ConversationEvent> {
    // We need the DB ref — get it from LOG_SENDER's paired DB
    // Actually we store events in the same DB, so use a direct connection approach
    // For now, try to read from the DB file directly
    let db_path = "assets/llama_chat.db";
    let conn = match rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let mut stmt = match conn.prepare(
        "SELECT level, message, timestamp FROM logs WHERE conversation_id = ?1 AND level LIKE 'event:%' ORDER BY timestamp ASC LIMIT 1000"
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    stmt.query_map(
        rusqlite::params![conv_id],
        |row| {
            let level: String = row.get(0)?;
            let event_type = level.strip_prefix("event:").unwrap_or(&level).to_string();
            Ok(ConversationEvent {
                event_type,
                message: row.get(1)?,
                timestamp: row.get::<_, i64>(2)? * 1000,
            })
        }
    ).ok()
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}
