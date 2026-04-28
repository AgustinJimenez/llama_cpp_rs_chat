// Re-export all config functions from the extracted crate
#[allow(unused_imports)]
pub use llama_chat_config::*;

// Re-export get_resolved_system_prompt from the engine crate
#[allow(unused_imports)]
pub use llama_chat_engine::config_ext::get_resolved_system_prompt;
