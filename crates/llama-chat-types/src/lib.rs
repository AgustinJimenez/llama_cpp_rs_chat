pub mod logger;
pub mod models;
pub mod tool_tags;
pub mod ipc_types;
pub mod event_log;
pub mod mcp_config;
pub mod native_tool_result;

// Re-export key types at crate root for convenience
pub use models::*;
pub use tool_tags::{TagPair, ToolTags};
pub use ipc_types::*;
pub use event_log::ConversationEvent;
pub use logger::{Logger, LOGGER};
pub use mcp_config::{McpServerConfig, McpTransport};
pub use native_tool_result::NativeToolResult;
