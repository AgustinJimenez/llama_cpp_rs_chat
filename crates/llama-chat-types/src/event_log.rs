//! Conversation event type definition.
//!
//! The event log infrastructure (init_event_log, log_event, etc.) remains
//! in the main crate at src/web/event_log.rs.

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ConversationEvent {
    pub timestamp: i64,
    pub event_type: String,
    pub message: String,
}
