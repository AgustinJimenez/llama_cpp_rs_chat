// Chat module — re-exports from the `llama-chat-engine` workspace crate.
//
// All chat engine functionality (generation, tool dispatch, templates,
// compaction, sub-agents) is now in the engine crate.

// Re-export public modules
#[allow(unused_imports)]
pub mod jinja_templates {
    pub use llama_chat_engine::jinja_templates::*;
}
#[allow(unused_imports)]
pub mod loop_detection {
    pub use llama_chat_engine::loop_detection::*;
}
#[allow(unused_imports)]
pub mod sub_checks {
    pub use llama_chat_engine::sub_checks::*;
}
#[allow(unused_imports)]
pub mod sub_agent {
    pub use llama_chat_engine::sub_agent::*;
}
#[allow(unused_imports)]
pub mod tool_defs {
    pub use llama_chat_engine::tool_defs::*;
}
#[allow(unused_imports)]
pub mod tool_parser {
    pub use llama_chat_engine::tool_parser::*;
}
#[allow(unused_imports)]
pub mod tool_tags {
    pub use llama_chat_engine::tool_tags::*;
}
#[allow(unused_imports)]
pub mod templates {
    pub use llama_chat_engine::templates::*;
}

// Top-level re-exports (used by other modules)
#[allow(unused_imports)]
pub use llama_chat_engine::generate_llama_response;
#[allow(unused_imports)]
pub use llama_chat_engine::generate_title_text;
#[allow(unused_imports)]
pub use llama_chat_engine::warmup_system_prompt;
#[allow(unused_imports)]
pub use llama_chat_engine::get_universal_system_prompt_with_tags;
#[allow(unused_imports)]
pub use llama_chat_engine::get_tool_tags_for_model;
#[allow(unused_imports)]
pub use llama_chat_engine::GenerationOutput;
