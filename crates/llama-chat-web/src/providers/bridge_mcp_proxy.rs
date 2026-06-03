//! MCP proxy for remote (OpenAI-compat) providers.
//!
//! Routes MCP tool calls through the worker IPC bridge so remote providers
//! can use the same MCP tools as local models.

use serde_json::Value;
use tokio::runtime::Handle;

use llama_chat_worker::worker::worker_bridge::SharedWorkerBridge;

/// Implements `McpManagerOps` by proxying calls to a worker via IPC.
/// Used to give remote providers access to MCP tools without running a
/// separate McpManager in the server process.
pub struct BridgeMcpProxy {
    bridge: SharedWorkerBridge,
    /// Cached tool names populated at construction time via `get_mcp_tool_names()`.
    tool_names: Vec<String>,
    /// Cached tool definitions for injecting into the API tools list.
    tool_defs: Vec<llama_chat_tools::McpToolDefInfo>,
}

impl BridgeMcpProxy {
    /// Build a proxy from a bridge. Fetches the current tool list synchronously.
    /// Must be called from a `spawn_blocking` context (or any thread with a tokio handle).
    pub fn new_blocking(bridge: SharedWorkerBridge) -> Self {
        let handle = Handle::current();
        let tool_names = handle.block_on(bridge.get_mcp_tool_names());
        // Full definitions not yet available via IPC; tools are still callable via call_tool.
        let tool_defs = Vec::new();
        eprintln!("[BRIDGE_MCP_PROXY] Built proxy: {} MCP tools available", tool_names.len());
        BridgeMcpProxy { bridge, tool_names, tool_defs }
    }
}

impl llama_chat_tools::McpManagerOps for BridgeMcpProxy {
    fn is_mcp_tool(&self, name: &str) -> bool {
        self.tool_names.iter().any(|n| n == name)
    }

    fn call_tool(&self, qualified_name: &str, args: Value) -> Result<String, String> {
        let handle = Handle::current();
        handle.block_on(self.bridge.call_mcp_tool(qualified_name, args))
    }

    fn get_server_statuses(&self) -> Vec<llama_chat_types::McpServerStatus> {
        Vec::new()
    }

    fn get_tool_definitions(&self) -> Vec<llama_chat_tools::McpToolDefInfo> {
        self.tool_defs.clone()
    }

    fn get_connected_server_names(&self) -> Vec<String> {
        Vec::new()
    }

    fn refresh_connections(&self, _db: &llama_chat_db::SharedDatabase) -> Result<(), String> {
        Ok(())
    }
}
