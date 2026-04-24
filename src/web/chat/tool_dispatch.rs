use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;

use super::super::background::execute_command_background;
use super::super::command::{execute_command_streaming_with_timeout, strip_ansi_codes};
use super::super::models::*;
use super::super::native_tools;
use super::sub_agent::{run_sub_agent};
use super::tool_tags::ToolTags;
use crate::{log_info};

/// Prefix a command with `rtk` for output compression, if RTK mode is enabled.
pub(super) fn maybe_rtk_prefix(cmd: &str, use_rtk: bool) -> String {
    if use_rtk {
        format!("rtk {}", cmd)
    } else {
        cmd.to_string()
    }
}

/// Check if a command is potentially destructive and return a warning.
pub(super) fn detect_destructive_command(cmd: &str) -> Option<&'static str> {
    let lower = cmd.to_lowercase();

    // File deletion
    if (lower.contains("rm ") || lower.contains("del ") || lower.contains("remove-item"))
        && (lower.contains("-rf") || lower.contains("-r") || lower.contains("--force") || lower.contains("-recurse"))
    {
        return Some("WARNING: This command may permanently delete files/directories.");
    }

    // Git destructive operations
    if lower.contains("git") {
        if lower.contains("reset --hard") {
            return Some("WARNING: git reset --hard discards uncommitted changes.");
        }
        if lower.contains("push") && (lower.contains("--force") || lower.contains("-f")) {
            return Some("WARNING: Force push may overwrite remote history.");
        }
        if lower.contains("clean") && lower.contains("-f") && !lower.contains("-n") && !lower.contains("--dry-run") {
            return Some("WARNING: git clean -f permanently deletes untracked files.");
        }
        if lower.contains("checkout -- .") || lower.contains("restore -- .") {
            return Some("WARNING: May discard all working tree changes.");
        }
    }

    // Database operations
    if lower.contains("drop table") || lower.contains("drop database") || lower.contains("truncate table") {
        return Some("WARNING: This SQL command causes irreversible data loss.");
    }

    // Disk operations
    if lower.contains("format ") || lower.contains("mkfs") || lower.contains("dd if=") {
        return Some("WARNING: This command may destroy disk data.");
    }

    // Process killing
    if (lower.contains("taskkill") || lower.contains("kill -9") || lower.contains("killall"))
        && !lower.contains("/pid") // Our own PID-based kills are fine
    {
        return Some("WARNING: This command kills processes.");
    }

    None
}

/// Check for potential command injection patterns.
/// Returns an error message if dangerous patterns are found.
pub(super) fn detect_command_injection(cmd: &str) -> Option<String> {
    // Check for command substitution that could be injection
    let patterns = [
        ("$(", "command substitution $()"),
        ("${", "parameter expansion ${}"),
        ("`", "backtick command substitution"),
    ];

    // Only flag these if they appear in tool arguments (not in the command itself)
    // For now, just log a warning but don't block execution
    for (pattern, desc) in &patterns {
        if cmd.contains(pattern) {
            eprintln!("[SECURITY] Command contains {}: {}", desc, &cmd[..cmd.len().min(100)]);
        }
    }

    // Block obviously dangerous patterns
    if cmd.contains("| base64") && cmd.contains("curl") {
        return Some("BLOCKED: Potential data exfiltration detected (curl + base64 pipe)".to_string());
    }
    if cmd.contains("eval ") && (cmd.contains("curl") || cmd.contains("wget")) {
        return Some("BLOCKED: Potential remote code execution (eval + download)".to_string());
    }

    None
}

/// Tools that are safe to execute in parallel (no side effects).
/// `execute_command` is always serial since it's hard to detect read-only shell commands reliably.
pub(super) const READ_ONLY_TOOLS: &[&str] = &[
    "read_file", "search_files", "find_files", "list_directory",
    "web_search", "web_fetch", "lsp_query", "git_status", "git_diff",
    "check_background_process", "list_background_processes",
    "todo_read", "list_tools", "get_tool_details", "list_skills",
    "take_screenshot", "list_windows", "get_cursor_position",
    "get_active_window", "list_monitors", "ocr_screen",
    // Browser tools — read-only, safe for parallel execution
    "browser_get_text", "browser_get_html", "browser_get_links",
    "browser_snapshot", "browser_screenshot", "browser_wait",
];

/// Maximum number of tools to execute concurrently in a parallel batch.
pub(super) const MAX_PARALLEL_TOOLS: usize = 10;

pub(super) fn is_read_only_tool(name: &str) -> bool {
    READ_ONLY_TOOLS.contains(&name)
}

/// Default timeout for native tool execution.
const NATIVE_TOOL_TIMEOUT_SECS: u64 = 30;
/// Browser tools may need longer for first-run startup, tab creation, and slow pages.
const BROWSER_TOOL_TIMEOUT_SECS: u64 = 90;

/// Run a native tool with a timeout to prevent blocking the generation thread indefinitely.
pub(super) fn run_native_tool_with_timeout(
    command_text: &str,
    web_search_provider: Option<&str>,
    web_search_api_key: Option<&str>,
    conversation_id: &str,
    use_htmd: bool,
    browser_backend: crate::web::browser::BrowserBackend,
    mcp_manager: Option<Arc<crate::web::mcp::McpManager>>,
    db: crate::web::database::SharedDatabase,
) -> Option<native_tools::NativeToolResult> {
    let cmd = command_text.to_string();
    let provider = web_search_provider.map(|s| s.to_string());
    let api_key = web_search_api_key.map(|s| s.to_string());
    let mcp = mcp_manager.clone();
    let db = db.clone();

    // Extract tool name for logging
    let tool_name = native_tools::extract_tool_name(&cmd).unwrap_or_else(|| "unknown".to_string());
    let tool_args_summary = native_tools::extract_tool_args_summary(&cmd);
    crate::web::event_log::log_event(conversation_id, "tool_start", &format!("{}: {}", tool_name, tool_args_summary));
    let tool_start = std::time::Instant::now();

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = native_tools::dispatch_native_tool(
            &cmd,
            provider.as_deref(),
            api_key.as_deref(),
            use_htmd,
            &browser_backend,
            mcp.as_deref(),
            Some(&db),
        );
        let _ = tx.send(result);
    });

    let timeout_secs = if tool_name.starts_with("browser_")
        || tool_name == "open_browser_view"
        || tool_name == "close_browser_view"
    {
        BROWSER_TOOL_TIMEOUT_SECS
    } else {
        NATIVE_TOOL_TIMEOUT_SECS
    };

    match rx.recv_timeout(std::time::Duration::from_secs(timeout_secs)) {
        Ok(result) => {
            let elapsed = tool_start.elapsed();
            let output_len = result.as_ref().map(|r| r.text.len()).unwrap_or(0);
            crate::web::event_log::log_event(conversation_id, "tool_end", &format!(
                "{}: {:.1}s, {} chars output", tool_name, elapsed.as_secs_f64(), output_len
            ));
            result
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            crate::web::event_log::log_event(conversation_id, "tool_timeout", &format!(
                "{}: timed out after {}s", tool_name, timeout_secs
            ));
            log_info!(conversation_id, "⏱️ Native tool timed out after {}s", timeout_secs);
            Some(native_tools::NativeToolResult::text_only(format!(
                "Error: Tool execution timed out after {} seconds. The network request may be slow or unresponsive. Please try again.",
                timeout_secs
            )))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            log_info!(conversation_id, "⚠️ Native tool thread panicked");
            Some(native_tools::NativeToolResult::text_only("Error: Tool execution failed unexpectedly.".to_string()))
        }
    }
}

/// Execute a single tool call given its parsed name and arguments.
/// Returns (text_output, image_bytes). Used by the batch execution path.
pub(super) fn execute_single_tool(
    name: &str,
    args: &serde_json::Value,
    tool_json: &str,
    conversation_id: &str,
    web_search_provider: Option<&str>,
    web_search_api_key: Option<&str>,
    token_sender: &Option<mpsc::UnboundedSender<TokenData>>,
    token_pos: i32,
    context_size: u32,
    cancel: Option<Arc<AtomicBool>>,
    use_rtk: bool,
    use_htmd: bool,
    browser_backend: &crate::web::browser::BrowserBackend,
    mcp_manager: Option<Arc<crate::web::mcp::McpManager>>,
    db: crate::web::database::SharedDatabase,
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    chat_template_string: Option<&str>,
    tags: &ToolTags,
) -> (String, Vec<Vec<u8>>) {
    // spawn_agent: run a sub-agent with fresh context
    if name == "spawn_agent" {
        let task = args.get("task").and_then(|v| v.as_str()).unwrap_or("");
        if task.is_empty() {
            return ("Error: 'task' argument is required for spawn_agent".to_string(), Vec::new());
        }
        let extra_context = args.get("context").and_then(|v| v.as_str());
        match run_sub_agent(
            model, backend, task, extra_context, chat_template_string,
            conversation_id, tags, web_search_provider, web_search_api_key,
            use_rtk, use_htmd, browser_backend, mcp_manager.clone(), db.clone(),
            token_sender,
        ) {
            Ok(result) => return (result, Vec::new()),
            Err(e) => return (format!("Sub-agent error: {}", e), Vec::new()),
        }
    }

    // execute_command gets streaming or background treatment (no images)
    if name == "execute_command" {
        if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
            if !cmd.is_empty() {
                // Security checks
                if let Some(injection_msg) = detect_command_injection(cmd) {
                    return (injection_msg, Vec::new());
                }
                if let Some(warning) = detect_destructive_command(cmd) {
                    eprintln!("[SECURITY] {}: {}", warning, &cmd[..cmd.len().min(100)]);
                    crate::web::event_log::log_event(conversation_id, "security_warning", &format!("{}: {}", warning, &cmd[..cmd.len().min(80)]));
                }

                let is_background = args.get("background").map(|v| {
                    v.as_bool().unwrap_or_else(|| {
                        v.as_str().map(|s| matches!(s.trim().to_lowercase().as_str(), "true" | "1" | "yes")).unwrap_or(false)
                    })
                }).unwrap_or(false);
                let timeout_secs = args.get("timeout").and_then(|v| {
                    v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
                });
                let rtk_cmd = maybe_rtk_prefix(cmd, use_rtk);
                if is_background {
                    log_info!(conversation_id, "🐚 Batch: background execute_command: {}", rtk_cmd);
                    let sender_clone = token_sender.clone();
                    let text = execute_command_background(&rtk_cmd, |line| {
                        if let Some(ref sender) = sender_clone {
                            let _ = sender.send(TokenData {
                                token: format!("{}\n", strip_ansi_codes(line)),
                                tokens_used: token_pos,
                                max_tokens: context_size as i32, status: None,
                                ..Default::default()
                            });
                        }
                    });
                    return (text, Vec::new());
                } else {
                    log_info!(conversation_id, "🐚 Batch: streaming execute_command (timeout={}s): {}", timeout_secs.unwrap_or(300), rtk_cmd);
                    let sender_clone = token_sender.clone();
                    let text = execute_command_streaming_with_timeout(&rtk_cmd, cancel, timeout_secs, &mut |line| {
                        if let Some(ref sender) = sender_clone {
                            let _ = sender.send(TokenData {
                                token: format!("{}\n", strip_ansi_codes(line)),
                                tokens_used: token_pos,
                                max_tokens: context_size as i32, status: None,
                                ..Default::default()
                            });
                        }
                    });
                    return (text, Vec::new());
                }
            }
        }
    }

    // Try native tool dispatch (may return images for vision)
    if let Some(native_result) = run_native_tool_with_timeout(
        tool_json,
        web_search_provider,
        web_search_api_key,
        conversation_id,
        use_htmd,
        browser_backend.clone(),
        mcp_manager.clone(),
        db.clone(),
    ) {
        log_info!(conversation_id, "📦 Batch: native tool '{}' dispatched (images={})", name, native_result.images.len());
        if let Some(ref sender) = token_sender {
            let _ = sender.send(TokenData {
                token: native_result.text.trim().to_string(),
                tokens_used: token_pos,
                max_tokens: context_size as i32, status: None,
                ..Default::default()
            });
        }
        return (native_result.text, native_result.images);
    }

    // Fallback: unknown tool
    let err = format!("Error: Unknown or unsupported tool '{}'", name);
    if let Some(ref sender) = token_sender {
        let _ = sender.send(TokenData {
            token: err.clone(),
            tokens_used: token_pos,
            max_tokens: context_size as i32, status: None,
            ..Default::default()
        });
    }
    (err, Vec::new())
}
