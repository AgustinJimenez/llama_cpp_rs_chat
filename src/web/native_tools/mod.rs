//! Native file I/O and code execution tools.
//!
//! This module re-exports from the `llama-chat-tools` workspace crate.

// Re-export everything from the tools crate's public API
#[allow(unused_imports)]
pub use llama_chat_tools::*;

/// Build a `DispatchContext` that connects the tools crate to the root crate's
/// skill system and tool catalog (in `jinja_templates`).
#[allow(dead_code)]
pub fn make_dispatch_context() -> llama_chat_tools::DispatchContext<'static> {
    llama_chat_web::native_tools_bridge::make_dispatch_context()
}
