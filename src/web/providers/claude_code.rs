//! Claude Code provider — spawns the `claude` CLI to use the user's subscription.
//!
//! Protocol: JSON Lines over stdout from `claude --print "prompt" --output-format stream-json`
//! Authentication: Uses the user's existing OAuth login (Max/Pro subscription)

use serde::{Deserialize, Serialize};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use super::{resolve_cli_cwd, CliTokenData};

/// Events streamed from the Claude CLI
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClaudeEvent {
    #[serde(rename = "system")]
    System {
        subtype: String,
        session_id: Option<String>,
        model: Option<String>,
    },
    #[serde(rename = "assistant")]
    Assistant {
        message: AssistantMessage,
        session_id: Option<String>,
    },
    #[serde(rename = "result")]
    Result {
        subtype: String,
        #[serde(default)]
        result: Option<String>,
        stop_reason: Option<String>,
        duration_ms: Option<u64>,
        total_cost_usd: Option<f64>,
        session_id: Option<String>,
        #[serde(default)]
        usage: serde_json::Value,
    },
    #[serde(rename = "user")]
    User {
        message: AssistantMessage,
    },
    #[serde(rename = "rate_limit_event")]
    RateLimit {
        rate_limit_info: serde_json::Value,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub content: Vec<ContentBlock>,
    #[serde(default)]
    pub usage: serde_json::Value,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: serde_json::Value,
    },
    #[serde(other)]
    Other,
}

fn normalize_model(model: Option<&str>) -> &str {
    match model {
        Some("opus") => "opus",
        Some("haiku") => "haiku",
        _ => "sonnet",
    }
}

pub fn display_model_name(model: Option<&str>) -> String {
    match normalize_model(model) {
        "opus" => "Claude Opus".to_string(),
        "haiku" => "Claude Haiku".to_string(),
        _ => "Claude Sonnet".to_string(),
    }
}

/// Get the claude CLI command name (handles Windows .cmd wrapper)
fn claude_cmd() -> &'static str {
    if cfg!(target_os = "windows") { "claude.cmd" } else { "claude" }
}

/// Check if the Claude CLI is available
pub async fn is_available() -> bool {
    Command::new(claude_cmd())
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get the Claude CLI version
pub async fn get_version() -> Option<String> {
    let output = Command::new(claude_cmd())
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()?;
    String::from_utf8(output.stdout).ok().map(|s| s.trim().to_string())
}

/// Generate a response using the Claude CLI.
/// If `session_id` is provided, resumes that conversation.
/// Returns a receiver that streams token events.
pub async fn generate(
    prompt: &str,
    model: Option<&str>,
    max_turns: Option<u32>,
    cwd: Option<&str>,
    session_id: Option<&str>,
) -> Result<mpsc::UnboundedReceiver<CliTokenData>, String> {
    let (tx, rx) = mpsc::unbounded_channel();
    let model = normalize_model(model);
    eprintln!("[CLAUDE_CODE] generate() called with prompt={:?} model={} cwd={:?}", &prompt[..prompt.len().min(50)], model, cwd);
    let resolved_cwd = resolve_cli_cwd(cwd)?;
    eprintln!("[CLAUDE_CODE] resolved_cwd={:?}", resolved_cwd);

    // On Windows, use the npm-installed claude binary directly (not the .cmd wrapper)
    // to avoid batch file argument escaping issues with tokio::process::Command.
    let claude_bin = if cfg!(target_os = "windows") {
        let appdata = std::env::var("APPDATA").unwrap_or_default();
        let cli_js = std::path::PathBuf::from(&appdata)
            .join("npm")
            .join("node_modules")
            .join("@anthropic-ai")
            .join("claude-code")
            .join("cli.js");
        eprintln!("[CLAUDE_CODE] Looking for CLI at: {:?} (exists: {})", cli_js, cli_js.exists());
        if cli_js.exists() {
            cli_js.to_string_lossy().to_string()
        } else {
            eprintln!("[CLAUDE_CODE] cli.js not found, falling back to claude_cmd()");
            claude_cmd().to_string() // fallback to .cmd
        }
    } else {
        "claude".to_string()
    };

    eprintln!("[CLAUDE_CODE] Using binary: {}", claude_bin);
    let mut cmd = if claude_bin.ends_with(".js") {
        let mut c = Command::new("node");
        c.arg(&claude_bin);
        c
    } else {
        Command::new(&claude_bin)
    };
    cmd.arg("--print").arg(prompt)
        .arg("--output-format").arg("stream-json")
        .arg("--model").arg(model);
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null());

    if let Some(sid) = session_id {
        cmd.arg("--resume").arg(sid);
    }

    if let Some(turns) = max_turns {
        cmd.arg("--max-turns").arg(turns.to_string());
    }

    // NOTE: --setting-sources none caused "batch file arguments are invalid" on Windows
    // MCP tools and CLAUDE.md will be loaded but this ensures compatibility

    cmd.current_dir(&resolved_cwd);

    let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn claude CLI: {e}"))?;
    let stdout = child.stdout.take().ok_or("Failed to capture claude stdout")?;

    // Read JSON Lines in a background task
    tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        let mut actual_model_id: Option<String> = None;

        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }

            match serde_json::from_str::<ClaudeEvent>(&line) {
                Ok(ClaudeEvent::System { session_id, model, .. }) => {
                    eprintln!("[CLAUDE_CODE] Session started: {:?}, model: {:?}", session_id, model);
                    actual_model_id = model;
                }
                Ok(ClaudeEvent::Assistant { message, .. }) => {
                    for block in &message.content {
                        match block {
                            ContentBlock::Text { text } => {
                                let _ = tx.send(CliTokenData {
                                    token: text.clone(),
                                    is_done: false,
                                    session_id: None,
                                    stop_reason: None,
                                    cost_usd: None,
                                    duration_ms: None,
                                    model_id: actual_model_id.clone(), input_tokens: None, output_tokens: None,
                                });
                            }
                            ContentBlock::ToolUse { name, input, .. } => {
                                // Format as our app's tool call tags so the UI renders with native widgets
                                let args = serde_json::to_string(input).unwrap_or_default();
                                let tool_display = format!(
                                    "\n<tool_call>{{\"name\": \"{}\", \"arguments\": {}}}</tool_call>\n",
                                    name, args
                                );
                                let _ = tx.send(CliTokenData {
                                    token: tool_display,
                                    is_done: false,
                                    session_id: None,
                                    stop_reason: None,
                                    cost_usd: None,
                                    duration_ms: None,
                                    model_id: actual_model_id.clone(), input_tokens: None, output_tokens: None,
                                });
                            }
                            _ => {}
                        }
                    }
                }
                Ok(ClaudeEvent::User { message }) => {
                    // Tool results from Claude's own execution
                    for block in &message.content {
                        if let ContentBlock::ToolResult { content, .. } = block {
                            let result_str = match content {
                                serde_json::Value::String(s) => s.clone(),
                                serde_json::Value::Array(arr) => {
                                    arr.iter().filter_map(|v| {
                                        if v.get("type").and_then(|t| t.as_str()) == Some("text") {
                                            v.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                                        } else { None }
                                    }).collect::<Vec<_>>().join("\n")
                                }
                                other => serde_json::to_string(other).unwrap_or_default(),
                            };
                            if !result_str.is_empty() {
                                let truncated = if result_str.len() > 500 {
                                    format!("{}...", &result_str[..500])
                                } else { result_str };
                                let result_display = format!("\n<tool_response>\n{}\n</tool_response>\n", truncated);
                                let _ = tx.send(CliTokenData {
                                    token: result_display,
                                    is_done: false,
                                    session_id: None,
                                    stop_reason: None,
                                    cost_usd: None,
                                    duration_ms: None,
                                    model_id: actual_model_id.clone(), input_tokens: None, output_tokens: None,
                                });
                            }
                        }
                    }
                }
                Ok(ClaudeEvent::Result { result: _, stop_reason, total_cost_usd, duration_ms, session_id, usage, .. }) => {
                    let input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                        + usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                        + usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                    let output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                    eprintln!("[CLAUDE_CODE] Done: stop_reason={:?}, cost=${:?}, duration={}ms, tokens={}in/{}out",
                        stop_reason, total_cost_usd, duration_ms.unwrap_or(0), input_tokens, output_tokens);
                    let _ = tx.send(CliTokenData {
                        token: String::new(),
                        is_done: true,
                        session_id,
                        stop_reason,
                        cost_usd: total_cost_usd,
                        duration_ms,
                        model_id: actual_model_id.clone(),
                        input_tokens: Some(input_tokens),
                        output_tokens: Some(output_tokens),
                    });
                }
                Ok(ClaudeEvent::RateLimit { rate_limit_info }) => {
                    eprintln!("[CLAUDE_CODE] Rate limit: {:?}", rate_limit_info);
                }
                Ok(ClaudeEvent::Unknown) => {
                    // Ignore unknown event types
                }
                Err(e) => {
                    eprintln!("[CLAUDE_CODE] Failed to parse event: {e}");
                    eprintln!("[CLAUDE_CODE] Raw line: {}", &line[..line.len().min(200)]);
                }
            }
        }

        // Ensure done is sent if CLI exits without result
        let _ = tx.send(CliTokenData {
            token: String::new(),
            is_done: true,
            session_id: None,
            stop_reason: Some("cli_exit".to_string()),
            cost_usd: None,
            duration_ms: None,
            model_id: actual_model_id, input_tokens: None, output_tokens: None,
        });

        let _ = child.wait().await;
    });

    Ok(rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_is_available() {
        let available = is_available().await;
        println!("Claude CLI available: {}", available);
    }

    #[tokio::test]
    async fn test_get_version() {
        if let Some(version) = get_version().await {
            println!("Claude CLI version: {}", version);
        }
    }
}
