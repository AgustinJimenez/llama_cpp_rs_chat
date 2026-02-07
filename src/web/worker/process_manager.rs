//! Worker process lifecycle management.
//!
//! Spawns the worker as a child process (same binary with `--worker` flag),
//! monitors its health, and restarts it on crash.

use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

/// Manages the worker child process lifecycle.
pub struct ProcessManager {
    child: Mutex<Option<Child>>,
    db_path: String,
    restart_count: AtomicU32,
    is_alive: AtomicBool,
}

impl ProcessManager {
    /// Spawn a new worker process.
    pub fn spawn(db_path: &str) -> Result<Self, String> {
        let child = spawn_worker(db_path)?;

        Ok(Self {
            child: Mutex::new(Some(child)),
            db_path: db_path.to_string(),
            restart_count: AtomicU32::new(0),
            is_alive: AtomicBool::new(true),
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
    pub fn kill(&self) {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(ref mut child) = *guard {
                eprintln!("[PROCESS_MGR] Killing worker process");
                let _ = child.kill();
                let _ = child.wait(); // Reap
            }
            *guard = None;
        }
        self.is_alive.store(false, Ordering::SeqCst);
    }

    /// Restart the worker process (after kill or crash).
    pub fn restart(&self) -> Result<(), String> {
        // Kill existing if still alive
        self.kill();

        let child = spawn_worker(&self.db_path)?;
        if let Ok(mut guard) = self.child.lock() {
            *guard = Some(child);
        }
        self.is_alive.store(true, Ordering::SeqCst);
        self.restart_count.fetch_add(1, Ordering::Relaxed);

        eprintln!(
            "[PROCESS_MGR] Worker restarted (restart #{})",
            self.restart_count.load(Ordering::Relaxed)
        );
        Ok(())
    }

    /// Check if the worker process is still alive (non-blocking).
    pub fn check_alive(&self) -> bool {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(ref mut child) = *guard {
                match child.try_wait() {
                    Ok(None) => return true, // Still running
                    Ok(Some(status)) => {
                        eprintln!("[PROCESS_MGR] Worker exited with status: {status}");
                        *guard = None;
                        self.is_alive.store(false, Ordering::SeqCst);
                        return false;
                    }
                    Err(e) => {
                        eprintln!("[PROCESS_MGR] Failed to check worker status: {e}");
                        return false;
                    }
                }
            }
        }
        false
    }

    pub fn is_alive(&self) -> bool {
        self.is_alive.load(Ordering::SeqCst)
    }

    pub fn restart_count(&self) -> u32 {
        self.restart_count.load(Ordering::Relaxed)
    }
}

impl Drop for ProcessManager {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Spawn a worker child process using the current executable.
fn spawn_worker(db_path: &str) -> Result<Child, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Cannot find own executable: {e}"))?;

    eprintln!("[PROCESS_MGR] Spawning worker: {} --worker --db-path {db_path}", exe.display());

    Command::new(exe)
        .arg("--worker")
        .arg("--db-path")
        .arg(db_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit()) // Worker logs go to parent's stderr
        .spawn()
        .map_err(|e| format!("Failed to spawn worker: {e}"))
}
