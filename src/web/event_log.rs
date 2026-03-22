//! In-memory event log for conversation debugging.
//! Captures key events (stalls, compaction, tool calls, Y/N checks) per conversation.

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

fn store() -> &'static Mutex<HashMap<String, Vec<ConversationEvent>>> {
    EVENT_STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn log_event(conversation_id: &str, event_type: &str, message: &str) {
    let conv_id = conversation_id.trim_end_matches(".txt").to_string();
    let event = ConversationEvent {
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
        event_type: event_type.to_string(),
        message: message.to_string(),
    };
    if let Ok(mut map) = store().lock() {
        let events = map.entry(conv_id).or_default();
        events.push(event);
        if events.len() > MAX_EVENTS_PER_CONVERSATION {
            events.drain(..events.len() - MAX_EVENTS_PER_CONVERSATION);
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
    store()
        .lock()
        .ok()
        .and_then(|map| map.get(conv_id).cloned())
        .unwrap_or_default()
}
