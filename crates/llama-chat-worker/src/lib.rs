//! Out-of-process model worker and MCP client integration.
//!
//! The model runs in a separate child process for:
//! - Memory reclaim: kill the process to free all VRAM/RAM
//! - Crash isolation: model crash doesn't kill the web server

// Logging macros (same as root crate)
#[macro_export]
macro_rules! log_info {
    ($target:expr, $($arg:tt)*) => { eprintln!($($arg)*) };
}
#[macro_export]
macro_rules! log_debug {
    ($target:expr, $($arg:tt)*) => { if cfg!(debug_assertions) { eprintln!($($arg)*) } };
}
#[macro_export]
macro_rules! log_warn {
    ($target:expr, $($arg:tt)*) => { eprintln!($($arg)*) };
}

pub mod prevent_sleep;
pub mod worker;
pub mod mcp;

// Re-exports
pub use worker::worker_bridge::{GenerationResult, ModelMeta, SharedWorkerBridge, WorkerBridge};
pub use worker::process_manager::ProcessManager;
pub use worker::worker_main::run_worker;
pub use mcp::manager::{McpManager, SharedMcpManager};
pub use mcp::tool_registry::McpToolDef;
pub use mcp::config::{McpServerConfig, McpTransport};

// Re-export IPC types from the types crate
pub use llama_chat_types::ipc_types;
