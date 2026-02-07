// Chat module - split from chat_handler.rs for better organization
//
// This module contains the core chat functionality:
// - templates.rs: Chat template formatting (ChatML, Mistral, Llama3, Gemma)
// - generation.rs: Token generation with sampling and streaming
// - command_executor.rs: Command detection and execution during generation
// - stop_conditions.rs: Stop condition checking logic

mod command_executor;
mod generation;
mod jinja_templates;
mod stop_conditions;
mod templates;
pub mod tool_tags;

pub use generation::generate_llama_response;
pub use jinja_templates::{apply_native_chat_template, parse_conversation_to_messages, get_available_tools};
pub use templates::{apply_model_chat_template, apply_system_prompt_by_type, apply_system_prompt_by_type_with_tags, get_universal_system_prompt, get_universal_system_prompt_with_tags};
pub use tool_tags::{get_tool_tags_for_model, ToolTags};
