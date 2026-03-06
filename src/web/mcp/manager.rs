//! MCP Manager: orchestrates multiple MCP client connections.
//!
//! Owns a persistent tokio runtime for async MCP operations.
//! Called from the worker's synchronous generation thread via `rt.block_on()`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::client::McpClient;
use super::tool_registry::McpToolDef;
use crate::web::database::SharedDatabase;
use crate::web::worker::ipc_types::McpServerStatus;

/// Shared MCP manager type for passing across threads.
pub type SharedMcpManager = Arc<McpManager>;

/// Manages multiple MCP server connections and their tool registries.
pub struct McpManager {
    /// Persistent tokio runtime for all MCP async operations.
    rt: tokio::runtime::Runtime,
    /// Connected MCP clients, keyed by server_id.
    clients: Mutex<HashMap<String, McpClient>>,
    /// Maps qualified_name → (server_id, original_tool_name) for dispatch routing.
    tool_routing: Mutex<HashMap<String, (String, String)>>,
    /// All MCP tool definitions (for injection into system prompts).
    mcp_tools: Mutex<Vec<McpToolDef>>,
}

impl McpManager {
    /// Create a new MCP manager. Does not connect to any servers yet.
    pub fn new() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("mcp-runtime")
            .build()
            .expect("Failed to create MCP tokio runtime");

        McpManager {
            rt,
            clients: Mutex::new(HashMap::new()),
            tool_routing: Mutex::new(HashMap::new()),
            mcp_tools: Mutex::new(Vec::new()),
        }
    }

    /// Load MCP server configs from DB, connect to enabled servers, discover tools.
    /// Call this on startup and when settings change.
    pub fn refresh_connections(&self, db: &SharedDatabase) -> Result<(), String> {
        self.rt.block_on(async {
            self.refresh_connections_async(db).await
        })
    }

    async fn refresh_connections_async(&self, db: &SharedDatabase) -> Result<(), String> {
        // Load configs from database
        let configs = crate::web::database::mcp::load_mcp_servers(db);

        let enabled_ids: Vec<String> = configs.iter()
            .filter(|c| c.enabled)
            .map(|c| c.id.clone())
            .collect();

        // Disconnect servers that are no longer enabled or configured
        {
            let mut clients = self.clients.lock().unwrap_or_else(|p| p.into_inner());
            let to_remove: Vec<String> = clients.keys()
                .filter(|id| !enabled_ids.contains(id))
                .cloned()
                .collect();
            for id in to_remove {
                if let Some(client) = clients.remove(&id) {
                    eprintln!("[MCP] Disconnecting removed/disabled server '{}'", client.server_name);
                    client.disconnect().await;
                }
            }
        }

        // Connect to newly enabled servers
        let mut errors = Vec::new();
        for config in configs.iter().filter(|c| c.enabled) {
            let already_connected = {
                let clients = self.clients.lock().unwrap_or_else(|p| p.into_inner());
                clients.contains_key(&config.id)
            };

            if already_connected {
                continue;
            }

            eprintln!("[MCP] Connecting to server '{}' (id={})...", config.name, config.id);
            match McpClient::connect(config).await {
                Ok(client) => {
                    eprintln!("[MCP] Connected to '{}': {} tools discovered", config.name, client.tools.len());
                    let mut clients = self.clients.lock().unwrap_or_else(|p| p.into_inner());
                    clients.insert(config.id.clone(), client);
                }
                Err(e) => {
                    eprintln!("[MCP] Failed to connect to '{}': {}", config.name, e);
                    errors.push(format!("{}: {}", config.name, e));
                }
            }
        }

        // Rebuild tool registry from all connected clients
        self.rebuild_tool_registry();

        if errors.is_empty() {
            Ok(())
        } else {
            // Partial success: some servers connected, some failed
            Ok(()) // Don't fail the whole refresh for partial errors
        }
    }

    /// Rebuild the tool routing table and merged tool definitions.
    fn rebuild_tool_registry(&self) {
        let clients = self.clients.lock().unwrap_or_else(|p| p.into_inner());

        let mut routing = HashMap::new();
        let mut all_tools = Vec::new();

        for client in clients.values() {
            for tool in &client.tools {
                routing.insert(
                    tool.qualified_name.clone(),
                    (client.server_id.clone(), tool.name.clone()),
                );
                all_tools.push(tool.clone());
            }
        }

        let tool_count = all_tools.len();

        *self.tool_routing.lock().unwrap_or_else(|p| p.into_inner()) = routing;
        *self.mcp_tools.lock().unwrap_or_else(|p| p.into_inner()) = all_tools;

        eprintln!("[MCP] Tool registry rebuilt: {} tools from {} servers", tool_count, clients.len());
    }

    /// Check if a tool name is an MCP tool (qualified name lookup).
    pub fn is_mcp_tool(&self, name: &str) -> bool {
        let routing = self.tool_routing.lock().unwrap_or_else(|p| p.into_inner());
        routing.contains_key(name)
    }

    /// Call an MCP tool by its qualified name. Blocks the current thread.
    pub fn call_tool(&self, qualified_name: &str, args: Value) -> Result<String, String> {
        // Look up routing info
        let (server_id, original_name) = {
            let routing = self.tool_routing.lock().unwrap_or_else(|p| p.into_inner());
            routing.get(qualified_name)
                .cloned()
                .ok_or_else(|| format!("Unknown MCP tool: {qualified_name}"))?
        };

        // Call the tool on the appropriate client
        self.rt.block_on(async {
            let clients = self.clients.lock().unwrap_or_else(|p| p.into_inner());
            let client = clients.get(&server_id)
                .ok_or_else(|| format!("MCP server '{}' not connected", server_id))?;
            client.call_tool(&original_name, args).await
        })
    }

    /// Get all MCP tool definitions for injection into system prompts.
    pub fn get_tool_definitions(&self) -> Vec<McpToolDef> {
        self.mcp_tools.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Get names of all connected servers.
    pub fn get_connected_server_names(&self) -> Vec<String> {
        let clients = self.clients.lock().unwrap_or_else(|p| p.into_inner());
        clients.values().map(|c| c.server_name.clone()).collect()
    }

    /// Get detailed status of each connected MCP server (for IPC responses).
    pub fn get_server_statuses(&self) -> Vec<McpServerStatus> {
        let clients = self.clients.lock().unwrap_or_else(|p| p.into_inner());
        clients.values()
            .map(|c| McpServerStatus {
                id: c.server_id.clone(),
                name: c.server_name.clone(),
                connected: true,
                tool_count: c.tools.len(),
                tools: c.tools.iter().map(|t| t.qualified_name.clone()).collect(),
            })
            .collect()
    }

    /// Shut down all MCP connections.
    #[allow(dead_code)]
    pub fn shutdown(&self) {
        self.rt.block_on(async {
            let mut clients = self.clients.lock().unwrap_or_else(|p| p.into_inner());
            for (_, client) in clients.drain() {
                client.disconnect().await;
            }
        });
        *self.tool_routing.lock().unwrap_or_else(|p| p.into_inner()) = HashMap::new();
        *self.mcp_tools.lock().unwrap_or_else(|p| p.into_inner()) = Vec::new();
        eprintln!("[MCP] All connections shut down");
    }
}
