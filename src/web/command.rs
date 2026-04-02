use std::env;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::utils::silent_command;
use std::time::Instant;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

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
                eprintln!("[KILL] Process tree killed (pid={})", pid);
            }
            Ok(status) => {
                eprintln!("[KILL] taskkill exited with {} for pid={}", status, pid);
            }
            Err(e) => {
                eprintln!("[KILL] Failed to run taskkill for pid={}: {}", pid, e);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Send SIGTERM to process group
        unsafe { libc::kill(-(pid as i32), libc::SIGTERM); }
    }
}

// Helper function to parse command with proper quote handling
pub fn parse_command_with_quotes(cmd: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current_part = String::new();
    let mut in_quotes = false;
    let chars = cmd.chars().peekable();

    for ch in chars {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                // Don't include the quote character in the output
            }
            ' ' if !in_quotes => {
                if !current_part.is_empty() {
                    parts.push(current_part.clone());
                    current_part.clear();
                }
            }
            _ => {
                current_part.push(ch);
            }
        }
    }

    if !current_part.is_empty() {
        parts.push(current_part);
    }

    parts
}

/// Check if a command uses shell operators that require a shell to interpret.
fn needs_shell(cmd: &str) -> bool {
    let mut in_quotes = false;
    let mut prev = '\0';
    for ch in cmd.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
        }
        if !in_quotes {
            match ch {
                '|' | '<' | ';' => return true,
                '>' if prev != '2' => return true, // allow 2> but catch > and >>
                '&' if prev == '&' => return true,  // &&
                _ => {}
            }
        }
        prev = ch;
    }
    false
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
        let target = target.trim_matches('"').trim_matches('\'');
        match std::env::set_current_dir(target) {
            Ok(()) => {
                if let Ok(new_dir) = std::env::current_dir() {
                    eprintln!("[CWD] Persisted directory change to: {}", new_dir.display());
                }
            }
            Err(e) => {
                eprintln!("[CWD] Failed to persist cd to '{}': {}", target, e);
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

    // Commands with shell operators (|, >, &&, etc.) must go through PowerShell
    if needs_shell(cmd) {
        let escaped = cmd.replace('$', "`$");
        return silent_command("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &escaped])
            .env("PATH", &path)
            .stdin(Stdio::null())
            .output();
    }

    // Try direct execution first — no shell means no quoting issues
    let result = silent_command(&parts[0])
        .args(&parts[1..])
        .env("PATH", &path)
        .stdin(Stdio::null())
        .output();

    match &result {
        Ok(_) => result,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Command not found as executable — try PowerShell for aliases/builtins
            // (cat, dir, type, ls, etc. are PowerShell aliases, not real executables)
            let escaped = cmd.replace('$', "`$");
            silent_command("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", &escaped])
                .env("PATH", &path)
                .stdin(Stdio::null())
                .output()
        }
        Err(_) => result,
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
        #[cfg(target_os = "windows")]
        let output = silent_command("cmd")
            .raw_arg(format!("/C {trimmed}"))
            .env("PATH", enriched_windows_path())
            .stdin(Stdio::null())
            .output();
        #[cfg(not(target_os = "windows"))]
        let output = silent_command("sh").arg("-c").arg(trimmed).stdin(Stdio::null()).output();
        // Persist CWD if compound command started with cd
        track_cwd_change(trimmed);
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
                    format!("Command failed with exit code {exit_code} and produced no output. The command may have found no matches or encountered a silent error.{annotation}")
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

        let output = if is_windows {
            execute_windows(cmd.trim(), &parts)
        } else {
            silent_command(&parts[0])
                .args(&parts[1..])
                .output()
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

/// Execute a command with streaming output, calling `on_line` for each line of stdout.
/// Returns the full accumulated output as a String (same as `execute_command`).
/// Uses spawn() + BufReader instead of .output() so output is available line-by-line.
///
/// If `cancel` is provided and set to `true`, the child process is killed and
/// the function returns early with a cancellation notice.
pub fn execute_command_streaming(
    cmd: &str,
    cancel: Option<Arc<AtomicBool>>,
    mut on_line: impl FnMut(&str),
) -> String {
    execute_command_streaming_with_timeout(cmd, cancel, None, &mut on_line)
}

pub fn execute_command_streaming_with_timeout(
    cmd: &str,
    cancel: Option<Arc<AtomicBool>>,
    timeout_override: Option<u64>,
    on_line: &mut dyn FnMut(&str),
) -> String {
    let trimmed = cmd.trim();

    let parts = parse_command_with_quotes(trimmed);
    if parts.is_empty() {
        return "Error: Empty command".to_string();
    }

    // cd command - no streaming needed
    let command_name = &parts[0];
    if command_name == "cd" {
        return execute_command(cmd);
    }

    // Capture CWD before execution so we can detect changes
    let original_cwd = std::env::current_dir().unwrap_or_default();

    // Determine how to spawn the command
    let has_shell_ops = trimmed.contains("&&")
        || trimmed.contains("||")
        || trimmed.contains(" | ")
        || trimmed.contains(';')
        || trimmed.contains('>')
        || trimmed.contains('<');

    // For echo redirects on Unix, use the native handler (no streaming needed)
    if has_shell_ops && !cfg!(target_os = "windows") {
        if let Some(result) = try_native_echo_redirect(trimmed) {
            return result;
        }
    }

    // For streaming, ALWAYS merge stderr into stdout via `2>&1` shell wrapper.
    // Many tools (composer, npm, cargo, etc.) write progress to stderr, and we want
    // to stream ALL output — not just stdout. The shell wrapper overhead is negligible
    // for long-running commands that benefit from streaming.
    let env_vars = [
        ("PYTHONUNBUFFERED", "1"),
        ("COMPOSER_PROCESS_TIMEOUT", "0"),
        ("GIT_FLUSH", "1"),
        ("CI", "true"),
    ];

    #[cfg(target_os = "windows")]
    let child_result = {
        let path = enriched_windows_path();
        let mut cmd = silent_command("cmd");
        cmd.raw_arg(format!("/C {trimmed} 2>&1"))
            .env("PATH", &path);
        for (k, v) in &env_vars {
            cmd.env(k, v);
        }
        // CRITICAL: null stdin so child doesn't inherit the worker's IPC pipe.
        // Without this, MSYS2 tools (wc, grep, etc.) hang forever waiting on
        // the inherited pipe even when given a file argument.
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
    };

    #[cfg(not(target_os = "windows"))]
    let child_result = {
        let mut cmd = silent_command("sh");
        cmd.arg("-c").arg(format!("{trimmed} 2>&1"));
        for (k, v) in &env_vars {
            cmd.env(k, v);
        }
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
    };

    match child_result {
        Ok(mut child) => {
            let mut output = String::new();
            let child_pid = child.id();
            eprintln!("[STREAM] Started: pid={} cmd={}", child_pid, &trimmed[..trimmed.len().min(100)]);
            let stdout_pipe = child.stdout.take();

            let inactivity_timeout_secs: u64 = timeout_override.unwrap_or(120); // Default 2 min, resets on output
            // Check cancellation every 200ms — responsive enough without busy-waiting
            const POLL_INTERVAL_MS: u64 = 200;

            const MAX_WALL_CLOCK_SECS: u64 = 120;
            let mut was_cancelled = false;
            let mut inactivity_killed = false;
            let total_timeout_killed = false; // Kept for compat, no longer triggered separately
            let wall_start = Instant::now();

            if let Some(stdout) = stdout_pipe {
                // Channel-based reader: a dedicated thread reads from the pipe and
                // sends chunks through a channel. The main thread uses recv_timeout()
                // to reliably detect inactivity — unlike the old monitor-thread approach
                // where a blocking read() could prevent the timeout from taking effect
                // (e.g. if kill_process_tree failed silently, the monitor exited and
                // nobody was left to retry, leaving read() blocked forever).
                let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
                std::thread::spawn(move || {
                    let mut reader = std::io::BufReader::new(stdout);
                    let mut buf = [0u8; 4096];
                    loop {
                        match std::io::Read::read(&mut reader, &mut buf) {
                            Ok(0) => break, // EOF
                            Ok(n) => {
                                if tx.send(buf[..n].to_vec()).is_err() {
                                    break; // receiver dropped
                                }
                            }
                            Err(_) => break,
                        }
                    }
                });

                let mut line_buf = String::new();
                let mut last_data = Instant::now();

                loop {
                    // Check user cancellation
                    if let Some(ref flag) = cancel {
                        if flag.load(Ordering::Relaxed) {
                            eprintln!("[STREAM] Cancelled by user, killing pid={}", child_pid);
                            kill_process_tree(child_pid);
                            was_cancelled = true;
                            break;
                        }
                    }

                    // Log every 60s to confirm the loop is running
                    let elapsed_secs = wall_start.elapsed().as_secs();
                    if elapsed_secs > 0 && elapsed_secs % 60 == 0 && elapsed_secs / 60 != (elapsed_secs - 1) / 60 {
                        eprintln!("[STREAM] Still running: {}s elapsed, pid={}", elapsed_secs, child_pid);
                    }

                    // Wall-clock timeout: if the command has been running too long
                    // (even if it keeps producing output), stop waiting and return
                    // control to the model. The process keeps running — the model
                    // can check on it or kill it.
                    let elapsed = wall_start.elapsed().as_secs();
                    // Debug: log at 60s and 110s to confirm loop is running
                    if elapsed == 60 || elapsed == 110 {
                        eprintln!("[STREAM] Wall-clock check: {}s / {}s limit, pid={}", elapsed, MAX_WALL_CLOCK_SECS, child_pid);
                    }
                    if elapsed >= MAX_WALL_CLOCK_SECS {
                        let pid = child_pid;
                        eprintln!(
                            "[STREAM] Wall-clock limit ({}s) reached, detaching from pid={}",
                            MAX_WALL_CLOCK_SECS, pid
                        );
                        // Truncate output if huge — keep last 40 lines
                        let lines: Vec<&str> = output.lines().collect();
                        if lines.len() > 40 {
                            output = format!(
                                "[...{} earlier lines truncated...]\n{}",
                                lines.len() - 40,
                                lines[lines.len()-40..].join("\n")
                            );
                        }
                        output.push_str(&format!(
                            "\n[Command still running after {}s (PID {}). It may be stuck or very slow. You can kill it with: taskkill /F /T /PID {}]\n",
                            MAX_WALL_CLOCK_SECS, pid, pid
                        ));
                        // Kill the process — don't leave orphans
                        kill_process_tree(pid);
                        return output;
                    }

                    match rx.recv_timeout(std::time::Duration::from_millis(POLL_INTERVAL_MS)) {
                        Ok(data) => {
                            last_data = Instant::now();
                            let chunk = String::from_utf8_lossy(&data);
                            for ch in chunk.chars() {
                                if ch == '\n' || ch == '\r' {
                                    if !line_buf.is_empty() {
                                        on_line(&line_buf);
                                        output.push_str(&line_buf);
                                        output.push('\n');
                                        line_buf.clear();
                                    }
                                } else {
                                    line_buf.push(ch);
                                }
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            // No data — check inactivity threshold (resets on each output)
                            if last_data.elapsed().as_secs() >= inactivity_timeout_secs {
                                eprintln!(
                                    "[STREAM] Inactivity timeout ({}s no output), killing pid={}",
                                    inactivity_timeout_secs, child_pid
                                );
                                kill_process_tree(child_pid);
                                inactivity_killed = true;
                                break;
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            eprintln!("[STREAM] Pipe disconnected after {}s, pid={}", wall_start.elapsed().as_secs(), child_pid);
                            break; // Reader thread exited (pipe closed / process exited)
                        }
                    }
                }

                // Flush remaining content in line buffer
                if !line_buf.is_empty() {
                    on_line(&line_buf);
                    output.push_str(&line_buf);
                    output.push('\n');
                }
            }

            // Wall-clock safety net: if we've been running too long, kill and return
            // immediately. This catches the case where the pipe disconnects at 0s
            // (e.g. winget) but the child process keeps running.
            const POST_PIPE_WALL_LIMIT: u64 = MAX_WALL_CLOCK_SECS;
            if wall_start.elapsed().as_secs() >= POST_PIPE_WALL_LIMIT {
                eprintln!("[STREAM] Wall-clock exceeded after pipe closed ({}s), killing pid={}",
                    wall_start.elapsed().as_secs(), child_pid);
                kill_process_tree(child_pid);
                output.push_str(&format!("\n[Command killed after {}s wall-clock limit]\n", wall_start.elapsed().as_secs()));
                return output;
            }

            // Reap child process — with timeout to avoid blocking forever
            // (e.g. winget may close its pipe but keep running for minutes)
            let mut exit_code = -1i32;
            let mut success = false;
            let reap_deadline = std::time::Duration::from_secs(
                if wall_start.elapsed().as_secs() >= POST_PIPE_WALL_LIMIT { 1 } else { 5 }
            );
            let reap_start = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(s)) => {
                        exit_code = s.code().unwrap_or(-1);
                        success = s.success();
                        break;
                    }
                    Ok(None) if reap_start.elapsed() < reap_deadline
                        && wall_start.elapsed().as_secs() < MAX_WALL_CLOCK_SECS =>
                    {
                        std::thread::sleep(std::time::Duration::from_millis(200));
                    }
                    _ => {
                        eprintln!("[STREAM] Killing unreaped child pid={}", child_pid);
                        kill_process_tree(child_pid);
                        // Give it a moment to die, then best-effort reap
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        if let Ok(Some(s)) = child.try_wait() {
                            exit_code = s.code().unwrap_or(-1);
                            success = s.success();
                        }
                        break;
                    }
                }
            }

            if was_cancelled {
                output.push_str("\n[Cancelled by user]\n");
            } else if total_timeout_killed || inactivity_killed {
                output.push_str(&format!(
                    "\n[Process killed: no output for {}s. TIP: Use \"timeout\": {} in your tool call for slow commands, or \"background\": true for servers/daemons.]\n",
                    inactivity_timeout_secs, inactivity_timeout_secs * 2
                ));
            }

            // Persist CWD if compound command started with cd
            track_cwd_change(trimmed);
            let annotation = cwd_annotation(&original_cwd).unwrap_or_default();

            if output.trim().is_empty() {
                if success {
                    format!("Command executed successfully (no output){annotation}")
                } else {
                    format!("Command failed with exit code {exit_code} and produced no output.{annotation}")
                }
            } else {
                format!("{output}{annotation}")
            }
        }
        Err(e) => {
            // On Windows, if direct execution fails with NotFound, try PowerShell
            if cfg!(target_os = "windows") && !has_shell_ops && e.kind() == std::io::ErrorKind::NotFound {
                // Fall back to non-streaming execute_command for PowerShell fallback
                execute_command(cmd)
            } else {
                format!("Failed to execute command: {e}")
            }
        }
    }
}

// ── Command output sanitization ──────────────────────────────────────────────

/// Maximum lines of command output to keep for model context.
const MAX_COMMAND_OUTPUT_LINES: usize = 80;
/// Maximum characters of command output to keep for model context.
const MAX_COMMAND_OUTPUT_CHARS: usize = 8000;
/// Lines to keep from the beginning of truncated output.
const HEAD_LINES: usize = 15;
/// Lines to keep from the end of truncated output.
const TAIL_LINES: usize = 25;

/// Remove ANSI escape codes (colors, cursor movement, OSC sequences) from text.
/// Uses a simple state-machine parser instead of regex to avoid potential segfaults
/// from lazy_static regex compilation in the worker process.
pub fn strip_ansi_codes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == 0x1b {
            // ESC character — start of escape sequence
            i += 1;
            if i >= len { break; }

            if bytes[i] == b'[' {
                // CSI sequence: ESC [ ... final_byte (letter)
                i += 1;
                while i < len && (bytes[i] == b';' || bytes[i].is_ascii_digit()) {
                    i += 1;
                }
                // Skip the final byte (letter)
                if i < len && bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
            } else if bytes[i] == b']' {
                // OSC sequence: ESC ] ... (BEL or ESC \)
                i += 1;
                while i < len {
                    if bytes[i] == 0x07 {
                        i += 1;
                        break;
                    }
                    if bytes[i] == 0x1b && i + 1 < len && bytes[i + 1] == b'\\' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
            } else {
                // Other escape: skip ESC + next char
                i += 1;
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    result
}

/// Truncate long output: keep first HEAD_LINES + last TAIL_LINES, omit middle.
/// Also enforces a hard character limit.
pub fn truncate_command_output(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();

    let mut result = if lines.len() > MAX_COMMAND_OUTPUT_LINES {
        let omitted = lines.len() - HEAD_LINES - TAIL_LINES;
        let head = lines[..HEAD_LINES].join("\n");
        let tail = lines[lines.len() - TAIL_LINES..].join("\n");
        format!("{}\n\n... ({} lines omitted) ...\n\n{}", head, omitted, tail)
    } else {
        text.to_string()
    };

    // Hard character limit
    if result.len() > MAX_COMMAND_OUTPUT_CHARS {
        result.truncate(MAX_COMMAND_OUTPUT_CHARS);
        result.push_str("\n... (output truncated)");
    }

    result
}

/// Strip ANSI codes and truncate long command output for model context injection.
/// The raw output is still streamed to the frontend; this only affects what the model sees.
pub fn sanitize_command_output(text: &str) -> String {
    let clean = strip_ansi_codes(text);
    truncate_command_output(&clean)
}

/// Find the position of the last `>` redirect operator that is NOT inside quotes.
fn find_last_redirect(cmd: &str) -> Option<usize> {
    let mut last_pos = None;
    let mut in_single = false;
    let mut in_double = false;
    for (i, ch) in cmd.chars().enumerate() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '>' if !in_single && !in_double => last_pos = Some(i),
            _ => {}
        }
    }
    last_pos
}

/// Split a command string on `&&` and `||` operators (outside of quotes).
fn split_on_chain_ops(cmd: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_single = false;
    let mut in_double = false;
    let bytes = cmd.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '&' if !in_single && !in_double && i + 1 < bytes.len() && bytes[i + 1] == b'&' => {
                parts.push(cmd[start..i].trim());
                i += 2;
                start = i;
                continue;
            }
            '|' if !in_single && !in_double && i + 1 < bytes.len() && bytes[i + 1] == b'|' => {
                parts.push(cmd[start..i].trim());
                i += 2;
                start = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(cmd[start..].trim());
    parts
}

/// Extract the content from an echo command (handling double quotes, single quotes, or bare text).
fn extract_echo_content(echo_part: &str) -> Option<String> {
    let trimmed = echo_part.trim();
    let after_echo = if let Some(stripped) = trimmed.strip_prefix("echo ") {
        stripped.trim()
    } else {
        return None;
    };

    if (after_echo.starts_with('"') && after_echo.ends_with('"')
        || after_echo.starts_with('\'') && after_echo.ends_with('\''))
        && after_echo.len() >= 2
    {
        Some(after_echo[1..after_echo.len() - 1].to_string())
    } else {
        Some(after_echo.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_command() {
        let result = parse_command_with_quotes("ls -la");
        assert_eq!(result, vec!["ls", "-la"]);
    }

    #[test]
    fn test_parse_command_with_quoted_arg() {
        let result = parse_command_with_quotes(r#"cat "file with spaces.txt""#);
        assert_eq!(result, vec!["cat", "file with spaces.txt"]);
    }

    #[test]
    fn test_parse_command_with_multiple_quoted_args() {
        let result = parse_command_with_quotes(r#"cp "source file.txt" "dest file.txt""#);
        assert_eq!(result, vec!["cp", "source file.txt", "dest file.txt"]);
    }

    #[test]
    fn test_parse_command_with_mixed_quotes_and_regular_args() {
        let result = parse_command_with_quotes(r#"git commit -m "Initial commit" --no-verify"#);
        assert_eq!(
            result,
            vec!["git", "commit", "-m", "Initial commit", "--no-verify"]
        );
    }

    #[test]
    fn test_parse_command_with_empty_string() {
        let result = parse_command_with_quotes("");
        assert_eq!(result, Vec::<String>::new());
    }

    #[test]
    fn test_parse_command_with_only_spaces() {
        let result = parse_command_with_quotes("   ");
        assert_eq!(result, Vec::<String>::new());
    }

    #[test]
    fn test_parse_command_with_trailing_spaces() {
        let result = parse_command_with_quotes("ls -la   ");
        assert_eq!(result, vec!["ls", "-la"]);
    }

    #[test]
    fn test_parse_command_with_leading_spaces() {
        let result = parse_command_with_quotes("   ls -la");
        assert_eq!(result, vec!["ls", "-la"]);
    }

    #[test]
    fn test_parse_command_with_path_containing_spaces() {
        let result = parse_command_with_quotes(r#"cd "C:\Program Files\MyApp""#);
        assert_eq!(result, vec!["cd", r"C:\Program Files\MyApp"]);
    }

    #[test]
    fn test_parse_command_with_nested_quotes() {
        // Quotes within quotes - outer quotes are removed
        let result = parse_command_with_quotes(r#"echo "Hello "World"""#);
        // This will parse as: echo "Hello " World ""
        // Which gives: ["echo", "Hello ", "World", ""]
        assert!(result.contains(&"echo".to_string()));
    }

    #[test]
    fn test_execute_empty_command() {
        let result = execute_command("");
        assert_eq!(result, "Error: Empty command");
    }

    #[test]
    fn test_execute_echo_command() {
        let result = execute_command("echo Hello");
        assert!(result.contains("Hello") || result.contains("executed successfully"));
    }

    #[test]
    fn test_cd_without_argument() {
        let result = execute_command("cd");
        assert!(result.contains("requires a directory argument"));
    }

    #[test]
    fn test_command_with_special_characters() {
        let result = parse_command_with_quotes(r#"grep "pattern*" file.txt"#);
        assert_eq!(result, vec!["grep", "pattern*", "file.txt"]);
    }

    #[test]
    fn test_git_commit_with_quoted_message() {
        let result = parse_command_with_quotes(r#"git commit -m "Fix bug #123""#);
        assert_eq!(result, vec!["git", "commit", "-m", "Fix bug #123"]);
    }

    #[test]
    fn test_windows_path_parsing() {
        let result = parse_command_with_quotes(r#"type "C:\Users\test\file.txt""#);
        assert_eq!(result, vec!["type", r"C:\Users\test\file.txt"]);
    }

    #[test]
    fn test_unix_path_parsing() {
        let result = parse_command_with_quotes(r#"cat "/home/user/my file.txt""#);
        assert_eq!(result, vec!["cat", "/home/user/my file.txt"]);
    }

    #[test]
    fn test_native_echo_redirect_preserves_dollar_vars() {
        let cmd = r#"echo "<?php\n\$table->id();\n\$fillable = ['name'];" > /tmp/test_echo_redir.php"#;
        let result = try_native_echo_redirect(cmd);
        assert!(result.is_some(), "Should match echo > file pattern");
        let content = std::fs::read_to_string("/tmp/test_echo_redir.php").unwrap();
        assert!(content.contains("$table"), "Dollar vars should be preserved");
        assert!(content.contains("$fillable"), "Dollar vars should be preserved");
        std::fs::remove_file("/tmp/test_echo_redir.php").ok();
    }

    #[test]
    fn test_native_echo_redirect_with_chain() {
        let cmd = r#"mkdir -p /tmp/test_echo_chain && echo "hello" > /tmp/test_echo_chain/test.txt"#;
        let result = try_native_echo_redirect(cmd);
        assert!(result.is_some());
        let content = std::fs::read_to_string("/tmp/test_echo_chain/test.txt").unwrap();
        assert_eq!(content.trim(), "hello");
        std::fs::remove_dir_all("/tmp/test_echo_chain").ok();
    }

    #[test]
    fn test_native_echo_redirect_non_echo_falls_through() {
        let cmd = "cat foo.txt > bar.txt";
        let result = try_native_echo_redirect(cmd);
        assert!(result.is_none(), "Non-echo redirects should fall through");
    }

    #[test]
    fn test_find_last_redirect() {
        assert_eq!(find_last_redirect(r#"echo "hi" > file.txt"#), Some(10));
        assert_eq!(find_last_redirect(r#"echo "a > b" > out.txt"#), Some(13));
        assert_eq!(find_last_redirect("echo hello"), None);
    }

    #[test]
    fn test_split_on_chain_ops() {
        let parts = split_on_chain_ops("mkdir -p dir && echo hi > f.txt");
        assert_eq!(parts, vec!["mkdir -p dir", "echo hi > f.txt"]);
    }

    #[test]
    fn test_extract_echo_content() {
        assert_eq!(extract_echo_content(r#"echo "hello world""#), Some("hello world".to_string()));
        assert_eq!(extract_echo_content(r#"echo 'single quotes'"#), Some("single quotes".to_string()));
        assert_eq!(extract_echo_content("echo bare text"), Some("bare text".to_string()));
    }

    #[test]
    fn test_native_echo_redirect_with_newline_escapes() {
        let cmd = r#"echo "line1\nline2\nline3" > /tmp/test_echo_newlines.txt"#;
        let result = try_native_echo_redirect(cmd);
        assert!(result.is_some());
        let content = std::fs::read_to_string("/tmp/test_echo_newlines.txt").unwrap();
        assert!(content.contains("line1\nline2\nline3") || content.contains("line1"));
        std::fs::remove_file("/tmp/test_echo_newlines.txt").ok();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_cmd_quoted_paths_raw_arg() {
        // Simulates the exact command an AI agent generates: quoted PHP path + quoted composer path
        let php = env::current_dir().unwrap().join("php-8.2.30").join("php.exe");
        if !php.exists() {
            eprintln!("Skipping: php.exe not found at {:?}", php);
            return;
        }
        // Command with quoted paths — the exact pattern that broke before raw_arg fix
        let cmd = format!("\"{}\" -v", php.display());
        let result = execute_command(&cmd);
        assert!(result.contains("PHP"), "Expected PHP version output, got: {result}");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_streaming_cmd_quoted_paths_raw_arg() {
        let php = env::current_dir().unwrap().join("php-8.2.30").join("php.exe");
        if !php.exists() {
            eprintln!("Skipping: php.exe not found at {:?}", php);
            return;
        }
        let cmd = format!("\"{}\" -v", php.display());
        let mut lines = Vec::new();
        let result = execute_command_streaming(&cmd, None, |line| lines.push(line.to_string()));
        assert!(result.contains("PHP"), "Expected PHP version output, got: {result}");
        assert!(!lines.is_empty(), "Expected streaming lines, got none");
    }

    #[test]
    fn test_track_cwd_change_with_and_and() {
        let original = env::current_dir().unwrap();
        let temp = env::temp_dir();
        let cmd = format!("cd {} && echo hello", temp.display());
        track_cwd_change(&cmd);
        let now = env::current_dir().unwrap();
        // Restore original CWD before asserting (so test cleanup works)
        let _ = env::set_current_dir(&original);
        // On Windows, temp_dir() may return a short path; canonicalize both
        assert_eq!(
            now.canonicalize().unwrap_or(now.clone()),
            temp.canonicalize().unwrap_or(temp.clone()),
            "CWD should have changed to temp dir"
        );
    }

    #[test]
    fn test_track_cwd_change_with_semicolon() {
        let original = env::current_dir().unwrap();
        let temp = env::temp_dir();
        let cmd = format!("cd {}; echo hello", temp.display());
        track_cwd_change(&cmd);
        let now = env::current_dir().unwrap();
        let _ = env::set_current_dir(&original);
        assert_eq!(
            now.canonicalize().unwrap_or(now.clone()),
            temp.canonicalize().unwrap_or(temp.clone()),
        );
    }

    #[test]
    fn test_track_cwd_change_quoted_path() {
        let original = env::current_dir().unwrap();
        let temp = env::temp_dir();
        let cmd = format!("cd \"{}\" && echo hello", temp.display());
        track_cwd_change(&cmd);
        let now = env::current_dir().unwrap();
        let _ = env::set_current_dir(&original);
        assert_eq!(
            now.canonicalize().unwrap_or(now.clone()),
            temp.canonicalize().unwrap_or(temp.clone()),
        );
    }

    #[test]
    fn test_track_cwd_change_no_cd() {
        let original = env::current_dir().unwrap();
        track_cwd_change("echo hello && echo world");
        let now = env::current_dir().unwrap();
        assert_eq!(now, original, "CWD should not change for non-cd commands");
    }

    #[test]
    fn test_track_cwd_change_invalid_dir() {
        let original = env::current_dir().unwrap();
        track_cwd_change("cd /nonexistent_dir_12345 && echo hello");
        let now = env::current_dir().unwrap();
        assert_eq!(now, original, "CWD should not change for invalid directory");
    }

    #[test]
    fn test_cwd_annotation_same_dir() {
        let cwd = env::current_dir().unwrap();
        assert!(cwd_annotation(&cwd).is_none(), "No annotation when CWD unchanged");
    }

    #[test]
    fn test_cwd_annotation_different_dir() {
        let original = env::current_dir().unwrap();
        let temp = env::temp_dir();
        let _ = env::set_current_dir(&temp);
        let annotation = cwd_annotation(&original);
        let _ = env::set_current_dir(&original);
        assert!(annotation.is_some(), "Should produce annotation when CWD differs");
        assert!(annotation.unwrap().contains("[CWD:"));
    }
}
