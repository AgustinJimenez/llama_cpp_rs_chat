#![allow(dead_code, unused_imports)]

#[macro_use]
extern crate llama_chat_types;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

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

fn assert_desktop_result(output: &str) {
    assert!(
        output.contains("[desktop_result]"),
        "desktop result footer missing from tool output: {output}"
    );
}

fn extract_pid(output: &str) -> Result<u32, String> {
    let marker = "pid=";
    let start = output
        .find(marker)
        .ok_or_else(|| format!("No pid found in output: {output}"))?
        + marker.len();
    let digits: String = output[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return Err(format!("Malformed pid in output: {output}"));
    }
    digits
        .parse::<u32>()
        .map_err(|e| format!("Parsing pid '{digits}': {e}"))
}

fn temp_smoke_file_path() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    std::env::temp_dir().join(format!("llama_cpp_desktop_smoke_{timestamp}.txt"))
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
    assert_desktop_result(&open);

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
    assert_desktop_result(&wait);

    let focus = call_tool(client, "focus_window", json!({ "title": title_filter })).await?;
    assert!(focus.to_lowercase().contains("blender"));
    let pid = extract_pid(&focus)?;

    let verified_escape = call_tool(
        client,
        "press_key",
        json!({
            "key": "escape",
            "verify_screen_change": true,
            "verify_threshold_pct": 0.1,
            "verify_timeout_ms": 2000,
        }),
    )
    .await?;
    assert_desktop_result(&verified_escape);
    let _ = call_tool(
        client,
        "click_window_relative",
        json!({
            "pid": pid,
            "x": 760,
            "y": 430,
            "delay_ms": 400,
        }),
    )
    .await?;
    let _ = call_tool(client, "press_key", json!({ "key": "s" })).await?;
    let typed = call_tool(client, "type_text", json!({ "text": "2" })).await?;
    assert_desktop_result(&typed);
    let _ = call_tool(client, "press_key", json!({ "key": "enter" })).await?;

    let active = call_tool(client, "get_active_window", json!({})).await?;
    assert!(active.to_lowercase().contains("blender"));
    assert_desktop_result(&active);
    assert_eq!(extract_pid(&active)?, pid);

    Ok(())
}

async fn run_notepad_smoke(client: &McpClient) -> Result<(), String> {
    let file_path = temp_smoke_file_path();
    let expected = "Hello";
    fs::write(&file_path, "").map_err(|e| format!("creating temp file: {e}"))?;

    let open = call_tool(
        client,
        "open_application",
        json!({
            "target": "notepad.exe",
            "args": file_path.display().to_string(),
        }),
    )
    .await?;
    assert_desktop_result(&open);

    let wait = call_tool(
        client,
        "wait_for_window",
        json!({
            "title": "notepad",
            "timeout_ms": 20000,
            "poll_ms": 250,
        }),
    )
    .await?;
    assert!(wait.to_lowercase().contains("notepad"));
    assert_desktop_result(&wait);

    let focus = call_tool(client, "focus_window", json!({ "title": "notepad" })).await?;
    assert!(focus.to_lowercase().contains("notepad"));
    let pid = extract_pid(&focus)?;

    let typed = call_tool(
        client,
        "send_keys_to_window",
        json!({
            "pid": pid,
            "text": expected,
            "method": "send_input",
        }),
    )
    .await?;
    assert_desktop_result(&typed);
    std::thread::sleep(std::time::Duration::from_millis(400));

    let ocr = call_tool(
        client,
        "ocr_find_text",
        json!({
            "pid": pid,
            "text": expected,
        }),
    )
    .await?;
    assert_desktop_result(&ocr);
    assert!(
        ocr.to_lowercase().contains("found") && ocr.to_lowercase().contains(&expected.to_lowercase()),
        "OCR find-text smoke did not find expected text. Output: {ocr}"
    );

    let saved = call_tool(
        client,
        "send_keys_to_window",
        json!({
            "pid": pid,
            "keys": "ctrl+s",
            "method": "send_input",
        }),
    )
    .await?;
    assert_desktop_result(&saved);
    std::thread::sleep(std::time::Duration::from_millis(300));

    let snapped = call_tool(
        client,
        "snap_window",
        json!({
            "pid": pid,
            "position": "left",
        }),
    )
    .await?;
    assert_desktop_result(&snapped);

    let active = call_tool(client, "get_active_window", json!({})).await?;
    assert!(active.to_lowercase().contains("notepad"));
    assert_eq!(extract_pid(&active)?, pid);
    assert_desktop_result(&active);

    let closed = call_tool(client, "close_window", json!({ "pid": pid })).await?;
    assert_desktop_result(&closed);

    std::thread::sleep(std::time::Duration::from_millis(700));
    let contents = fs::read_to_string(&file_path).map_err(|e| format!("reading temp file: {e}"))?;
    if let Err(e) = fs::remove_file(&file_path) {
        eprintln!("warning: failed to remove temp smoke file {}: {e}", file_path.display());
    }
    assert!(
        contents.contains(expected),
        "Notepad smoke file did not contain expected text. Contents: {contents:?}"
    );

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
        "notepad" => run_notepad_smoke(&client).await,
        other => Err(format!("Unknown smoke scenario '{other}'")),
    };

    client.disconnect().await;
    result
}
