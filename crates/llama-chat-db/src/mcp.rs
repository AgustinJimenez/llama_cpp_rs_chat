//! Database CRUD operations for MCP server configurations.

use super::{db_error, current_timestamp_secs, SharedDatabase};
use llama_chat_types::mcp_config::{McpServerConfig, McpTransport};
use std::collections::HashMap;

/// Load all MCP server configurations from the database.
pub fn load_mcp_servers(db: &SharedDatabase) -> Vec<McpServerConfig> {
    let conn = db.connection();
    let mut stmt = match conn.prepare(
        "SELECT id, name, transport, command, args, env_vars, url, enabled FROM mcp_servers"
    ) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("MCP: failed to prepare query: {e}");
            return Vec::new();
        }
    };

    let rows = match stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let name: String = row.get(1)?;
        let transport_type: String = row.get(2)?;
        let command: Option<String> = row.get(3)?;
        let args_json: Option<String> = row.get(4)?;
        let env_json: Option<String> = row.get(5)?;
        let url: Option<String> = row.get(6)?;
        let enabled: bool = row.get(7)?;

        Ok((id, name, transport_type, command, args_json, env_json, url, enabled))
    }) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("MCP: failed to query mcp_servers: {e}");
            return Vec::new();
        }
    };

    let mut configs = Vec::new();
    for row in rows {
        if let Ok((id, name, transport_type, command, args_json, env_json, url, enabled)) = row {
            let transport = match transport_type.as_str() {
                "stdio" => {
                    let cmd = command.unwrap_or_default();
                    let args: Vec<String> = args_json
                        .and_then(|j| serde_json::from_str(&j).ok())
                        .unwrap_or_default();
                    let env_vars: HashMap<String, String> = env_json
                        .and_then(|j| serde_json::from_str(&j).ok())
                        .unwrap_or_default();
                    McpTransport::Stdio { command: cmd, args, env_vars }
                }
                "http" => {
                    McpTransport::Http { url: url.unwrap_or_default() }
                }
                other => {
                    eprintln!("MCP: unknown transport type '{other}' for server '{name}'");
                    continue;
                }
            };

            configs.push(McpServerConfig { id, name, transport, enabled });
        }
    }

    configs
}

/// Save (insert or update) an MCP server configuration.
pub fn save_mcp_server(db: &SharedDatabase, config: &McpServerConfig) -> Result<(), String> {
    let conn = db.connection();
    let now = current_timestamp_secs() as i64;

    let (transport_type, command, args_json, env_json, url) = match &config.transport {
        McpTransport::Stdio { command, args, env_vars } => {
            let args_j = serde_json::to_string(args).unwrap_or_else(|_| "[]".to_string());
            let env_j = serde_json::to_string(env_vars).unwrap_or_else(|_| "{}".to_string());
            ("stdio", Some(command.clone()), Some(args_j), Some(env_j), None)
        }
        McpTransport::Http { url } => {
            ("http", None, None, None, Some(url.clone()))
        }
    };

    conn.execute(
        "INSERT INTO mcp_servers (id, name, transport, command, args, env_vars, url, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name,
           transport = excluded.transport,
           command = excluded.command,
           args = excluded.args,
           env_vars = excluded.env_vars,
           url = excluded.url,
           enabled = excluded.enabled,
           updated_at = excluded.updated_at",
        rusqlite::params![
            config.id,
            config.name,
            transport_type,
            command,
            args_json,
            env_json,
            url,
            config.enabled,
            now,
        ],
    ).map_err(db_error("save MCP server"))?;

    Ok(())
}

/// Delete an MCP server configuration by ID.
pub fn delete_mcp_server(db: &SharedDatabase, id: &str) -> Result<(), String> {
    let conn = db.connection();
    conn.execute("DELETE FROM mcp_servers WHERE id = ?1", [id])
        .map_err(db_error("delete MCP server"))?;
    Ok(())
}

/// Toggle an MCP server's enabled status.
pub fn toggle_mcp_server(db: &SharedDatabase, id: &str, enabled: bool) -> Result<(), String> {
    let conn = db.connection();
    let now = current_timestamp_secs() as i64;
    conn.execute(
        "UPDATE mcp_servers SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![enabled, now, id],
    ).map_err(db_error("toggle MCP server"))?;
    Ok(())
}
