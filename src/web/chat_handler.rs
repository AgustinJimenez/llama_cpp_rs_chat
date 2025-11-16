// This module has been split into chat/templates.rs and chat/generation.rs
// Re-export for backward compatibility

pub use super::chat::{apply_model_chat_template, generate_llama_response};
