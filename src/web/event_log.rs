//! Event log for conversation debugging.
//! Captures key events (stalls, compaction, tool calls, Y/N checks) per conversation.
//! Events are stored in-memory (fast) AND persisted to the `logs` DB table (survives restarts).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::OnceLock;

const MAX_EVENTS_PER_CONVERSATION: usize = 500;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ConversationEvent {
    pub timestamp: i64,
    pub event_type: String,
    pub message: String,
}

static EVENT_STORE: OnceLock<Mutex<HashMap<String, Vec<ConversationEvent>>>> = OnceLock::new();
static EVENT_DB_REF: OnceLock<Mutex<Option<super::database::SharedDatabase>>> = OnceLock::new();

fn store() -> &'static Mutex<HashMap<String, Vec<ConversationEvent>>> {
    EVENT_STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn db_ref() -> &'static Mutex<Option<super::database::SharedDatabase>> {
    EVENT_DB_REF.get_or_init(|| Mutex::new(None))
}

/// Initialize event logging with a DB reference for persistence.
/// Call once at worker startup.
pub fn init_event_log(db: super::database::SharedDatabase) {
    if let Ok(mut r) = db_ref().lock() {
        *r = Some(db);
    }
}

pub fn log_event(conversation_id: &str, event_type: &str, message: &str) {
    let conv_id = conversation_id.trim_end_matches(".txt").to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let event = ConversationEvent {
        timestamp: now,
        event_type: event_type.to_string(),
        message: message.to_string(),
    };

    // In-memory store (fast, for live UI)
    if let Ok(mut map) = store().lock() {
        let events = map.entry(conv_id.clone()).or_default();
        events.push(event);
        if events.len() > MAX_EVENTS_PER_CONVERSATION {
            events.drain(..events.len() - MAX_EVENTS_PER_CONVERSATION);
        }
    }

    // Persist to DB logs table (survives restarts)
    if let Ok(db_guard) = db_ref().lock() {
        if let Some(ref db) = *db_guard {
            let level = format!("event:{event_type}");
            let conn = db.connection();
            let _ = conn.execute(
                "INSERT INTO logs (conversation_id, level, message, timestamp) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![conv_id, level, message, now / 1000], // timestamp in seconds
            );
        }
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
    if let Some(db) = db_ref().lock().ok().and_then(|g| g.clone()) {
        let events = load_events_from_db(&db, conv_id);
        if !events.is_empty() {
            return events;
        }
    }

    Vec::new()
}

fn load_events_from_db(db: &super::database::SharedDatabase, conv_id: &str) -> Vec<ConversationEvent> {
    let conn = db.connection();
    let mut stmt = match conn.prepare(
        "SELECT level, message, timestamp FROM logs WHERE conversation_id = ?1 AND level LIKE 'event:%' ORDER BY timestamp ASC LIMIT 500"
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
