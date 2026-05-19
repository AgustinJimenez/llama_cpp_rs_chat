//! WebSocket handlers for chat streaming, conversation watching, and status.

use std::sync::atomic::AtomicU32;

// Global counter for active WebSocket connections.
pub static ACTIVE_WS_CONNECTIONS: AtomicU32 = AtomicU32::new(0);

mod chat;
mod watch;
mod status;
mod title;

pub use chat::handle_websocket;
pub use watch::handle_conversation_watch;
pub use status::handle_status_ws;
pub use title::{strip_tool_tags, sanitize_title};
