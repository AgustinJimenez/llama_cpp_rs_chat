use std::env;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use crate::utils::silent_command;
use super::shell_env::{get_shell_env, capture_env_from_command};
use super::parsing::{parse_command_with_quotes, find_last_redirect, split_on_chain_ops, extract_echo_content};

#[path = "execution/streaming.rs"]
mod streaming;
pub use streaming::{execute_command_streaming, execute_command_streaming_with_timeout};

#[path = "execution/pty.rs"]
mod pty;
pub use pty::execute_command_pty;

// ── Process tree kill (Windows) ─────────────────────────────────────────────
// On Windows, `child.kill()` only terminates the top-level process (cmd.exe).
// Child processes (e.g. php.exe spawned by cmd) inherit the stdout pipe handle,
// so the pipe stays open and `read()` blocks forever. `taskkill /T` terminates
// the entire process tree.

/// Kill a process and all its children. On Windows uses `taskkill /T /F`,
/// on other platforms falls back to regular kill (which covers process groups).
pub fn kill_process_tree(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        match silent_command("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .status()
        {
            Ok(status) if status.success() => {
                eprintln!("[KILL] Process tree killed (pid={pid})");
            }
            Ok(status) => {
                eprintln!("[KILL] taskkill exited with {status} for pid={pid}");
            }
            Err(e) => {
                eprintln!("[KILL] Failed to run taskkill for pid={pid}: {e}");
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Send SIGTERM to process group
        unsafe { libc::kill(-(pid as i32), libc::SIGTERM); }
    }
}

/// After executing a compound command, check if it started with `cd` and
/// persist the directory change to the process CWD. This way subsequent
/// tool calls use the new directory even though the `cd` ran in a subshell.
fn track_cwd_change(cmd: &str) {
    let trimmed = cmd.trim();

    // Check if the command starts with a cd
    let cd_target = if trimmed.starts_with("cd ") || trimmed.starts_with("cd\t") {
        let rest = &trimmed[3..];
        // Find where cd arguments end (&&, ||, ;, |, or end of string)
        let end = rest
            .find("&&")
            .or_else(|| rest.find("||"))
            .or_else(|| rest.find(';'))
            .or_else(|| rest.find('|'))
            .unwrap_or(rest.len());
        Some(rest[..end].trim())
    } else {
        None
    };

    if let Some(target) = cd_target {
        if target.is_empty() {
            return;
        }
        // Strip Windows cd flags like /d, /D before extracting the path
        let target = target.trim_matches('"').trim_matches('\'');
        let target = if target.starts_with("/d ") || target.starts_with("/D ") {
            target[3..].trim().trim_matches('"').trim_matches('\'')
        } else {
            target
        };
        match std::env::set_current_dir(target) {
            Ok(()) => {
                if let Ok(new_dir) = std::env::current_dir() {
                    eprintln!("[CWD] Persisted directory change to: {}", new_dir.display());
                }
            }
            Err(e) => {
                eprintln!("[CWD] Failed to persist cd to '{target}': {e}");
            }
        }
    }
}

/// Get CWD annotation string if CWD differs from a previously captured directory.
fn cwd_annotation(original_cwd: &std::path::Path) -> Option<String> {
    if let Ok(current) = std::env::current_dir() {
        if current != original_cwd {
            return Some(format!("\n[CWD: {}]", current.display()));
        }
    }
    None
}

/// Enrich PATH with common Windows tool directories.
pub fn enriched_windows_path() -> String {
    let current_path = env::var("PATH").unwrap_or_default();
    let extra_dirs = [
        r"C:\WINDOWS\system32",
        r"C:\WINDOWS",
        r"C:\WINDOWS\System32\Wbem",
        r"C:\WINDOWS\System32\WindowsPowerShell\v1.0",
        r"C:\Program Files\Git\cmd",
        r"C:\Program Files\nodejs",
        r"C:\ProgramData\chocolatey\bin",
        r"C:\php",
    ];
    extra_dirs
        .iter()
        .filter(|d| !current_path.contains(*d))
        .fold(current_path.clone(), |acc, d| format!("{acc};{d}"))
}

/// Execute a command on Windows.
/// Strategy: try direct execution first (avoids shell quoting issues for python, git, etc.).
/// Fall back to PowerShell for shell builtins (cat, dir, type) and commands with shell operators.
fn execute_windows(cmd: &str, parts: &[String]) -> std::io::Result<std::process::Output> {
    let path = enriched_windows_path();
    let persisted_env = get_shell_env();

    // Commands with shell operators (|, >, &&, etc.) must go through PowerShell
    if super::parsing::needs_shell(cmd) {
        let escaped = cmd.replace('$', "`$");
        let mut c = silent_command("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", &escaped])
            .env("PATH", &path);
        for (k, v) in &persisted_env {
            c.env(k, v);
        }
        return c.stdin(Stdio::null()).output();
    }

    // Try direct execution first — no shell means no quoting issues
    let mut c = silent_command(&parts[0]);
    c.args(&parts[1..]).env("PATH", &path);
    for (k, v) in &persisted_env {
        c.env(k, v);
    }
    let result = c.stdin(Stdio::null()).output();

    match &result {
        Ok(_) => result,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Command not found as executable — try PowerShell for aliases/builtins
            // (cat, dir, type, ls, etc. are PowerShell aliases, not real executables)
            let escaped = cmd.replace('$', "`$");
            let mut c = silent_command("powershell");
            c.args(["-NoProfile", "-NonInteractive", "-Command", &escaped])
                .env("PATH", &path);
            for (k, v) in &persisted_env {
                c.env(k, v);
            }
            c.stdin(Stdio::null()).output()
        }
        Err(_) => result,
    }
}

/// Intercept `echo "..." > file` patterns and write directly with std::fs::write.
/// This avoids shell variable expansion ($table becomes empty) and quoting issues.
/// Returns Some(result) if handled, None to fall through to sh -c.
fn try_native_echo_redirect(cmd: &str) -> Option<String> {
    let parts = split_on_chain_ops(cmd);
    let last_part = parts.last()?.trim();

    // The last segment must have a redirect
    let redirect_pos = find_last_redirect(last_part)?;

    // Split into echo part and file path
    let echo_part = last_part[..redirect_pos].trim();
    let file_path = last_part[redirect_pos + 1..].trim();

    // Must start with echo
    if !echo_part.starts_with("echo ") {
        return None;
    }

    // File path must not be empty
    if file_path.is_empty() {
        return None;
    }

    // Execute any preceding chained commands (mkdir -p, etc.) via shell
    if parts.len() > 1 {
        let prefix_cmds = &parts[..parts.len() - 1];
        for prefix in prefix_cmds {
            let output = silent_command("sh").arg("-c").arg(prefix).output();
            match output {
                Ok(o) if !o.status.success() => {
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    return Some(format!("Error: {stderr}"));
                }
                Err(e) => return Some(format!("Error: {e}")),
                _ => {}
            }
        }
    }

    // Extract echo content and write directly
    let content = extract_echo_content(echo_part)?;

    // Process \n escape sequences to real newlines
    let content = content.replace("\\n", "\n").replace("\\t", "\t");

    // Ensure parent directory exists
    if let Some(parent) = Path::new(file_path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    match std::fs::write(file_path, &content) {
        Ok(_) => Some(format!("Written {} bytes to {file_path}", content.len())),
        Err(e) => Some(format!("Error writing to {file_path}: {e}")),
    }
}

// Helper function to execute system commands
pub fn execute_command(cmd: &str) -> String {
    let trimmed = cmd.trim();

    // Parse command with proper quote handling
    let parts = parse_command_with_quotes(trimmed);
    if parts.is_empty() {
        return "Error: Empty command".to_string();
    }

    // If the command contains shell operators, delegate to sh/bash so they work.
    // This handles `cd /dir && npm init`, pipes, redirects, etc.
    let has_shell_ops = trimmed.contains("&&")
        || trimmed.contains("||")
        || trimmed.contains(" | ")
        || trimmed.contains(';')
        || trimmed.contains('>')
        || trimmed.contains('<');

    if has_shell_ops {
        // Try native echo redirect first (avoids shell $variable expansion)
        if !cfg!(target_os = "windows") {
            if let Some(result) = try_native_echo_redirect(trimmed) {
                return result;
            }
        }
        let original_cwd = std::env::current_dir().unwrap_or_default();
        let persisted_env = get_shell_env();
        #[cfg(target_os = "windows")]
        let output = {
            let mut c = silent_command("cmd");
            c.raw_arg(format!("/C {trimmed}"))
                .env("PATH", enriched_windows_path());
            for (k, v) in &persisted_env {
                c.env(k, v);
            }
            c.stdin(Stdio::null()).output()
        };
        #[cfg(not(target_os = "windows"))]
        let output = {
            let mut c = silent_command("sh");
            c.arg("-c").arg(trimmed);
            for (k, v) in &persisted_env {
                c.env(k, v);
            }
            c.stdin(Stdio::null()).output()
        };
        // Persist CWD if compound command started with cd
        track_cwd_change(trimmed);
        // Capture any env var assignments from the command
        capture_env_from_command(trimmed);
        return match output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let stderr = String::from_utf8_lossy(&o.stderr);
                let exit_code = o.status.code().unwrap_or(-1);
                let annotation = cwd_annotation(&original_cwd).unwrap_or_default();
                if !stderr.is_empty() && !o.status.success() {
                    format!("{stdout}\nError (exit code {exit_code}): {stderr}{annotation}")
                } else if stdout.is_empty() && stderr.is_empty() && o.status.success() {
                    format!("Command executed successfully (no output){annotation}")
                } else if stdout.is_empty() && stderr.is_empty() && !o.status.success() {
                    let cd_hint = if cmd.contains("&&") && cmd.trim_start().starts_with("cd ") {
                        " If using 'cd path && command', use the full path directly instead: e.g. 'python C:/full/path/script.py'."
                    } else {
                        ""
                    };
                    format!("Command failed with exit code {exit_code} and produced no output.{cd_hint}{annotation}")
                } else {
                    let combined = format!("{stdout}{stderr}");
                    if combined.trim().is_empty() {
                        if o.status.success() {
                            format!("Command executed successfully (no output){annotation}")
                        } else {
                            format!("Command failed with exit code {exit_code} (no output){annotation}")
                        }
                    } else {
                        format!("{combined}{annotation}")
                    }
                }
            }
            Err(e) => format!("Failed to execute command: {e}"),
        };
    }

    let command_name = &parts[0];

    // Special handling for cd command - actually change the process working directory
    if command_name == "cd" {
        let target_dir = if parts.len() > 1 {
            &parts[1]
        } else {
            return "Error: cd command requires a directory argument".to_string();
        };

        match env::set_current_dir(target_dir) {
            Ok(_) => {
                if let Ok(new_dir) = env::current_dir() {
                    format!("Successfully changed directory to: {}", new_dir.display())
                } else {
                    "Directory changed successfully".to_string()
                }
            }
            Err(e) => {
                format!("Error: Failed to change directory: {e}")
            }
        }
    } else {
        // Normal command execution for non-cd commands
        let is_windows = cfg!(target_os = "windows");

        // Capture any env var assignments (e.g. standalone `set VAR=value`)
        capture_env_from_command(trimmed);

        let output = if is_windows {
            execute_windows(cmd.trim(), &parts)
        } else {
            let mut c = silent_command(&parts[0]);
            c.args(&parts[1..]);
            for (k, v) in &get_shell_env() {
                c.env(k, v);
            }
            c.output()
        };

        match output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                // Handle commands that succeed silently
                if output.status.success() && stdout.is_empty() && stderr.is_empty() {
                    match command_name.as_str() {
                        "find" => "No files found matching the search criteria".to_string(),
                        "mkdir" => "Directory created successfully".to_string(),
                        "touch" => "File created successfully".to_string(),
                        "rm" | "rmdir" => "File/directory removed successfully".to_string(),
                        "mv" | "cp" => "File operation completed successfully".to_string(),
                        "chmod" => "Permissions changed successfully".to_string(),
                        _ => {
                            if parts.len() > 1 {
                                format!("Command '{}' executed successfully", parts.join(" "))
                            } else {
                                format!("Command '{command_name}' executed successfully")
                            }
                        }
                    }
                } else if !output.status.success() && stdout.is_empty() && stderr.is_empty() {
                    let exit_code = output.status.code().unwrap_or(-1);
                    format!("Command failed with exit code {exit_code} and produced no output.")
                } else if !stderr.is_empty() {
                    let exit_code = output.status.code().unwrap_or(0);
                    if output.status.success() {
                        format!("{stdout}{stderr}")
                    } else {
                        format!("{stdout}\nError (exit code {exit_code}): {stderr}")
                    }
                } else {
                    stdout.to_string()
                }
            }
            Err(e) => {
                format!("Failed to execute command: {e}")
            }
        }
    }
}

#[cfg(test)]
mod tests;
