//! MCP client wrapper around rmcp session.
//!
//! Each `McpClient` represents a connection to a single MCP server.
//! It handles connecting, discovering tools, and calling them.

use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, Tool},
    service::{RunningService, RoleClient},
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use serde_json::Value;
use tokio::process::Command;

use super::config::{McpServerConfig, McpTransport};
use super::tool_registry::McpToolDef;
use crate::{log_info, log_warn, log_debug};

/// A connected MCP client session.
pub struct McpClient {
    pub server_id: String,
    pub server_name: String,
    /// Active rmcp session
    session: RunningService<RoleClient, ()>,
    /// Cached tool definitions from this server
    pub tools: Vec<McpToolDef>,
}

impl McpClient {
    /// Connect to an MCP server and perform the initialization handshake.
    pub async fn connect(config: &McpServerConfig) -> Result<Self, String> {
        log_info!("system", "MCP: connecting to server '{}' ({})", config.name, config.id);

        let session = match &config.transport {
            McpTransport::Stdio { command, args, env_vars } => {
                let cmd_str = command.clone();
                let args_clone = args.clone();
                let env_clone = env_vars.clone();

                let transport = TokioChildProcess::new(
                    Command::new(&cmd_str).configure(|cmd| {
                        for arg in &args_clone {
                            cmd.arg(arg);
                        }
                        for (key, val) in &env_clone {
                            cmd.env(key, val);
                        }
                        // Ensure child process doesn't inherit our stdin
                        cmd.stdin(std::process::Stdio::piped());
                    })
                ).map_err(|e| format!("MCP transport error for '{}': {e}", config.name))?;

                ().serve(transport)
                    .await
                    .map_err(|e| format!("MCP handshake failed for '{}': {e}", config.name))?
            }
            McpTransport::Http { url: _ } => {
                return Err("HTTP/SSE transport not yet implemented".to_string());
            }
        };

        if let Some(info) = session.peer_info() {
            log_info!(
                "system",
                "MCP: connected to '{}' — server: {:?} v{:?}",
                config.name,
                info.server_info.name,
                info.server_info.version
            );
        }

        let mut client = McpClient {
            server_id: config.id.clone(),
            server_name: config.name.clone(),
            session,
            tools: Vec::new(),
        };

        // Discover tools immediately after connection
        client.discover_tools().await?;

        Ok(client)
    }

    /// Discover all tools exposed by this MCP server.
    pub async fn discover_tools(&mut self) -> Result<(), String> {
        let raw_tools: Vec<Tool> = self.session
            .list_all_tools()
            .await
            .map_err(|e| format!("MCP tools/list failed for '{}': {e}", self.server_name))?;

        self.tools = raw_tools
            .into_iter()
            .map(|t| {
                let name = t.name.to_string();
                let qualified = McpToolDef::make_qualified_name(&self.server_name, &name);
                let description = t.description.as_deref().unwrap_or("").to_string();
                let input_schema = serde_json::to_value(&*t.input_schema).unwrap_or_default();

                McpToolDef {
                    name,
                    qualified_name: qualified,
                    description,
                    input_schema,
                    server_id: self.server_id.clone(),
                    server_name: self.server_name.clone(),
                }
            })
            .collect();

        log_info!(
            "system",
            "MCP: discovered {} tools from '{}'",
            self.tools.len(),
            self.server_name
        );
        for tool in &self.tools {
            log_debug!("system", "  MCP tool: {} → {}", tool.name, tool.qualified_name);
        }

        Ok(())
    }

    /// Call a tool on this MCP server by its original (non-qualified) name.
    pub async fn call_tool(&self, tool_name: &str, args: Value) -> Result<String, String> {
        log_info!("system", "MCP: calling tool '{}' on server '{}'", tool_name, self.server_name);

        let arguments = if let Value::Object(map) = args {
            map
        } else if args.is_null() {
            serde_json::Map::new()
        } else {
            return Err(format!("MCP tool arguments must be a JSON object, got: {args}"));
        };

        let params = CallToolRequestParams::new(tool_name.to_string())
            .with_arguments(arguments);

        let result = self.session
            .call_tool(params)
            .await
            .map_err(|e| format!("MCP tools/call failed for '{}' on '{}': {e}", tool_name, self.server_name))?;

        // Extract text content from the result
        let mut output_parts: Vec<String> = Vec::new();
        for content in &result.content {
            if let Some(text) = content.as_text() {
                output_parts.push(text.text.clone());
            }
        }

        let output = if output_parts.is_empty() {
            // Fallback: serialize the whole result
            serde_json::to_string_pretty(&result.content).unwrap_or_else(|_| "(empty result)".to_string())
        } else {
            output_parts.join("\n")
        };

        if result.is_error == Some(true) {
            log_warn!("system", "MCP: tool '{}' returned error: {}", tool_name, output);
            return Err(format!("MCP tool error: {output}"));
        }

        log_info!("system", "MCP: tool '{}' returned {} chars", tool_name, output.len());
        Ok(output)
    }

    /// Gracefully disconnect from the MCP server.
    pub async fn disconnect(mut self) {
        log_info!("system", "MCP: disconnecting from '{}'", self.server_name);
        let _ = self.session.close().await;
    }
}
