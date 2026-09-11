//! Worker process lifecycle management.
//!
//! Spawns the worker as a child process (same binary with `--worker` flag),
//! monitors its health, and restarts it on crash.

use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

/// Manages the worker child process lifecycle.
pub struct ProcessManager {
    child: Mutex<Option<Child>>,
    db_path: String,
    restart_count: AtomicU32,
    /// Monotonically increasing counter — incremented on every restart.
    /// Each stdout reader task records the generation at spawn time and
    /// aborts its crash-recovery handler if the generation has advanced
    /// (meaning another reader already took over).
    generation: AtomicU64,
    /// Set to true when this worker is intentionally shut down.
    /// The stdout reader task checks this before attempting crash recovery.
    is_shutdown: AtomicBool,
}

impl ProcessManager {
    /// Spawn a new worker process.
    pub fn spawn(db_path: &str) -> Result<Self, String> {
        let child = spawn_worker(db_path)?;

        Ok(Self {
            child: Mutex::new(Some(child)),
            db_path: db_path.to_string(),
            restart_count: AtomicU32::new(0),
            generation: AtomicU64::new(0),
            is_shutdown: AtomicBool::new(false),
        })
    }

    /// Take the child's stdin handle for writing commands.
    pub fn take_stdin(&self) -> Option<std::process::ChildStdin> {
        self.child
            .lock()
            .ok()
            .and_then(|mut guard| guard.as_mut().and_then(|c| c.stdin.take()))
    }

    /// Take the child's stdout handle for reading responses.
    pub fn take_stdout(&self) -> Option<std::process::ChildStdout> {
        self.child
            .lock()
            .ok()
            .and_then(|mut guard| guard.as_mut().and_then(|c| c.stdout.take()))
    }

    /// Kill the worker process immediately. OS reclaims all memory.
    /// Sets the shutdown flag so the stdout reader task won't auto-restart.
    ///
    /// Also kills all background processes tracked in the DB before killing the
    /// worker, because force-kill bypasses the worker's `BgProcessGuard::drop()`.
    pub fn kill(&self) {
        self.is_shutdown.store(true, Ordering::SeqCst);
        // Clean up worker's background processes before killing it.
        // The worker's BgProcessGuard won't fire on force-kill.
        llama_chat_command::background::kill_all_db_processes(&self.db_path);
        if let Ok(mut guard) = self.child.lock() {
            if let Some(ref mut child) = *guard {
                eprintln!("[PROCESS_MGR] Killing worker process");
                let _ = child.kill();
                let _ = child.wait(); // Reap
            }
            *guard = None;
        }
    }

    /// Returns true if this worker was intentionally shut down (not a crash).
    pub fn is_shutdown(&self) -> bool {
        self.is_shutdown.load(Ordering::SeqCst)
    }

    /// Restart the worker process (after kill or crash).
    pub fn restart(&self) -> Result<(), String> {
        // Kill existing if still alive
        self.kill();

        let child = spawn_worker(&self.db_path)?;
        if let Ok(mut guard) = self.child.lock() {
            *guard = Some(child);
        }
        self.restart_count.fetch_add(1, Ordering::Relaxed);
        self.generation.fetch_add(1, Ordering::SeqCst);
        // kill() (called above) latches is_shutdown=true so the dying reader skips crash
        // recovery. The freshly spawned worker is live again, so future deaths of THIS
        // worker must go through normal crash recovery — otherwise every restart after
        // the first permanently disables recovery for the rest of the process lifetime.
        self.is_shutdown.store(false, Ordering::SeqCst);

        eprintln!(
            "[PROCESS_MGR] Worker restarted (restart #{}, gen={})",
            self.restart_count.load(Ordering::Relaxed),
            self.generation.load(Ordering::Relaxed),
        );
        Ok(())
    }

    /// Return the current generation counter.
    /// Stdout reader tasks use this to detect if they've been superseded.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
}

impl Drop for ProcessManager {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Spawn a worker child process using the current executable.
/// Path of the worker stderr log, so callers and docs agree on one location.
pub fn worker_log_path() -> std::path::PathBuf {
    std::path::PathBuf::from("logs").join("worker.stderr.log")
}

/// Open the worker stderr log in append mode, creating `logs/` if needed.
/// Returns `None` on any failure so the caller can fall back to inheritance —
/// losing logs must never prevent a worker from starting.
fn worker_log_file() -> Option<std::fs::File> {
    let path = worker_log_path();
    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("[PROCESS_MGR] Cannot create worker log dir {}: {e}", dir.display());
            return None;
        }
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    // Windows: without an explicit share mode the child holds the file exclusively and
    // nothing can read it while a worker is alive — which defeats the point of a log.
    // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE = 0x7.
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        opts.share_mode(0x7);
    }
    match opts.open(&path) {
        Ok(f) => Some(f),
        Err(e) => {
            // Report it. The previous file-based attempt at this swallowed the error and
            // produced an empty log that read as "this code never ran" (AGENT_TASKS/012).
            eprintln!("[PROCESS_MGR] Cannot open worker log {}: {e}", path.display());
            None
        }
    }
}

fn spawn_worker(db_path: &str) -> Result<Child, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Cannot find own executable: {e}"))?;

    eprintln!("[PROCESS_MGR] Spawning worker: {} --worker --db-path {db_path}", exe.display());

    let mut cmd = Command::new(exe);
    cmd.arg("--worker")
        .arg("--db-path")
        .arg(db_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());

    // Worker stderr goes to an explicit file handle, not `Stdio::inherit()`.
    //
    // Inheritance does not survive the ways this app is actually launched: under the Tauri
    // desktop shell, a detached/background server, or `Start-Process` redirection, the
    // worker's stderr reached nowhere. Only the parent's own startup lines showed up, so
    // llama.cpp's loader output and every `eprintln!` in the engine were invisible — which
    // blocked the AGENT_TASKS/012 investigation entirely and made a silent failure look
    // like unreached code.
    //
    // Handing the child a real `File` (the pattern Atomic Chat uses for its `llama-server`
    // subprocess, `src-tauri/src/core/agent/eval/server.rs`) does not depend on the parent
    // having a usable stderr at all. Append mode so concurrent workers coexist and a
    // restart does not discard the log that explains why the previous one died.
    match worker_log_file() {
        Some(f) => {
            cmd.stderr(Stdio::from(f));
        }
        None => {
            cmd.stderr(Stdio::inherit());
        }
    }

    // On Windows, prevent the worker from opening a visible console window
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    cmd.spawn()
        .map_err(|e| format!("Failed to spawn worker: {e}"))
}
