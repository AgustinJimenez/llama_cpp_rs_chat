//! MCP (Model Context Protocol) server management tools.

use serde_json::Value;

/// Ensure MCP servers are connected (lazy init). Call before any MCP tool access.
pub(crate) fn ensure_mcp_connected(
    mcp_manager: Option<&super::super::mcp::McpManager>,
    db: Option<&super::super::database::SharedDatabase>,
) {
    let (Some(mgr), Some(db)) = (mcp_manager, db) else { return };
    // Only connect if we have configured servers but none are connected yet
    let configs = super::super::database::mcp::load_mcp_servers(db);
    if configs.is_empty() { return; }
    let statuses = mgr.get_server_statuses();
    let any_connected = statuses.iter().any(|s| s.connected);
    if !any_connected {
        eprintln!("[MCP] Lazy connect: {} configured servers, connecting on first use...", configs.len());
        match mgr.refresh_connections(db) {
            Ok(()) => {
                let connected = mgr.get_connected_server_names();
                let tools = mgr.get_tool_definitions().len();
                eprintln!("[MCP] Lazy connect complete: {} servers, {} tools ({})",
                    connected.len(), tools, connected.join(", "));
            }
            Err(e) => eprintln!("[MCP] Lazy connect failed: {e}"),
        }
    }
}

/// List MCP tools with brief descriptions for the tool catalog.
pub(crate) fn tool_list_mcp_tools(
    mcp_manager: Option<&super::super::mcp::McpManager>,
    db: Option<&super::super::database::SharedDatabase>,
) -> String {
    let db = match db {
        Some(d) => d,
        None => return "No MCP servers configured.".to_string(),
    };

    let configs = super::super::database::mcp::load_mcp_servers(db);
    if configs.is_empty() {
        return "No MCP servers configured. Add servers in Settings → MCP Servers.".to_string();
    }

    // Lazy connect on first access
    ensure_mcp_connected(mcp_manager, Some(db));

    let statuses = mcp_manager.map(|mgr| mgr.get_server_statuses()).unwrap_or_default();
    let tool_defs = mcp_manager.map(|mgr| mgr.get_tool_definitions()).unwrap_or_default();

    let mut lines = Vec::new();
    for cfg in &configs {
        let status = statuses.iter().find(|s| s.id == cfg.id);
        let connected = status.map(|s| s.connected).unwrap_or(false);
        if !cfg.enabled || !connected {
            continue;
        }
        lines.push(format!("## {} (connected)", cfg.name));
        // List tools for this server with brief descriptions
        for td in &tool_defs {
            // MCP tool names are prefixed with mcp__<server>__
            let prefix = format!("mcp__{}__", cfg.name);
            if td.qualified_name.starts_with(&prefix) {
                let brief = td.description.split('.').next().unwrap_or(&td.description);
                lines.push(format!("  {}: {}", td.qualified_name, brief));
            }
        }
    }

    if lines.is_empty() {
        return "No MCP servers are currently connected. Check Settings → MCP Servers and click Refresh.".to_string();
    }
    lines.join("\n")
}

/// Get the full schema for an MCP tool by name.
pub(crate) fn get_mcp_tool_schema(
    tool_name: &str,
    mcp_manager: Option<&super::super::mcp::McpManager>,
) -> Option<String> {
    let mgr = mcp_manager?;
    let tool_defs = mgr.get_tool_definitions();
    let td = tool_defs.iter().find(|t| t.qualified_name == tool_name)?;
    let schema = td.to_openai_function();
    Some(serde_json::to_string_pretty(&schema).unwrap_or_default())
}

pub(crate) fn tool_list_mcp_servers(
    mcp_manager: Option<&super::super::mcp::McpManager>,
    db: Option<&super::super::database::SharedDatabase>,
) -> String {
    let db = match db {
        Some(d) => d,
        None => return "Error: Database not available".to_string(),
    };

    let configs = super::super::database::mcp::load_mcp_servers(db);
    if configs.is_empty() {
        return "No MCP servers configured.".to_string();
    }

    let statuses = mcp_manager.map(|mgr| mgr.get_server_statuses()).unwrap_or_default();

    let mut lines = vec!["MCP Servers:".to_string()];
    for cfg in &configs {
        let status = statuses.iter().find(|s| s.id == cfg.id);
        let connected = status.map(|s| s.connected).unwrap_or(false);
        let tool_count = status.map(|s| s.tool_count).unwrap_or(0);
        let state = if !cfg.enabled {
            "disabled"
        } else if connected {
            "connected"
        } else {
            "disconnected"
        };
        let transport = match &cfg.transport {
            super::super::mcp::config::McpTransport::Stdio { command, args, .. } => {
                if args.is_empty() {
                    format!("stdio: {command}")
                } else {
                    format!("stdio: {command} {}", args.join(" "))
                }
            }
            super::super::mcp::config::McpTransport::Http { url } => format!("http: {url}"),
        };
        lines.push(format!(
            "  - {} [{}] ({}) — {} tool{}",
            cfg.name, state, transport, tool_count,
            if tool_count == 1 { "" } else { "s" }
        ));
        if let Some(st) = status {
            for tool_name in &st.tools {
                lines.push(format!("      • {tool_name}"));
            }
        }
    }
    lines.join("\n")
}

pub(crate) fn tool_add_mcp_server(
    args: &Value,
    mcp_manager: Option<&super::super::mcp::McpManager>,
    db: Option<&super::super::database::SharedDatabase>,
) -> String {
    let db = match db {
        Some(d) => d,
        None => return "Error: Database not available".to_string(),
    };

    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => return "Error: 'name' argument is required".to_string(),
    };

    let transport_str = args.get("transport").and_then(|v| v.as_str()).unwrap_or("stdio");

    let transport = match transport_str {
        "stdio" => {
            let command = match args.get("command").and_then(|v| v.as_str()) {
                Some(c) if !c.is_empty() => c.to_string(),
                _ => return "Error: 'command' argument is required for stdio transport".to_string(),
            };
            let cmd_args: Vec<String> = args.get("args")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            let env_vars: std::collections::HashMap<String, String> = args.get("env_vars")
                .and_then(|v| v.as_object())
                .map(|obj| obj.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
                .unwrap_or_default();
            super::super::mcp::config::McpTransport::Stdio { command, args: cmd_args, env_vars }
        }
        "http" => {
            let url = match args.get("url").and_then(|v| v.as_str()) {
                Some(u) if !u.is_empty() => u.to_string(),
                _ => return "Error: 'url' argument is required for http transport".to_string(),
            };
            super::super::mcp::config::McpTransport::Http { url }
        }
        other => return format!("Error: Unknown transport type '{other}'. Use 'stdio' or 'http'."),
    };

    let id = uuid::Uuid::new_v4().to_string();
    let config = super::super::mcp::config::McpServerConfig {
        id,
        name: name.clone(),
        transport,
        enabled: true,
    };

    if let Err(e) = super::super::database::mcp::save_mcp_server(db, &config) {
        return format!("Error saving MCP server: {e}");
    }

    // Refresh connections to connect the new server
    let mut tool_list = String::new();
    if let Some(mgr) = mcp_manager {
        if let Err(e) = mgr.refresh_connections(db) {
            return format!("Server saved but failed to connect: {e}");
        }
        let statuses = mgr.get_server_statuses();
        if let Some(status) = statuses.iter().find(|s| s.name == name) {
            if status.connected {
                tool_list = if status.tools.is_empty() {
                    String::new()
                } else {
                    format!("\nAvailable tools: {}", status.tools.join(", "))
                };
            } else {
                return format!("MCP server '{}' saved but failed to connect. Check the command/URL and try again.", name);
            }
        }
    }

    format!("Added MCP server '{name}' successfully.{tool_list}\nNote: New tools will be available in the next message.")
}

pub(crate) fn tool_remove_mcp_server(
    args: &Value,
    mcp_manager: Option<&super::super::mcp::McpManager>,
    db: Option<&super::super::database::SharedDatabase>,
) -> String {
    let db = match db {
        Some(d) => d,
        None => return "Error: Database not available".to_string(),
    };

    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) if !n.is_empty() => n,
        _ => return "Error: 'name' argument is required".to_string(),
    };

    let configs = super::super::database::mcp::load_mcp_servers(db);
    let server = configs.iter().find(|c| c.name.eq_ignore_ascii_case(name));

    let server = match server {
        Some(s) => s,
        None => {
            let available: Vec<&str> = configs.iter().map(|c| c.name.as_str()).collect();
            return if available.is_empty() {
                format!("MCP server '{name}' not found. No MCP servers are configured.")
            } else {
                format!("MCP server '{name}' not found. Available servers: {}", available.join(", "))
            };
        }
    };

    let server_id = server.id.clone();
    let server_name = server.name.clone();

    if let Err(e) = super::super::database::mcp::delete_mcp_server(db, &server_id) {
        return format!("Error removing MCP server: {e}");
    }

    // Refresh to disconnect the removed server
    if let Some(mgr) = mcp_manager {
        let _ = mgr.refresh_connections(db);
    }

    format!("Removed MCP server '{server_name}' successfully.")
}
