//! MCP tool definition types and conversion to OpenAI function format.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A tool definition discovered from an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDef {
    /// Original tool name from the MCP server (e.g. "read_file")
    pub name: String,
    /// Namespaced name: mcp__<server_name>__<tool_name>
    pub qualified_name: String,
    /// Human-readable description
    pub description: String,
    /// JSON Schema for input parameters
    pub input_schema: Value,
    /// The server ID this tool belongs to
    pub server_id: String,
    /// The server display name
    pub server_name: String,
}

impl McpToolDef {
    /// Create a qualified (namespaced) tool name to avoid collisions with native tools.
    pub fn make_qualified_name(server_name: &str, tool_name: &str) -> String {
        // Sanitize server name: lowercase, replace non-alphanumeric with underscore
        let safe_server: String = server_name
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '_' { c.to_ascii_lowercase() } else { '_' })
            .collect();
        format!("mcp__{safe_server}__{tool_name}")
    }

    /// Convert to OpenAI function-calling format for injection into system prompts.
    pub fn to_openai_function(&self) -> Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.qualified_name,
                "description": format!("[MCP:{}] {}", self.server_name, self.description),
                "parameters": self.input_schema,
            }
        })
    }

    /// Convert to the simpler tool format used by `get_available_tools()`.
    pub fn to_tool_def(&self) -> Value {
        serde_json::json!({
            "name": self.qualified_name,
            "description": format!("[MCP:{}] {}", self.server_name, self.description),
            "parameters": self.input_schema,
        })
    }
}
