// Tool execution route handler

use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;
use std::fs;

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

pub async fn handle_post_tools_execute(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))]
    llama_state: SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
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
        let chat_template = state.as_ref()
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

    sys_debug!("[TOOL EXECUTE] Original: {} → Actual: {}", request.tool_name, tool_name);

    // Execute tool based on (possibly translated) name
    let result = match tool_name.as_str() {
        "read_file" => {
            // Extract file path from arguments
            let path = tool_arguments.get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if path.is_empty() {
                return Ok(json_error(StatusCode::BAD_REQUEST, "File path is required"));
            }

            // Read file
            match fs::read_to_string(path) {
                Ok(content) => {
                    serde_json::json!({
                        "success": true,
                        "result": content,
                        "path": path
                    })
                }
                Err(e) => {
                    serde_json::json!({
                        "success": false,
                        "error": format!("Failed to read file '{}': {}", path, e)
                    })
                }
            }
        }
        "write_file" => {
            // Extract path and content from arguments
            let path = tool_arguments.get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let content = tool_arguments.get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if path.is_empty() {
                return Ok(json_error(StatusCode::BAD_REQUEST, "File path is required"));
            }

            // Write file
            match fs::write(path, content) {
                Ok(_) => {
                    serde_json::json!({
                        "success": true,
                        "result": format!("Successfully wrote {} bytes to '{}'", content.len(), path),
                        "path": path,
                        "bytes_written": content.len()
                    })
                }
                Err(e) => {
                    serde_json::json!({
                        "success": false,
                        "error": format!("Failed to write file '{}': {}", path, e)
                    })
                }
            }
        }
        "list_directory" => {
            // Extract path and recursive flag from arguments
            let path = tool_arguments.get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let recursive = tool_arguments.get("recursive")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            // List directory contents
            if recursive {
                // Recursive listing using walkdir
                use walkdir::WalkDir;
                let entries: Vec<String> = WalkDir::new(path)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .map(|e| {
                        let metadata = e.metadata().ok();
                        let size = metadata.as_ref().and_then(|m| if m.is_file() { Some(m.len()) } else { None });
                        let file_type = if e.file_type().is_dir() { "DIR" } else { "FILE" };
                        format!("{:>10} {:>15} {}",
                            file_type,
                            size.map(|s| format!("{} bytes", s)).unwrap_or_else(|| "".to_string()),
                            e.path().display()
                        )
                    })
                    .collect();

                serde_json::json!({
                    "success": true,
                    "result": entries.join("\n"),
                    "path": path,
                    "count": entries.len(),
                    "recursive": true
                })
            } else {
                // Non-recursive listing
                match fs::read_dir(path) {
                    Ok(entries) => {
                        let items: Vec<String> = entries
                            .filter_map(|e| e.ok())
                            .map(|e| {
                                let metadata = e.metadata().ok();
                                let size = metadata.as_ref().and_then(|m| if m.is_file() { Some(m.len()) } else { None });
                                let file_type = if metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false) { "DIR" } else { "FILE" };
                                format!("{:>10} {:>15} {}",
                                    file_type,
                                    size.map(|s| format!("{} bytes", s)).unwrap_or_else(|| "".to_string()),
                                    e.file_name().to_string_lossy()
                                )
                            })
                            .collect();

                        serde_json::json!({
                            "success": true,
                            "result": items.join("\n"),
                            "path": path,
                            "count": items.len(),
                            "recursive": false
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "success": false,
                            "error": format!("Failed to list directory '{}': {}", path, e)
                        })
                    }
                }
            }
        }
        "bash" | "shell" | "command" => {
            // Extract command from arguments
            let command = tool_arguments.get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if command.is_empty() {
                return Ok(json_error(StatusCode::BAD_REQUEST, "Command is required"));
            }

            // Execute command (with timeout for safety)
            let output = if cfg!(target_os = "windows") {
                // Use PowerShell on Windows for better path and quoting handling
                // PowerShell handles backslashes and quotes much better than cmd.exe
                sys_debug!("[BASH TOOL] Executing Windows command via PowerShell: {}", command);
                std::process::Command::new("powershell")
                    .args(["-NoProfile", "-NonInteractive", "-Command", command])
                    .output()
            } else {
                sys_debug!("[BASH TOOL] Executing Unix command: sh -c {}", command);
                std::process::Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .output()
            };

            match output {
                Ok(output) => {
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
                Err(e) => {
                    serde_json::json!({
                        "success": false,
                        "error": format!("Failed to execute command: {}", e)
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
