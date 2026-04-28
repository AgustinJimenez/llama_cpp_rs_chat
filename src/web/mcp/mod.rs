// Re-export everything from the llama-chat-worker crate's MCP module

#[allow(unused_imports)]
pub mod client {
    pub use llama_chat_worker::mcp::client::*;
}

#[allow(unused_imports)]
pub mod config {
    pub use llama_chat_worker::mcp::config::*;
}

#[allow(unused_imports)]
pub mod manager {
    pub use llama_chat_worker::mcp::manager::*;
}

#[allow(unused_imports)]
pub mod tool_registry {
    pub use llama_chat_worker::mcp::tool_registry::*;
}

// Top-level re-exports
#[allow(unused_imports)]
pub use llama_chat_worker::{McpManager, SharedMcpManager};
#[allow(unused_imports)]
pub use llama_chat_worker::McpServerConfig;
#[allow(unused_imports)]
pub use llama_chat_worker::McpToolDef;
