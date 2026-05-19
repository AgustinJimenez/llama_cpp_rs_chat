//! Execution tool handlers: bash/shell/command and web_fetch.

use tokio::task::spawn_blocking;
use tokio::time::{timeout, Duration};

use super::helpers::{fetch_url_as_text, FETCH_TIMEOUT_SECS, MAX_TEXT_CHARS};

pub async fn handle_bash(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let command = tool_arguments.get("command").and_then(|v| v.as_str()).unwrap_or("");
    if command.is_empty() {
        return serde_json::json!({ "success": false, "error": "Command is required" });
    }

    const COMMAND_TIMEOUT_SECS: u64 = 15;
    let cmd_string = command.to_string();
    let exec = spawn_blocking(move || {
        if cfg!(target_os = "windows") {
            sys_debug!(
                "[BASH TOOL] Executing Windows command via PowerShell: {}",
                cmd_string
            );
            llama_chat_engine::utils::silent_command("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", &cmd_string])
                .output()
        } else {
            sys_debug!("[BASH TOOL] Executing Unix command: sh -c {}", cmd_string);
            llama_chat_engine::utils::silent_command("sh")
                .arg("-c")
                .arg(&cmd_string)
                .output()
        }
    });

    match timeout(Duration::from_secs(COMMAND_TIMEOUT_SECS), exec).await {
        Ok(Ok(Ok(output))) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let combined = if !stderr.is_empty() {
                format!("{stdout}\nSTDERR:\n{stderr}")
            } else {
                stdout
            };
            serde_json::json!({
                "success": true,
                "result": combined,
                "exit_code": output.status.code()
            })
        }
        Ok(Ok(Err(e))) => serde_json::json!({
            "success": false,
            "error": format!("Failed to execute command: {}", e)
        }),
        Ok(Err(join_err)) => serde_json::json!({
            "success": false,
            "error": format!("Command task failed: {}", join_err)
        }),
        Err(_) => serde_json::json!({
            "success": false,
            "error": format!("Command timed out after {}s", COMMAND_TIMEOUT_SECS)
        }),
    }
}

pub async fn handle_web_fetch(tool_arguments: &serde_json::Value) -> serde_json::Value {
    let url = tool_arguments.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if url.is_empty() {
        return serde_json::json!({ "success": false, "error": "URL is required" });
    }

    let max_chars = tool_arguments
        .get("max_length")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(MAX_TEXT_CHARS);

    let url_owned = url.to_string();
    match timeout(
        Duration::from_secs(FETCH_TIMEOUT_SECS + 5),
        spawn_blocking(move || fetch_url_as_text(&url_owned, max_chars)),
    )
    .await
    {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => serde_json::json!({
            "success": false,
            "error": format!("Web fetch task failed: {}", e)
        }),
        Err(_) => serde_json::json!({
            "success": false,
            "error": format!("Web fetch timed out after {}s", FETCH_TIMEOUT_SECS)
        }),
    }
}
