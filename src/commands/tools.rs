// ─── Tool Execution ───────────────────────────────────────────────────

use serde::Deserialize;

use crate::web::worker::worker_bridge::SharedWorkerBridge;

#[derive(Deserialize)]
pub struct ToolExecuteRequest {
    pub tool_name: String,
    pub arguments: serde_json::Value,
}

#[tauri::command]
pub async fn execute_tool(
    request: ToolExecuteRequest,
    bridge: tauri::State<'_, SharedWorkerBridge>,
) -> Result<serde_json::Value, String> {
    let meta = bridge.model_status().await;
    let chat_template = meta
        .as_ref()
        .and_then(|m| m.chat_template_type.as_deref())
        .unwrap_or("Unknown");
    let capabilities = crate::web::models::get_model_capabilities(chat_template);
    let (tool_name, tool_arguments) = crate::web::models::translate_tool_for_model(
        &request.tool_name,
        &request.arguments,
        &capabilities,
    );

    let result = match tool_name.as_str() {
        "read_file" => {
            let path = tool_arguments
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if path.is_empty() {
                return Err("File path is required".into());
            }
            match tokio::fs::read_to_string(path).await {
                Ok(content) => serde_json::json!({"success": true, "result": content, "path": path}),
                Err(e) => serde_json::json!({"success": false, "error": format!("Failed to read file: {e}")}),
            }
        }
        "write_file" => {
            let path = tool_arguments
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let content = tool_arguments
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if path.is_empty() {
                return Err("File path is required".into());
            }
            match tokio::fs::write(path, content).await {
                Ok(_) => serde_json::json!({
                    "success": true,
                    "result": format!("Wrote {} bytes to '{path}'", content.len()),
                    "path": path,
                    "bytes_written": content.len()
                }),
                Err(e) => serde_json::json!({"success": false, "error": format!("Failed to write file: {e}")}),
            }
        }
        "list_directory" => {
            let path = tool_arguments
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            match tokio::fs::read_dir(path).await {
                Ok(mut entries) => {
                    let mut items = Vec::new();
                    while let Ok(Some(e)) = entries.next_entry().await {
                        let meta = e.metadata().await.ok();
                        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                        let size =
                            meta.and_then(|m| if m.is_file() { Some(m.len()) } else { None });
                        items.push(format!(
                            "{:>10} {:>15} {}",
                            if is_dir { "DIR" } else { "FILE" },
                            size.map(|s| format!("{s} bytes")).unwrap_or_default(),
                            e.file_name().to_string_lossy()
                        ));
                    }
                    serde_json::json!({"success": true, "result": items.join("\n"), "count": items.len()})
                }
                Err(e) => {
                    serde_json::json!({"success": false, "error": format!("Failed to list directory: {e}")})
                }
            }
        }
        "bash" | "shell" | "command" => {
            let command = tool_arguments
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if command.is_empty() {
                return Err("Command is required".into());
            }
            let cmd = command.to_string();
            let exec = tokio::task::spawn_blocking(move || {
                if cfg!(target_os = "windows") {
                    std::process::Command::new("powershell")
                        .args(["-NoProfile", "-NonInteractive", "-Command", &cmd])
                        .output()
                } else {
                    std::process::Command::new("sh")
                        .arg("-c")
                        .arg(&cmd)
                        .output()
                }
            });
            match tokio::time::timeout(std::time::Duration::from_secs(15), exec).await {
                Ok(Ok(Ok(output))) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    let combined = if stderr.is_empty() {
                        stdout
                    } else {
                        format!("{stdout}\nSTDERR:\n{stderr}")
                    };
                    serde_json::json!({"success": true, "result": combined, "exit_code": output.status.code()})
                }
                Ok(Ok(Err(e))) => {
                    serde_json::json!({"success": false, "error": format!("Failed to execute: {e}")})
                }
                Ok(Err(e)) => {
                    serde_json::json!({"success": false, "error": format!("Task failed: {e}")})
                }
                Err(_) => {
                    serde_json::json!({"success": false, "error": "Command timed out after 15s"})
                }
            }
        }
        "web_fetch" => {
            let url = tool_arguments
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if url.is_empty() {
                return Err("URL is required".into());
            }
            let max_chars = tool_arguments
                .get("max_length")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .unwrap_or(10_000);
            let url_owned = url.to_string();
            tokio::task::spawn_blocking(move || {
                crate::web::routes::tools::fetch_url_as_text(&url_owned, max_chars)
            })
            .await
            .map_err(|e| format!("Task failed: {e}"))?
        }
        _ => serde_json::json!({"success": false, "error": format!("Unknown tool: {}", request.tool_name)}),
    };

    Ok(result)
}

trait Pipe: Sized {
    fn pipe<F, R>(self, f: F) -> R
    where
        F: FnOnce(Self) -> R,
    {
        f(self)
    }
}
impl<T> Pipe for T {}

#[tauri::command]
pub async fn web_fetch(
    url: String,
    max_length: Option<usize>,
) -> Result<serde_json::Value, String> {
    let max_chars = max_length.unwrap_or(10_000);
    tokio::task::spawn_blocking(move || crate::web::routes::tools::fetch_url_as_text(&url, max_chars))
        .await
        .map_err(|e| format!("Task failed: {e}"))?
        .pipe(Ok)
}
