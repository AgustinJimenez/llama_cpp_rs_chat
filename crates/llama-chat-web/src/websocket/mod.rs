//! WebSocket handlers for chat streaming, conversation watching, and status.

use std::sync::atomic::AtomicU32;

// Global counter for active WebSocket connections.
pub static ACTIVE_WS_CONNECTIONS: AtomicU32 = AtomicU32::new(0);

pub const MAX_SERVER_AUTO_CONTINUES: u32 = 3;
const SERVER_CONTINUE_PREVIEW_LEN: usize = 200;

/// Finish reasons that the server handles internally via re-generation.
pub fn should_server_auto_continue(finish_reason: &str) -> bool {
    matches!(
        finish_reason,
        "length" | "cuda_deadlock" | "loop_recovery" | "infinite_loop"
    )
}

/// Build the continuation prompt the server sends for each auto-continue reason.
pub fn make_server_continuation_message(finish_reason: &str, original_message: &str) -> String {
    if matches!(finish_reason, "loop_recovery" | "infinite_loop") {
        "[SYSTEM] Infinite loop detected — you have been repeating similar actions without \
         progress. STOP your current approach entirely. Step back, analyze what went wrong, \
         explain it to the user, and either try a COMPLETELY DIFFERENT strategy or ask the \
         user for guidance. Do NOT repeat any of the previous commands."
            .to_string()
    } else {
        let preview: String = original_message
            .chars()
            .take(SERVER_CONTINUE_PREVIEW_LEN)
            .collect();
        let ellipsis = if original_message.len() > SERVER_CONTINUE_PREVIEW_LEN {
            "..."
        } else {
            ""
        };
        format!(
            "Continue working on this task: \"{preview}{ellipsis}\". Pick up where you left off."
        )
    }
}

mod chat;
mod watch;
mod status;
mod title;

pub use chat::handle_websocket;
pub use watch::handle_conversation_watch;
pub use status::handle_status_ws;
pub use title::{sanitize_title, spawn_title_generation, strip_tool_tags};
