//! MCP server configuration types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for a single MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub id: String,
    pub name: String,
    pub transport: McpTransport,
    pub enabled: bool,
}

/// Transport configuration for connecting to an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum McpTransport {
    Stdio {
        command: String,
        args: Vec<String>,
        #[serde(default)]
        env_vars: HashMap<String, String>,
    },
    Http {
        url: String,
    },
}
