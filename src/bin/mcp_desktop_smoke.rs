#![allow(dead_code, unused_imports)]

use std::collections::HashMap;

use serde_json::{json, Value};

#[path = "../web/mod.rs"]
mod web;

use web::mcp::client::McpClient;
use web::mcp::config::{McpServerConfig, McpTransport};

async fn call_tool(client: &McpClient, name: &str, args: Value) -> Result<String, String> {
    let output = client.call_tool(name, args).await?;
    println!("\n== {name} ==\n{output}");
    Ok(output)
}

async fn run_blender_smoke(client: &McpClient) -> Result<(), String> {
    let blender_path = r"C:\Program Files\Blender Foundation\Blender 5.0\blender.exe";
    let title_filter = "blender";

    let open = call_tool(
        client,
        "open_application",
        json!({
            "target": blender_path,
            "args": "--factory-startup",
        }),
    )
    .await?;
    assert!(open.contains("[desktop_result]"));

    let wait = call_tool(
        client,
        "wait_for_window",
        json!({
            "title": title_filter,
            "timeout_ms": 20000,
            "poll_ms": 250,
        }),
    )
    .await?;
    assert!(wait.to_lowercase().contains("blender"));
    assert!(wait.contains("[desktop_result]"));

    let focus = call_tool(client, "focus_window", json!({ "title": title_filter })).await?;
    assert!(focus.to_lowercase().contains("blender"));

    let _ = call_tool(client, "press_key", json!({ "key": "escape" })).await?;
    let _ = call_tool(
        client,
        "click_window_relative",
        json!({
            "title": title_filter,
            "x": 760,
            "y": 430,
            "delay_ms": 400,
        }),
    )
    .await?;
    let _ = call_tool(client, "press_key", json!({ "key": "s" })).await?;
    let typed = call_tool(client, "type_text", json!({ "text": "2" })).await?;
    assert!(typed.contains("[desktop_result]"));
    let _ = call_tool(client, "press_key", json!({ "key": "enter" })).await?;

    let active = call_tool(client, "get_active_window", json!({})).await?;
    assert!(active.to_lowercase().contains("blender"));
    assert!(active.contains("[desktop_result]"));

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let scenario = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "blender".to_string());

    let config = McpServerConfig {
        id: "desktop-smoke".to_string(),
        name: "desktop-tools".to_string(),
        transport: McpTransport::Stdio {
            command: "cargo".to_string(),
            args: vec![
                "run".to_string(),
                "--quiet".to_string(),
                "--bin".to_string(),
                "mcp_desktop_tools".to_string(),
            ],
            env_vars: HashMap::new(),
        },
        enabled: true,
    };

    let client = McpClient::connect(&config).await?;

    let result = match scenario.as_str() {
        "blender" => run_blender_smoke(&client).await,
        other => Err(format!("Unknown smoke scenario '{other}'")),
    };

    client.disconnect().await;
    result
}
