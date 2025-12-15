// Chat module - split from chat_handler.rs for better organization
//
// This module contains the core chat functionality:
// - templates.rs: Chat template formatting (ChatML, Mistral, Llama3, Gemma)
// - generation.rs: Token generation with sampling and streaming

mod templates;
mod generation;

pub use templates::apply_model_chat_template;
pub use templates::get_universal_system_prompt;
pub use generation::generate_llama_response;
