// Tool execution route handler

use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::task::spawn_blocking;
use tokio::time::{timeout, Duration};

use crate::web::{
    request_parsing::parse_json_body,
    response_helpers::{json_error, json_raw},
};

// Import logging macros
use crate::{sys_debug, sys_warn};

#[cfg(not(feature = "mock"))]
use crate::web::models::SharedLlamaState;

#[derive(serde::Deserialize)]
struct ToolExecuteRequest {
    tool_name: String,
    arguments: serde_json::Value,
}

async fn canonicalize_allowed(path: &str) -> Result<PathBuf, String> {
    const ROOTS: [&str; 2] = ["/app", "/app/models"];

    let input = path.to_string();
    let canonical = spawn_blocking(move || std::fs::canonicalize(&input))
        .await
        .map_err(|e| format!("Failed to resolve path: {}", e))?
        .map_err(|e| format!("Failed to resolve path: {}", e))?;

    for root in ROOTS {
        let root_path = Path::new(root);
        if canonical.starts_with(root_path) {
            return Ok(canonical);
        }
    }

    Err("Path not allowed".to_string())
}

pub async fn handle_post_tools_execute(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] llama_state: SharedLlamaState,
    #[cfg(feature = "mock")] _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    // Parse request body using helper
    let request: ToolExecuteRequest = match parse_json_body(req.into_body()).await {
        Ok(req) => req,
        Err(error_response) => return Ok(error_response),
    };

    // Get current model's capabilities for tool translation
    #[cfg(not(feature = "mock"))]
    let (tool_name, tool_arguments) = {
        // Access the shared state to get current chat template
        let state_guard = llama_state;
        // Handle poisoned mutex by extracting the inner value
        let state = state_guard.lock().unwrap_or_else(|poisoned| {
            sys_warn!("[WARN] Mutex was poisoned, recovering...");
            poisoned.into_inner()
        });
        let chat_template = state
            .as_ref()
            .and_then(|s| s.chat_template_type.as_deref())
            .unwrap_or("Unknown");
        let capabilities = crate::web::models::get_model_capabilities(chat_template);

        // Translate tool if model doesn't support it natively
        crate::web::models::translate_tool_for_model(
            &request.tool_name,
            &request.arguments,
            &capabilities,
        )
    };

    #[cfg(feature = "mock")]
    let (tool_name, tool_arguments) = (request.tool_name.clone(), request.arguments.clone());

    sys_debug!(
        "[TOOL EXECUTE] Original: {} → Actual: {}",
        request.tool_name,
        tool_name
    );

    // Execute tool based on (possibly translated) name
    let result = match tool_name.as_str() {
        "read_file" => {
            // Extract file path from arguments
            let path = tool_arguments
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if path.is_empty() {
                return Ok(json_error(StatusCode::BAD_REQUEST, "File path is required"));
            }

            // Validate path
            let safe_path = match canonicalize_allowed(path).await {
                Ok(p) => p,
                Err(e) => return Ok(json_error(StatusCode::FORBIDDEN, &e)),
            };

            // Read file
            match fs::read_to_string(&safe_path).await {
                Ok(content) => {
                    serde_json::json!({
                        "success": true,
                        "result": content,
                        "path": safe_path.to_string_lossy()
                    })
                }
                Err(e) => {
                    serde_json::json!({
                        "success": false,
                        "error": format!("Failed to read file '{}': {}", safe_path.to_string_lossy(), e)
                    })
                }
            }
        }
        "write_file" => {
            // Extract path and content from arguments
            let path = tool_arguments
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let content = tool_arguments
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if path.is_empty() {
                return Ok(json_error(StatusCode::BAD_REQUEST, "File path is required"));
            }

            // Validate path
            let safe_path = match canonicalize_allowed(path).await {
                Ok(p) => p,
                Err(e) => return Ok(json_error(StatusCode::FORBIDDEN, &e)),
            };

            // Write file
            match fs::write(&safe_path, content).await {
                Ok(_) => {
                    serde_json::json!({
                        "success": true,
                        "result": format!("Successfully wrote {} bytes to '{}'", content.len(), safe_path.to_string_lossy()),
                        "path": safe_path.to_string_lossy(),
                        "bytes_written": content.len()
                    })
                }
                Err(e) => {
                    serde_json::json!({
                        "success": false,
                        "error": format!("Failed to write file '{}': {}", safe_path.to_string_lossy(), e)
                    })
                }
            }
        }
        "list_directory" => {
            // Extract path and recursive flag from arguments
            let path = tool_arguments
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let recursive = tool_arguments
                .get("recursive")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            // Validate path
            let safe_path = match canonicalize_allowed(path).await {
                Ok(p) => p,
                Err(e) => return Ok(json_error(StatusCode::FORBIDDEN, &e)),
            };

            // List directory contents
            if recursive {
                // Recursive listing using walkdir
                use walkdir::WalkDir;
                let root = safe_path.clone();
                let result = spawn_blocking(move || {
                    let entries: Vec<String> = WalkDir::new(root)
                        .into_iter()
                        .filter_map(|e| e.ok())
                        .map(|e| {
                            let metadata = e.metadata().ok();
                            let size = metadata.as_ref().and_then(|m| {
                                if m.is_file() {
                                    Some(m.len())
                                } else {
                                    None
                                }
                            });
                            let file_type = if e.file_type().is_dir() {
                                "DIR"
                            } else {
                                "FILE"
                            };
                            format!(
                                "{:>10} {:>15} {}",
                                file_type,
                                size.map(|s| format!("{} bytes", s))
                                    .unwrap_or_else(|| "".to_string()),
                                e.path().display()
                            )
                        })
                        .collect();
                    entries
                })
                .await;

                match result {
                    Ok(entries) => serde_json::json!({
                        "success": true,
                        "result": entries.join("\n"),
                        "path": safe_path.to_string_lossy(),
                        "count": entries.len(),
                        "recursive": true
                    }),
                    Err(e) => serde_json::json!({
                        "success": false,
                        "error": format!("Failed to list directory '{}': {}", safe_path.to_string_lossy(), e)
                    }),
                }
            } else {
                // Non-recursive listing
                match fs::read_dir(&safe_path).await {
                    Ok(mut entries) => {
                        let mut items: Vec<String> = Vec::new();
                        while let Ok(Some(e)) = entries.next_entry().await {
                            let metadata = e.metadata().await.ok();
                            let size = metadata.as_ref().and_then(|m| {
                                if m.is_file() {
                                    Some(m.len())
                                } else {
                                    None
                                }
                            });
                            let file_type =
                                if metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false) {
                                    "DIR"
                                } else {
                                    "FILE"
                                };
                            items.push(format!(
                                "{:>10} {:>15} {}",
                                file_type,
                                size.map(|s| format!("{} bytes", s))
                                    .unwrap_or_else(|| "".to_string()),
                                e.file_name().to_string_lossy()
                            ));
                        }

                        serde_json::json!({
                            "success": true,
                            "result": items.join("\n"),
                            "path": safe_path.to_string_lossy(),
                            "count": items.len(),
                            "recursive": false
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "success": false,
                            "error": format!("Failed to list directory '{}': {}", safe_path.to_string_lossy(), e)
                        })
                    }
                }
            }
        }
        "bash" | "shell" | "command" => {
            // Extract command from arguments
            let command = tool_arguments
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if command.is_empty() {
                return Ok(json_error(StatusCode::BAD_REQUEST, "Command is required"));
            }

            const COMMAND_TIMEOUT_SECS: u64 = 15;
            // Execute command with timeout to avoid hanging tasks
            let cmd_string = command.to_string();
            let exec = spawn_blocking(move || {
                if cfg!(target_os = "windows") {
                    sys_debug!(
                        "[BASH TOOL] Executing Windows command via PowerShell: {}",
                        cmd_string
                    );
                    std::process::Command::new("powershell")
                        .args(["-NoProfile", "-NonInteractive", "-Command", &cmd_string])
                        .output()
                } else {
                    sys_debug!("[BASH TOOL] Executing Unix command: sh -c {}", cmd_string);
                    std::process::Command::new("sh")
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
                        format!("{}\nSTDERR:\n{}", stdout, stderr)
                    } else {
                        stdout
                    };

                    serde_json::json!({
                        "success": true,
                        "result": combined,
                        "exit_code": output.status.code()
                    })
                }
                Ok(Ok(Err(e))) => {
                    serde_json::json!({
                        "success": false,
                        "error": format!("Failed to execute command: {}", e)
                    })
                }
                Ok(Err(join_err)) => {
                    serde_json::json!({
                        "success": false,
                        "error": format!("Command task failed: {}", join_err)
                    })
                }
                Err(_) => {
                    serde_json::json!({
                        "success": false,
                        "error": format!("Command timed out after {}s", COMMAND_TIMEOUT_SECS)
                    })
                }
            }
        }
        _ => {
            serde_json::json!({
                "success": false,
                "error": format!("Unknown tool: {}", request.tool_name)
            })
        }
    };

    Ok(json_raw(StatusCode::OK, result.to_string()))
}
