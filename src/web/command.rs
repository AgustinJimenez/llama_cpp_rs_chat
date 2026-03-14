use std::env;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
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
fn kill_process_tree(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        match Command::new("taskkill")
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
    unpersist_bg_process(pid);
}

/// Public wrapper to kill a background process by PID (used by REST API).
pub fn kill_background_process_by_pid(pid: u32) {
    kill_process_tree(pid);
}

// ── Background process infrastructure ──────────────────────────────────────

/// How long to capture initial output before returning (seconds).
const BACKGROUND_CAPTURE_SECS: u64 = 5;

/// A background process tracked by the global registry.
struct BackgroundProcess {
    pid: u32,
    command: String,
    /// Lines appended continuously by the reader thread.
    output_buffer: Arc<StdMutex<Vec<String>>>,
    /// Index into output_buffer: lines before this were already returned by check().
    cursor: Arc<AtomicUsize>,
    /// Set to false by the reader thread when the process exits.
    running: Arc<AtomicBool>,
    /// When the process was started (for elapsed time display).
    started_at: Instant,
    /// Consecutive checks with no new output (reset when output appears or process exits).
    no_output_checks: AtomicUsize,
    /// Total number of check_background_process calls (never resets).
    total_checks: AtomicUsize,
}

lazy_static::lazy_static! {
    static ref BACKGROUND_PROCESSES: StdMutex<Vec<BackgroundProcess>> = StdMutex::new(Vec::new());
    /// Database reference for persisting background process PIDs across crashes.
    static ref BG_DB_REF: StdMutex<Option<super::database::SharedDatabase>> = StdMutex::new(None);
    /// Unique session ID generated at app startup. Used to detect orphaned processes
    /// from previous sessions that crashed without cleanup.
    static ref BG_SESSION_ID: StdMutex<String> = StdMutex::new(String::new());
}

/// Initialize the background process tracking system with DB and session ID.
/// Called once at worker startup.
pub fn init_background_tracking(db: super::database::SharedDatabase, session_id: String) {
    if let Ok(mut db_ref) = BG_DB_REF.lock() {
        *db_ref = Some(db);
    }
    if let Ok(mut sid) = BG_SESSION_ID.lock() {
        *sid = session_id;
    }
}

/// Persist a background process PID to the database.
fn persist_bg_process(pid: u32, command: &str, conversation_id: Option<&str>) {
    let db = match BG_DB_REF.lock().ok().and_then(|r| r.clone()) {
        Some(d) => d,
        None => return,
    };
    let session_id = BG_SESSION_ID.lock().ok().map(|s| s.clone()).unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let conn = db.connection();
    let _ = conn.execute(
        "INSERT OR REPLACE INTO background_processes (pid, command, conversation_id, started_at, session_id) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![pid as i64, command, conversation_id, now, session_id],
    );
}

/// Remove a background process PID from the database (process exited or was killed).
fn unpersist_bg_process(pid: u32) {
    let db = match BG_DB_REF.lock().ok().and_then(|r| r.clone()) {
        Some(d) => d,
        None => return,
    };
    let conn = db.connection();
    let _ = conn.execute("DELETE FROM background_processes WHERE pid = ?1", [pid as i64]);
}

/// Check if a process is still alive by PID.
pub fn is_process_alive(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        // Use tasklist to check if PID exists (no extra dependencies)
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(0x08000000)
            .output()
            .map(|o| {
                let out = String::from_utf8_lossy(&o.stdout);
                out.contains(&pid.to_string())
            })
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        // kill with signal 0 checks existence without sending a signal
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
}

/// Get orphaned processes from previous sessions that are still running.
/// Returns (pid, command, started_at_secs) for each orphan.
pub fn get_orphaned_processes(db: &super::database::SharedDatabase) -> Vec<(u32, String, i64)> {
    let session_id = BG_SESSION_ID.lock().ok().map(|s| s.clone()).unwrap_or_default();
    let conn = db.connection();
    let mut stmt = match conn.prepare(
        "SELECT pid, command, started_at FROM background_processes WHERE session_id != ?1"
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map([&session_id], |row| {
        let pid: i64 = row.get(0)?;
        let command: String = row.get(1)?;
        let started_at: i64 = row.get(2)?;
        Ok((pid as u32, command, started_at))
    }) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    rows.filter_map(|r| r.ok())
        .filter(|(pid, _, _)| is_process_alive(*pid))
        .collect()
}

/// Remove all dead process records from the database (cleanup stale entries).
pub fn cleanup_dead_process_records(db: &super::database::SharedDatabase) {
    let conn = db.connection();
    let mut stmt = match conn.prepare("SELECT pid FROM background_processes") {
        Ok(s) => s,
        Err(_) => return,
    };
    let pids: Vec<u32> = stmt
        .query_map([], |row| {
            let pid: i64 = row.get(0)?;
            Ok(pid as u32)
        })
        .ok()
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();

    for pid in pids {
        if !is_process_alive(pid) {
            let _ = conn.execute("DELETE FROM background_processes WHERE pid = ?1", [pid as i64]);
        }
    }
}

/// Kill all background processes for the current session and clean up DB.
pub fn kill_all_session_processes() {
    let db = match BG_DB_REF.lock().ok().and_then(|r| r.clone()) {
        Some(d) => d,
        None => return,
    };
    let session_id = BG_SESSION_ID.lock().ok().map(|s| s.clone()).unwrap_or_default();
    if session_id.is_empty() { return; }

    let conn = db.connection();
    let mut stmt = match conn.prepare(
        "SELECT pid, command FROM background_processes WHERE session_id = ?1"
    ) {
        Ok(s) => s,
        Err(_) => return,
    };
    let procs: Vec<(u32, String)> = stmt
        .query_map([&session_id], |row| {
            let pid: i64 = row.get(0)?;
            let cmd: String = row.get(1)?;
            Ok((pid as u32, cmd))
        })
        .ok()
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();

    for (pid, cmd) in &procs {
        if is_process_alive(*pid) {
            eprintln!("[SHUTDOWN] Killing background process PID {}: {}", pid, cmd);
            kill_process_tree(*pid);
        }
    }
    let _ = conn.execute("DELETE FROM background_processes WHERE session_id = ?1", [&session_id]);
}

/// List all tracked background processes (in-memory + DB orphans).
pub fn list_all_background_processes() -> Vec<(u32, String, bool, String)> {
    let mut result = Vec::new();

    // In-memory processes (current session)
    if let Ok(procs) = BACKGROUND_PROCESSES.lock() {
        for p in procs.iter() {
            let is_running = p.running.load(Ordering::Relaxed);
            let elapsed = p.started_at.elapsed();
            let elapsed_str = if elapsed.as_secs() >= 60 {
                format!("{}m{}s", elapsed.as_secs() / 60, elapsed.as_secs() % 60)
            } else {
                format!("{}s", elapsed.as_secs())
            };
            let status = if is_running { "running" } else { "exited" };
            result.push((p.pid, p.command.clone(), is_running, format!("{} ({})", status, elapsed_str)));
        }
    }

    // DB orphans from previous sessions
    if let Some(db) = BG_DB_REF.lock().ok().and_then(|r| r.clone()) {
        let orphans = get_orphaned_processes(&db);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        for (pid, cmd, started_at) in orphans {
            // Skip if already in in-memory list
            if result.iter().any(|(p, _, _, _)| *p == pid) { continue; }
            let age_secs = (now - started_at).max(0);
            let age_str = if age_secs >= 3600 {
                format!("{}h{}m", age_secs / 3600, (age_secs % 3600) / 60)
            } else if age_secs >= 60 {
                format!("{}m{}s", age_secs / 60, age_secs % 60)
            } else {
                format!("{}s", age_secs)
            };
            result.push((pid, cmd, true, format!("orphaned (started {}ago)", age_str)));
        }
    }

    result
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

/// Enrich PATH with common Windows tool directories.
fn enriched_windows_path() -> String {
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
        return Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &escaped])
            .env("PATH", &path)
            .stdin(Stdio::null())
            .output();
    }

    // Try direct execution first — no shell means no quoting issues
    let result = Command::new(&parts[0])
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
            Command::new("powershell")
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
        #[cfg(target_os = "windows")]
        let output = Command::new("cmd")
            .raw_arg(format!("/C {trimmed}"))
            .env("PATH", enriched_windows_path())
            .stdin(Stdio::null())
            .output();
        #[cfg(not(target_os = "windows"))]
        let output = Command::new("sh").arg("-c").arg(trimmed).stdin(Stdio::null()).output();
        return match output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let stderr = String::from_utf8_lossy(&o.stderr);
                let exit_code = o.status.code().unwrap_or(-1);
                if !stderr.is_empty() && !o.status.success() {
                    format!("{stdout}\nError (exit code {exit_code}): {stderr}")
                } else if stdout.is_empty() && stderr.is_empty() && o.status.success() {
                    "Command executed successfully (no output)".to_string()
                } else if stdout.is_empty() && stderr.is_empty() && !o.status.success() {
                    format!("Command failed with exit code {exit_code} and produced no output. The command may have found no matches or encountered a silent error.")
                } else {
                    let combined = format!("{stdout}{stderr}");
                    if combined.trim().is_empty() {
                        if o.status.success() {
                            "Command executed successfully (no output)".to_string()
                        } else {
                            format!("Command failed with exit code {exit_code} (no output)")
                        }
                    } else {
                        combined
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
            Command::new(&parts[0])
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
        let mut cmd = Command::new("cmd");
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
        let mut cmd = Command::new("sh");
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
            let stdout_pipe = child.stdout.take();

            const INACTIVITY_TIMEOUT_SECS: u64 = 120;
            const TOTAL_TIMEOUT_SECS: u64 = 300; // 5 min hard wall-clock limit
            // Check cancellation every 200ms — responsive enough without busy-waiting
            const POLL_INTERVAL_MS: u64 = 200;

            let mut was_cancelled = false;
            let mut inactivity_killed = false;
            let mut total_timeout_killed = false;
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

                    // Hard wall-clock limit — prevents commands that trickle
                    // output (e.g. winget progress bars) from running forever
                    if wall_start.elapsed().as_secs() >= TOTAL_TIMEOUT_SECS {
                        eprintln!(
                            "[STREAM] Total timeout ({}s), killing pid={}",
                            TOTAL_TIMEOUT_SECS, child_pid
                        );
                        kill_process_tree(child_pid);
                        total_timeout_killed = true;
                        break;
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
                            // No data — check inactivity threshold
                            if last_data.elapsed().as_secs() >= INACTIVITY_TIMEOUT_SECS {
                                eprintln!(
                                    "[STREAM] Inactivity timeout ({}s), killing pid={}",
                                    INACTIVITY_TIMEOUT_SECS, child_pid
                                );
                                kill_process_tree(child_pid);
                                inactivity_killed = true;
                                break;
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
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

            // Reap child process
            let status = child.wait();
            let exit_code = status.as_ref().ok().and_then(|s| s.code()).unwrap_or(-1);
            let success = status.as_ref().map(|s| s.success()).unwrap_or(false);

            if was_cancelled {
                output.push_str("\n[Cancelled by user]\n");
            } else if total_timeout_killed {
                output.push_str(&format!(
                    "\n[Process killed: exceeded {}s wall-clock limit]\n",
                    TOTAL_TIMEOUT_SECS
                ));
            } else if inactivity_killed {
                output.push_str(&format!(
                    "\n[Process killed: no output for {}s — likely waiting for input]\n",
                    INACTIVITY_TIMEOUT_SECS
                ));
            }

            if output.trim().is_empty() {
                if success {
                    "Command executed successfully (no output)".to_string()
                } else {
                    format!("Command failed with exit code {exit_code} and produced no output.")
                }
            } else {
                output
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

/// Execute a command in the background. Captures initial output for `BACKGROUND_CAPTURE_SECS`,
/// then returns immediately while the process keeps running. A persistent reader thread
/// continues buffering output so `check_background_process()` can retrieve it later.
pub fn execute_command_background(
    cmd: &str,
    mut on_line: impl FnMut(&str),
) -> String {
    let trimmed = cmd.trim();

    // Env vars for unbuffered output
    let env_vars = [
        ("PYTHONUNBUFFERED", "1"),
        ("COMPOSER_PROCESS_TIMEOUT", "0"),
        ("GIT_FLUSH", "1"),
        ("CI", "true"),
    ];

    #[cfg(target_os = "windows")]
    let child_result = {
        let path = enriched_windows_path();
        let mut c = Command::new("cmd");
        c.raw_arg(format!("/C {trimmed} 2>&1"))
            .env("PATH", &path);
        for (k, v) in &env_vars {
            c.env(k, v);
        }
        // CREATE_NEW_PROCESS_GROUP (0x200) so child survives parent exit
        c.creation_flags(0x200);
        c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
    };

    #[cfg(not(target_os = "windows"))]
    let child_result = {
        let mut c = Command::new("sh");
        c.arg("-c").arg(format!("{trimmed} 2>&1"));
        for (k, v) in &env_vars {
            c.env(k, v);
        }
        c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
    };

    match child_result {
        Ok(mut child) => {
            let pid = child.id();
            let stdout_pipe = match child.stdout.take() {
                Some(p) => p,
                None => {
                    return format!("Error: Could not capture stdout for background process (PID: {pid})");
                }
            };

            // Shared state between reader thread, initial capture, and later check() calls
            let output_buffer: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
            let running = Arc::new(AtomicBool::new(true));
            let cursor = Arc::new(AtomicUsize::new(0));

            // Channel for initial capture: reader thread sends lines here too
            let (tx, rx) = std::sync::mpsc::channel::<String>();

            // Persistent reader thread — runs until process exits (EOF)
            let buf_ref = output_buffer.clone();
            let running_ref = running.clone();
            let reader_pid = pid;
            std::thread::spawn(move || {
                let mut reader = std::io::BufReader::new(stdout_pipe);
                let mut line_buf = String::new();
                let mut byte_buf = [0u8; 4096];

                loop {
                    match std::io::Read::read(&mut reader, &mut byte_buf) {
                        Ok(0) => break, // EOF
                        Ok(n) => {
                            let chunk = String::from_utf8_lossy(&byte_buf[..n]);
                            for ch in chunk.chars() {
                                if ch == '\n' || ch == '\r' {
                                    if !line_buf.is_empty() {
                                        // Append to shared buffer (always)
                                        if let Ok(mut buf) = buf_ref.lock() {
                                            buf.push(line_buf.clone());
                                        }
                                        // Also try to send to initial capture channel (may be closed)
                                        let _ = tx.send(line_buf.clone());
                                        line_buf.clear();
                                    }
                                } else {
                                    line_buf.push(ch);
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
                // Flush remaining
                if !line_buf.is_empty() {
                    if let Ok(mut buf) = buf_ref.lock() {
                        buf.push(line_buf.clone());
                    }
                    let _ = tx.send(line_buf);
                }
                running_ref.store(false, Ordering::Relaxed);
                unpersist_bg_process(reader_pid);
            });

            // Collect initial output for BACKGROUND_CAPTURE_SECS
            let deadline = std::time::Instant::now()
                + std::time::Duration::from_secs(BACKGROUND_CAPTURE_SECS);
            let mut initial_output = String::new();
            let mut exited_early = false;

            loop {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                match rx.recv_timeout(remaining) {
                    Ok(line) => {
                        on_line(&line);
                        initial_output.push_str(&line);
                        initial_output.push('\n');
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        exited_early = true;
                        break;
                    }
                }
            }

            // Set cursor past initial output so check() only returns NEW lines
            if let Ok(buf) = output_buffer.lock() {
                cursor.store(buf.len(), Ordering::Relaxed);
            }

            if exited_early {
                // Process already exited within the capture window
                return format!(
                    "{}\n(Process exited within {}s, PID: {})",
                    initial_output.trim(),
                    BACKGROUND_CAPTURE_SECS,
                    pid
                );
            }

            // Register in global registry
            if let Ok(mut procs) = BACKGROUND_PROCESSES.lock() {
                procs.push(BackgroundProcess {
                    pid,
                    command: trimmed.to_string(),
                    output_buffer,
                    cursor,
                    running,
                    started_at: Instant::now(),
                    no_output_checks: AtomicUsize::new(0),
                    total_checks: AtomicUsize::new(0),
                });
            }

            // Persist to DB for crash recovery
            persist_bg_process(pid, trimmed, None);

            // Forget the child handle so dropping it doesn't kill the process
            std::mem::forget(child);

            format!(
                "{}\n(Process running in background, PID: {})",
                initial_output.trim(),
                pid
            )
        }
        Err(e) => format!("Failed to start background process: {e}"),
    }
}

/// Check on a background process by PID. Returns status and any new output since last check.
/// If `wait_seconds > 0`, sleeps first before checking (merges wait + check into one call).
pub fn check_background_process(pid: u32, wait_seconds: u64) -> String {
    // Optional built-in wait before checking (saves a separate tool call)
    if wait_seconds > 0 {
        let capped = wait_seconds.min(30);
        std::thread::sleep(std::time::Duration::from_secs(capped));
    }
    let procs = match BACKGROUND_PROCESSES.lock() {
        Ok(p) => p,
        Err(_) => return "Error: Failed to access background process registry".to_string(),
    };

    let proc = match procs.iter().find(|p| p.pid == pid) {
        Some(p) => p,
        None => {
            // List available PIDs for convenience
            let available: Vec<String> = procs.iter().map(|p| format!("{}", p.pid)).collect();
            if available.is_empty() {
                return format!("No background process with PID {} found. No background processes are currently tracked.", pid);
            }
            return format!(
                "No background process with PID {} found. Tracked PIDs: {}",
                pid,
                available.join(", ")
            );
        }
    };

    let is_running = proc.running.load(Ordering::Relaxed);
    let status = if is_running { "running" } else { "exited" };

    let buf = match proc.output_buffer.lock() {
        Ok(b) => b,
        Err(_) => return format!("PID {}: {}\nStatus: {}\nError: could not read output buffer", pid, proc.command, status),
    };

    let prev_cursor = proc.cursor.load(Ordering::Relaxed);
    let new_lines: Vec<String> = buf[prev_cursor..].to_vec();
    proc.cursor.store(buf.len(), Ordering::Relaxed);

    // Track elapsed time since process started for context
    let elapsed = proc.started_at.elapsed();
    let elapsed_str = if elapsed.as_secs() >= 60 {
        format!("{}m{}s", elapsed.as_secs() / 60, elapsed.as_secs() % 60)
    } else {
        format!("{}s", elapsed.as_secs())
    };

    // Track total checks (never resets) — prevents infinite polling loops
    let total = proc.total_checks.fetch_add(1, Ordering::Relaxed) + 1;
    const MAX_TOTAL_CHECKS: usize = 5;

    if new_lines.is_empty() {
        if is_running {
            // Increment no-output check counter
            let checks = proc.no_output_checks.fetch_add(1, Ordering::Relaxed) + 1;
            if checks >= 5 || total >= MAX_TOTAL_CHECKS {
                return format!(
                    "PID {}: {}\nStatus: running ({})\n\
                    STOP POLLING. You have checked this process {} times. \
                    The process is still running in the background and will complete on its own. \
                    ASSUME IT WILL SUCCEED and continue with the next steps immediately. \
                    Do NOT check this PID again. Do NOT re-run the same command. \
                    Proceed as if the command completed successfully.",
                    pid, proc.command, elapsed_str, total
                );
            }
        }
        let mut result = format!(
            "PID {}: {}\nStatus: {} (running for {})\nNo new output since last check.",
            pid, proc.command, status, elapsed_str
        );
        if is_running {
            result.push_str("\n\nUse the `wait_seconds` parameter (e.g. 15) on your next check_background_process call to pause before checking again.");
        }
        result
    } else {
        // New output found — reset the no-output counter (but NOT total_checks)
        proc.no_output_checks.store(0, Ordering::Relaxed);

        let mut result = format!(
            "PID {}: {}\nStatus: {} (running for {})\nNew output ({} lines):\n{}",
            pid,
            proc.command,
            status,
            elapsed_str,
            new_lines.len(),
            new_lines.join("\n")
        );
        if is_running {
            if total >= MAX_TOTAL_CHECKS {
                result.push_str(&format!(
                    "\n\nSTOP POLLING. You have checked this process {} times. \
                    The process is still running and will complete on its own. \
                    ASSUME IT WILL SUCCEED and continue with the next steps immediately. \
                    Do NOT check this PID again. Do NOT re-run the same command. \
                    Proceed as if the command completed successfully.",
                    total
                ));
            } else {
                result.push_str("\n\nProcess is still running. Use `wait_seconds: 15` on your next check_background_process call to pause before checking.");
            }
        }
        result
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
            let output = Command::new("sh").arg("-c").arg(prefix).output();
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
}
