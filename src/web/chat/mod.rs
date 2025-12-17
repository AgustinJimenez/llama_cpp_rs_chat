// Chat module - split from chat_handler.rs for better organization
//
// This module contains the core chat functionality:
// - templates.rs: Chat template formatting (ChatML, Mistral, Llama3, Gemma)
// - generation.rs: Token generation with sampling and streaming
// - command_executor.rs: Command detection and execution during generation
// - stop_conditions.rs: Stop condition checking logic

mod command_executor;
mod generation;
mod stop_conditions;
mod templates;

pub use generation::generate_llama_response;
pub use templates::apply_model_chat_template;
pub use templates::get_universal_system_prompt;
