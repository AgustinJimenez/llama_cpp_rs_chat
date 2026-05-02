//! Gemini CLI provider — spawns Google's `gemini` CLI using the user's existing auth.
//!
//! Install: npm install -g @google/gemini-cli
//! Protocol: text over stdout from `gemini --prompt "text"`

use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use super::{resolve_cli_cwd, CliTokenData};

pub async fn is_available() -> bool {
    crate::providers::gemini_bin().await.is_some()
}

pub async fn get_version() -> Option<String> {
    let bin = crate::providers::gemini_bin().await?;
    let mut cmd = Command::new(&bin);
    cmd.arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    crate::providers::hide_cli_window(&mut cmd);
    let output = cmd.output().await.ok()?;
    // gemini --version may write to stderr
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let v = if !stdout.is_empty() { stdout } else { stderr };
    if v.is_empty() { None } else { Some(v) }
}

pub async fn generate(
    prompt: &str,
    _model: Option<&str>,
    _max_turns: Option<u32>,
    cwd: Option<&str>,
    _session_id: Option<&str>,
) -> Result<mpsc::UnboundedReceiver<CliTokenData>, String> {
    let bin = crate::providers::gemini_bin()
        .await
        .ok_or("Gemini CLI not found")?;
    let resolved_cwd = resolve_cli_cwd(cwd)?;
    let (tx, rx) = mpsc::unbounded_channel();

    let mut cmd = Command::new(&bin);
    cmd.arg("--prompt").arg(prompt)
        .current_dir(&resolved_cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null());
    crate::providers::hide_cli_window(&mut cmd);

    let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn gemini: {e}"))?;
    let stdout = child.stdout.take().ok_or("No stdout")?;

    tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        let mut full_text = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            full_text.push_str(&line);
            full_text.push('\n');
            let _ = tx.send(CliTokenData {
                token: line + "\n",
                is_done: false,
                session_id: None,
                stop_reason: None,
                cost_usd: None,
                duration_ms: None,
                model_id: None,
                input_tokens: None,
                output_tokens: None,
            });
        }
        let _ = tx.send(CliTokenData {
            token: String::new(),
            is_done: true,
            session_id: None,
            stop_reason: Some("stop".to_string()),
            cost_usd: None,
            duration_ms: None,
            model_id: None,
            input_tokens: None,
            output_tokens: None,
        });
        let _ = child.wait().await;
    });

    Ok(rx)
}
