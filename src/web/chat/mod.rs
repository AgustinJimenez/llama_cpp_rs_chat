// Chat module - split from chat_handler.rs for better organization
//
// This module contains the core chat functionality:
// - templates.rs: Chat template formatting (ChatML, Mistral, Llama3, Gemma)
// - generation.rs: Token generation with sampling and streaming
// - command_executor.rs: Command detection and execution during generation
// - stop_conditions.rs: Stop condition checking logic

mod command_executor;
mod compaction;
mod context_eval;
mod tool_dispatch;
mod tool_output;
mod generation;
pub mod sub_checks;
pub mod jinja_templates;
pub mod loop_detection;
mod prompt_builder;
mod sampler;
pub mod tool_defs;
mod stop_conditions;
pub mod sub_agent;
mod token_loop;
mod tool_grammar;
pub mod tool_parser;
mod templates;
pub mod tool_tags;

pub use generation::generate_llama_response;
pub use sub_checks::generate_title_text;
pub use generation::warmup_system_prompt;
pub use templates::get_universal_system_prompt_with_tags;
pub use tool_tags::get_tool_tags_for_model;
