//! Agent Browser backend — subprocess integration with agent-browser CLI.
//!
//! Uses the daemon-based `agent-browser` tool: `open <url>` then `get text body`
//! or `get html body`. The daemon persists between commands for fast page loads.

use std::process::{Command, Stdio};

/// Try to find the agent-browser binary.
fn find_binary() -> Option<String> {
    // 1. Explicit env var
    if let Ok(path) = std::env::var("AGENT_BROWSER_PATH") {
        return Some(path);
    }

    // 2. Check PATH via `where` (Windows) or `which` (Unix)
    #[cfg(target_os = "windows")]
    let check = Command::new("where").arg("agent-browser").output();
    #[cfg(not(target_os = "windows"))]
    let check = Command::new("which").arg("agent-browser").output();

    if let Ok(output) = check {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout)
                .lines().next().unwrap_or("").trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }

    // 3. Check node_modules/.bin/
    let local = if cfg!(target_os = "windows") {
        "node_modules/.bin/agent-browser.cmd"
    } else {
        "node_modules/.bin/agent-browser"
    };
    if std::path::Path::new(local).exists() {
        return Some(local.to_string());
    }

    None
}

/// Run an agent-browser command and return stdout.
fn run_cmd(binary: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(binary)
        .args(args)
        .stdin(Stdio::null()) // CRITICAL: prevent IPC pipe inheritance
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to run agent-browser: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Non-fatal warnings (e.g. "--native ignored") — check if stdout has content
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        if !stdout.trim().is_empty() {
            return Ok(stdout);
        }
        return Err(format!("agent-browser failed (exit {}): {}", output.status, stderr.trim()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Fetch page as plain text via agent-browser CLI.
pub fn fetch_text(url: &str, max_chars: usize) -> Result<String, String> {
    let binary = find_binary().ok_or_else(|| {
        "agent-browser not found. Install with: npm install -g agent-browser".to_string()
    })?;

    // Step 1: Navigate to URL
    run_cmd(&binary, &["open", url]).map_err(|e| format!("agent-browser open failed: {e}"))?;

    // Step 2: Get text content
    let text = run_cmd(&binary, &["get", "text", "body"])
        .map_err(|e| format!("agent-browser get text failed: {e}"))?;

    let text = text.trim().to_string();

    if text.len() > max_chars {
        let mut t = max_chars;
        while t > 0 && !text.is_char_boundary(t) { t -= 1; }
        Ok(format!("{}...\n[Truncated: first {} of {} chars]", &text[..t], t, text.len()))
    } else {
        Ok(text)
    }
}

/// Fetch raw HTML via agent-browser CLI.
pub fn fetch_html(url: &str) -> Result<String, String> {
    let binary = find_binary().ok_or_else(|| {
        "agent-browser not found. Install with: npm install -g agent-browser".to_string()
    })?;

    // Step 1: Navigate to URL
    run_cmd(&binary, &["open", url]).map_err(|e| format!("agent-browser open failed: {e}"))?;

    // Step 2: Get HTML content
    run_cmd(&binary, &["get", "html", "body"])
        .map_err(|e| format!("agent-browser get html failed: {e}"))
}
