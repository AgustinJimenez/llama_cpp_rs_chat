/// Watchdog thread that monitors generation heartbeat and kills the worker on deadlock.
///
/// The heartbeat is updated after every successful sample()/decode(). If not updated
/// within WATCHDOG_TIMEOUT_MS, the watchdog assumes a CUDA deadlock and calls
/// `process::exit(42)` to force a clean restart.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub const WATCHDOG_TIMEOUT_MS: u64 = 10_000; // 10 seconds

/// Live handles to the running watchdog thread.
pub(crate) struct WatchdogHandles {
    /// Stores the millisecond timestamp of the last successful sample()/decode().
    pub heartbeat: Arc<AtomicU64>,
    /// Set to `true` when generation is done — causes the watchdog to exit its loop.
    pub done: Arc<AtomicBool>,
    /// Set to `true` while a tool is executing — watchdog skips deadlock checks.
    pub paused: Arc<AtomicBool>,
    pub _join: std::thread::JoinHandle<()>,
}

impl WatchdogHandles {
    /// Spawn the watchdog thread and return handles.
    pub(crate) fn spawn(
        cancel: Arc<AtomicBool>,
        conversation_id: String,
    ) -> Self {
        let now_ms = || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
        };

        let heartbeat = Arc::new(AtomicU64::new(now_ms()));
        let done = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));

        let hb = heartbeat.clone();
        let done_flag = done.clone();
        let paused_flag = paused.clone();
        let _cancel = cancel;

        let handle = std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                if done_flag.load(Ordering::Relaxed) {
                    break;
                }
                if paused_flag.load(Ordering::Relaxed) {
                    continue;
                }
                let last = hb.load(Ordering::Relaxed);
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                if now.saturating_sub(last) > WATCHDOG_TIMEOUT_MS {
                    eprintln!(
                        "[WATCHDOG] sample()/decode() deadlock detected after {}ms — killing worker process (conv={})",
                        now - last, conversation_id
                    );
                    llama_chat_db::event_log::log_event(
                        &conversation_id,
                        "watchdog",
                        &format!("Deadlock detected: {}ms — worker process exit", now - last),
                    );
                    std::process::exit(42);
                }
            }
        });

        WatchdogHandles { heartbeat, done, paused, _join: handle }
    }

    /// Update the heartbeat to the current time (call after sample()/decode() succeeds).
    pub(crate) fn ping(&self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.heartbeat.store(now, Ordering::Relaxed);
    }

    /// Pause deadlock checks (call before slow operations like tool execution).
    pub(crate) fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
    }

    /// Resume deadlock checks (call after slow operation completes).
    pub(crate) fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
    }

    /// Signal the watchdog thread to exit.
    pub(crate) fn stop(self) {
        self.done.store(true, Ordering::Relaxed);
        let _ = self._join.join();
    }
}
