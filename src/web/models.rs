// Re-export all types from the shared types crate
pub use llama_chat_types::models::*;

use std::sync::{Arc, Mutex};

use llama_chat_db::conversation::ConversationLogger;

pub type SharedConversationLogger = Arc<Mutex<ConversationLogger>>;
