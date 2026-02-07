// This module has been split into chat/templates.rs and chat/generation.rs
// Re-export for backward compatibility

pub use super::chat::{
    get_universal_system_prompt_with_tags, get_tool_tags_for_model,
};
