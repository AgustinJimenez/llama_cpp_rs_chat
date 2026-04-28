//! Chat engine crate — core LLM generation loop, tool dispatch, compaction,
//! template rendering, and sub-agents.

// Re-export macros from llama-chat-types so all modules can use log_info! etc.
#[allow(unused_imports)]
#[macro_use]
extern crate llama_chat_types;

pub mod browser;
mod command_executor;
pub mod config_ext;
mod compaction;
mod context_eval;
pub mod filename_patterns;
mod generation;
pub mod gguf_info;
pub mod gguf_utils;
pub mod jinja_templates;
pub mod loop_detection;
pub mod model_manager;
mod prompt_builder;
mod sampler;
mod stop_conditions;
pub mod sub_agent;
pub mod sub_checks;
pub mod templates;
mod token_loop;
mod tool_dispatch;
mod tool_grammar;
mod tool_output;
pub mod tool_tags;
pub mod utils;
pub mod vram_calculator;

// Re-export tool_defs and tool_parser from the tools crate
pub mod tool_defs {
    pub use llama_chat_tools::tool_defs::*;
}

pub mod tool_parser {
    pub use llama_chat_tools::tool_parser::*;
}

// Type alias for shared conversation logger (used by generation and token_loop)
pub type SharedConversationLogger = std::sync::Arc<std::sync::Mutex<llama_chat_db::conversation::ConversationLogger>>;

// Public API
pub use generation::generate_llama_response;
pub use generation::GenerationOutput;
pub use sub_checks::generate_title_text;
pub use prompt_builder::warmup_system_prompt;
pub use templates::get_universal_system_prompt_with_tags;
pub use tool_tags::get_tool_tags_for_model;
pub use model_manager::{get_model_status, load_model, ModelParams};
pub use gguf_info::extract_model_info;
pub use vram_calculator::calculate_optimal_gpu_layers;

/// Build a `DispatchContext` that connects the tools crate to the engine crate's
/// tool catalog and schema functions. Skills are not available at the engine level
/// (the root crate can override this with a fuller context if needed).
pub fn make_dispatch_context() -> llama_chat_tools::DispatchContext<'static> {
    llama_chat_tools::DispatchContext {
        get_tool_catalog: Some(&|category| {
            jinja_templates::get_tool_catalog(category)
        }),
        get_tool_schema: Some(&|tool_name| {
            jinja_templates::get_tool_schema(tool_name)
        }),
        discover_skills: None,
        get_skill: None,
    }
}
