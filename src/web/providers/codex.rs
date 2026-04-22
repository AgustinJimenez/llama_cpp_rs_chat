//! Codex CLI provider — spawns the local `codex` CLI using the user's existing auth.
//!
//! Protocol: JSON Lines over stdout from `codex exec --json`

use serde::Deserialize;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

fn hide_window(cmd: &mut Command) {
    super::hide_cli_window(cmd);
}

use super::{resolve_cli_cwd, CliTokenData};

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum CodexEvent {
    #[serde(rename = "thread.started")]
    ThreadStarted {
        thread_id: String,
    },
    #[serde(rename = "item.completed")]
    ItemCompleted {
        item: CodexItem,
    },
    #[serde(rename = "turn.completed")]
    TurnCompleted {
        usage: Option<CodexUsage>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
struct CodexItem {
    #[serde(rename = "type")]
    item_type: String,
    text: Option<String>,
    // Tool call fields (for function_call items)
    name: Option<String>,
    arguments: Option<String>,
    // Tool result fields
    output: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CodexUsage {
    input_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

fn codex_cmd() -> &'static str {
    "codex"
}

/// On Windows, find the actual codex.js entrypoint and run via `node` directly.
/// This avoids the .cmd batch wrapper which breaks with tokio::process::Command.
#[cfg(target_os = "windows")]
fn resolve_codex_js() -> Option<std::path::PathBuf> {
    // Try npm user global: %APPDATA%/npm/node_modules/@openai/codex/bin/codex.js
    if let Ok(appdata) = std::env::var("APPDATA") {
        let js = std::path::PathBuf::from(&appdata)
            .join("npm/node_modules/@openai/codex/bin/codex.js");
        if js.exists() {
            return Some(js);
        }
    }
    // Try global nodejs dir
    let global = std::path::PathBuf::from("C:/Program Files/nodejs/node_modules/@openai/codex/bin/codex.js");
    if global.exists() {
        return Some(global);
    }
    None
}

fn normalize_model(model: Option<&str>) -> Option<&str> {
    match model {
        Some("") | None => None,
        other => other,
    }
}

pub fn display_model_name(model: Option<&str>) -> String {
    normalize_model(model).unwrap_or("gpt-5").to_string()
}

pub async fn is_available() -> bool {
    // On Windows, try node + codex.js directly (avoids broken .cmd wrapper)
    #[cfg(target_os = "windows")]
    if let Some(js) = resolve_codex_js() {
        let mut cmd = Command::new("node");
        cmd.arg(&js).arg("--version").stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null());
        hide_window(&mut cmd);
        if let Ok(o) = cmd.output().await {
            if o.status.success() {
                return true;
            }
        }
    }
    let mut cmd = Command::new(codex_cmd());
    cmd.arg("--version").stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null());
    hide_window(&mut cmd);
    cmd.output().await.map(|o| o.status.success()).unwrap_or(false)
}

pub async fn get_version() -> Option<String> {
    // On Windows, try node + codex.js directly
    #[cfg(target_os = "windows")]
    if let Some(js) = resolve_codex_js() {
        let mut cmd = Command::new("node");
        cmd.arg(&js).arg("--version").stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null());
        hide_window(&mut cmd);
        if let Ok(o) = cmd.output().await {
            if o.status.success() {
                let v = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
    }
    let mut cmd = Command::new(codex_cmd());
    cmd.arg("--version").stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null());
    hide_window(&mut cmd);
    let output = cmd.output().await.ok()?;
    String::from_utf8(output.stdout).ok().map(|s| s.trim().to_string())
}

pub async fn generate(
    prompt: &str,
    model: Option<&str>,
    _max_turns: Option<u32>,
    cwd: Option<&str>,
    session_id: Option<&str>,
) -> Result<mpsc::UnboundedReceiver<CliTokenData>, String> {
    let (tx, rx) = mpsc::unbounded_channel();
    let requested_model = normalize_model(model).map(str::to_string);
    let resolved_cwd = resolve_cli_cwd(cwd)?;

    // On Windows, use node + codex.js directly to avoid .cmd wrapper issues
    #[cfg(target_os = "windows")]
    let mut cmd = if let Some(js) = resolve_codex_js() {
        let mut c = Command::new("node");
        c.arg(js);
        c
    } else {
        Command::new(codex_cmd())
    };
    #[cfg(not(target_os = "windows"))]
    let mut cmd = Command::new(codex_cmd());

    if let Some(sid) = session_id {
        cmd.arg("exec").arg("resume").arg("--json");
        cmd.arg("--dangerously-bypass-approvals-and-sandbox");
        cmd.arg(sid);
    } else {
        cmd.arg("exec").arg("--json");
        cmd.arg("--skip-git-repo-check");
        cmd.arg("--dangerously-bypass-approvals-and-sandbox");
    }

    if let Some(model) = requested_model.as_deref() {
        cmd.arg("--model").arg(model);
    }
    cmd.arg("--cd").arg(&resolved_cwd);

    cmd.arg(prompt)
        .current_dir(&resolved_cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null());

    let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn codex CLI: {e}"))?;
    let stdout = child.stdout.take().ok_or("Failed to capture codex stdout")?;

    tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        let mut thread_id: Option<String> = None;
        let mut requested_model = requested_model;

        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }

            match serde_json::from_str::<CodexEvent>(&line) {
                Ok(CodexEvent::ThreadStarted { thread_id: tid }) => {
                    thread_id = Some(tid);
                }
                Ok(CodexEvent::ItemCompleted { item }) => {
                    let token = match item.item_type.as_str() {
                        "agent_message" => item.text.unwrap_or_default(),
                        "function_call" | "tool_call" => {
                            // Format as native tool call widget
                            let name = item.name.as_deref().unwrap_or("unknown");
                            let args = item.arguments.as_deref().unwrap_or("{}");
                            format!("\n<tool_call>{{\"name\": \"{}\", \"arguments\": {}}}</tool_call>\n", name, args)
                        }
                        "function_call_output" | "tool_result" => {
                            // Format as native tool response widget
                            let output = item.output.as_deref()
                                .or(item.text.as_deref())
                                .unwrap_or("");
                            let truncated = if output.len() > 500 { &output[..500] } else { output };
                            format!("\n<tool_response>\n{}\n</tool_response>\n", truncated)
                        }
                        _ => item.text.unwrap_or_default(),
                    };
                    if !token.is_empty() {
                        let _ = tx.send(CliTokenData {
                            token,
                            is_done: false,
                            session_id: None,
                            stop_reason: None,
                            cost_usd: None,
                            duration_ms: None,
                            model_id: requested_model.clone(),
                            input_tokens: None,
                            output_tokens: None,
                        });
                    }
                }
                Ok(CodexEvent::TurnCompleted { usage }) => {
                    let input_tokens = usage
                        .as_ref()
                        .and_then(|u| u.input_tokens)
                        .unwrap_or(0);
                    let cached_input_tokens = usage
                        .as_ref()
                        .and_then(|u| u.cached_input_tokens)
                        .unwrap_or(0);
                    let output_tokens = usage
                        .as_ref()
                        .and_then(|u| u.output_tokens)
                        .unwrap_or(0);

                    let _ = tx.send(CliTokenData {
                        token: String::new(),
                        is_done: true,
                        session_id: thread_id.clone(),
                        stop_reason: Some("turn_completed".to_string()),
                        cost_usd: None,
                        duration_ms: None,
                        model_id: requested_model.take(),
                        input_tokens: Some(input_tokens + cached_input_tokens),
                        output_tokens: Some(output_tokens),
                    });
                }
                Ok(CodexEvent::Unknown) => {}
                Err(e) => {
                    eprintln!("[CODEX] Failed to parse event: {e}");
                    eprintln!("[CODEX] Raw line: {}", &line[..line.len().min(200)]);
                }
            }
        }

        let _ = tx.send(CliTokenData {
            token: String::new(),
            is_done: true,
            session_id: thread_id,
            stop_reason: Some("cli_exit".to_string()),
            cost_usd: None,
            duration_ms: None,
            model_id: requested_model,
            input_tokens: None,
            output_tokens: None,
        });

        let _ = child.wait().await;
    });

    Ok(rx)
}
