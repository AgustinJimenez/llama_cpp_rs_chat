//! Background process infrastructure — tracking, persistence, and lifecycle management.

use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;

use crate::utils::silent_command;
use llama_chat_db::SharedDatabase;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

// ── Constants ────────────────────────────────────────────────────────────────

/// How long to capture initial output before returning (seconds).
const BACKGROUND_CAPTURE_SECS: u64 = 5;

// ── Background process struct & registry ─────────────────────────────────────

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
    static ref BG_DB_REF: StdMutex<Option<SharedDatabase>> = StdMutex::new(None);
    /// Unique session ID generated at app startup. Used to detect orphaned processes
    /// from previous sessions that crashed without cleanup.
    static ref BG_SESSION_ID: StdMutex<String> = StdMutex::new(String::new());
}

// ── Init & DB persistence ────────────────────────────────────────────────────

/// Initialize the background process tracking system with DB and session ID.
/// Called once at worker startup.
pub fn init_background_tracking(db: SharedDatabase, session_id: String) {
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

// ── Process queries ──────────────────────────────────────────────────────────

/// Check if a process is still alive by PID.
pub fn is_process_alive(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        // Use tasklist to check if PID exists (no extra dependencies)
        silent_command("tasklist")
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
pub fn get_orphaned_processes(db: &SharedDatabase) -> Vec<(u32, String, i64)> {
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
pub fn cleanup_dead_process_records(db: &SharedDatabase) {
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

// ── Kill & cleanup ───────────────────────────────────────────────────────────

/// Kill a background process by PID and remove from DB.
/// Public wrapper used by REST API.
pub fn kill_background_process_by_pid(pid: u32) {
    crate::kill_process_tree(pid);
    unpersist_bg_process(pid);
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
            crate::kill_process_tree(*pid);
            unpersist_bg_process(*pid);
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

// ── Background execution ─────────────────────────────────────────────────────

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
        let path = crate::enriched_windows_path();
        let mut c = silent_command("cmd");
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
        let mut c = silent_command("sh");
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
