//! Claude Code provider — spawns the `claude` CLI to use the user's subscription.
//!
//! Protocol: JSON Lines over stdout from `claude --print "prompt" --output-format stream-json`
//! Authentication: Uses the user's existing OAuth login (Max/Pro subscription)

use serde::{Deserialize, Serialize};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

/// Claude Code model options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClaudeModel {
    Opus,
    Sonnet,
    Haiku,
}

impl ClaudeModel {
    pub fn as_str(&self) -> &str {
        match self {
            ClaudeModel::Opus => "opus",
            ClaudeModel::Sonnet => "sonnet",
            ClaudeModel::Haiku => "haiku",
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            ClaudeModel::Opus => "Claude Opus",
            ClaudeModel::Sonnet => "Claude Sonnet",
            ClaudeModel::Haiku => "Claude Haiku",
        }
    }
}

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

/// Token data sent to the frontend (same format as llama.cpp provider)
#[allow(dead_code)]
pub struct ClaudeTokenData {
    pub token: String,
    pub is_done: bool,
    pub session_id: Option<String>,
    pub stop_reason: Option<String>,
    pub cost_usd: Option<f64>,
    pub duration_ms: Option<u64>,
    /// Actual model ID from CLI (e.g. "claude-sonnet-4-6")
    pub model_id: Option<String>,
    /// Total input tokens (including cache)
    pub input_tokens: Option<u64>,
    /// Total output tokens
    pub output_tokens: Option<u64>,
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
    model: &ClaudeModel,
    max_turns: Option<u32>,
    cwd: Option<&str>,
    session_id: Option<&str>,
) -> Result<mpsc::UnboundedReceiver<ClaudeTokenData>, String> {
    let (tx, rx) = mpsc::unbounded_channel();

    let mut cmd = Command::new(claude_cmd());
    cmd.arg("--print")
        .arg(prompt)
        .arg("--output-format")
        .arg("stream-json")
        .arg("--model")
        .arg(model.as_str())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null());

    if let Some(sid) = session_id {
        cmd.arg("--resume").arg(sid);
    }

    if let Some(turns) = max_turns {
        cmd.arg("--max-turns").arg(turns.to_string());
    }

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

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
                                let _ = tx.send(ClaudeTokenData {
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
                                // Format tool call for display
                                let args_str = serde_json::to_string(input).unwrap_or_default();
                                let args_short = if args_str.len() > 200 { format!("{}...", &args_str[..200]) } else { args_str };
                                let tool_display = format!("\n\n**Tool: {}**\n```\n{}\n```\n", name, args_short);
                                let _ = tx.send(ClaudeTokenData {
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
                                let result_display = format!("\n**Output:**\n```\n{}\n```\n", truncated);
                                let _ = tx.send(ClaudeTokenData {
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
                    let _ = tx.send(ClaudeTokenData {
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
        let _ = tx.send(ClaudeTokenData {
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
