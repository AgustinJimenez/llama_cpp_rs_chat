//! MCP (Model Context Protocol) client integration.
//!
//! Manages connections to external MCP tool servers, discovers their tools,
//! and routes tool calls from the LLM to the appropriate MCP server.

pub mod client;
pub mod config;
pub mod manager;
pub mod tool_registry;

// Re-exports — used in phases 2-5 when wired into worker + routes.
#[allow(unused_imports)]
pub use manager::{McpManager, SharedMcpManager};
#[allow(unused_imports)]
pub use config::McpServerConfig;
#[allow(unused_imports)]
pub use tool_registry::McpToolDef;
