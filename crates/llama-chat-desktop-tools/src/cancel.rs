//! Cancellation context and timeout helpers for desktop tool calls.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

// ─── Global desktop abort flag (set via /api/desktop/abort) ─────────────────
static DESKTOP_ABORT: AtomicBool = AtomicBool::new(false);

/// Set the global desktop abort flag. Called from the HTTP abort endpoint.
pub fn set_desktop_abort(abort: bool) {
    DESKTOP_ABORT.store(abort, Ordering::Relaxed);
}

/// Check (and auto-reset) the global desktop abort flag.
/// Returns `true` if abort was requested.
pub fn check_desktop_abort() -> bool {
    DESKTOP_ABORT.compare_exchange(true, false, Ordering::Relaxed, Ordering::Relaxed).is_ok()
}

// ─── Cancellation context ───────────────────────────────────────────────────

#[derive(Clone)]
pub struct DesktopCancellationContext {
    pub(crate) cancelled: Arc<AtomicBool>,
    pub(crate) deadline: std::time::Instant,
}

#[allow(dead_code)]
impl DesktopCancellationContext {
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            deadline: std::time::Instant::now() + timeout,
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed) || std::time::Instant::now() >= self.deadline
    }

    pub fn remaining(&self) -> Option<Duration> {
        self.deadline.checked_duration_since(std::time::Instant::now())
    }
}

thread_local! {
    static CURRENT_CANCEL_CONTEXT: RefCell<Option<DesktopCancellationContext>> = const { RefCell::new(None) };
}

pub fn with_desktop_cancellation_context<T>(
    context: DesktopCancellationContext,
    f: impl FnOnce() -> T,
) -> T {
    CURRENT_CANCEL_CONTEXT.with(|cell| {
        let previous = cell.replace(Some(context));
        let result = f();
        cell.replace(previous);
        result
    })
}

pub fn current_desktop_cancellation_context() -> Option<DesktopCancellationContext> {
    CURRENT_CANCEL_CONTEXT.with(|cell| cell.borrow().clone())
}

pub fn desktop_call_cancelled() -> bool {
    // Check the global abort flag first (set via /api/desktop/abort)
    if DESKTOP_ABORT.load(Ordering::Relaxed) {
        return true;
    }
    current_desktop_cancellation_context()
        .map(|ctx| ctx.is_cancelled())
        .unwrap_or(false)
}

pub fn desktop_cancel_error() -> String {
    if let Some(ctx) = current_desktop_cancellation_context() {
        if ctx.cancelled.load(Ordering::Relaxed) {
            "Operation cancelled".to_string()
        } else if std::time::Instant::now() >= ctx.deadline {
            "Operation timed out".to_string()
        } else {
            "Operation cancelled".to_string()
        }
    } else {
        "Operation cancelled".to_string()
    }
}

pub fn ensure_desktop_not_cancelled() -> Result<(), String> {
    if desktop_call_cancelled() {
        Err(desktop_cancel_error())
    } else {
        Ok(())
    }
}

pub fn interruptible_sleep(duration: Duration) -> Result<(), String> {
    let deadline = std::time::Instant::now() + duration;
    let slice = Duration::from_millis(50);
    loop {
        ensure_desktop_not_cancelled()?;
        let now = std::time::Instant::now();
        if now >= deadline {
            return Ok(());
        }
        let remaining = deadline.duration_since(now).min(slice);
        std::thread::sleep(remaining);
    }
}

/// Spawn a closure on a new thread and wait up to `timeout` for it to finish.
/// Returns Err if the thread panics or times out.
pub fn spawn_with_timeout<F, T>(timeout: Duration, f: F) -> Result<T, String>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    ensure_desktop_not_cancelled()?;

    let context = current_desktop_cancellation_context();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = if let Some(context) = context {
            with_desktop_cancellation_context(context, f)
        } else {
            f()
        };
        let _ = tx.send(result);
    });

    let timeout_deadline = std::time::Instant::now() + timeout;
    let poll = Duration::from_millis(50);

    loop {
        match rx.recv_timeout(poll) {
            Ok(result) => return Ok(result),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if let Some(context) = current_desktop_cancellation_context() {
                    if context.is_cancelled() {
                        return Err(desktop_cancel_error());
                    }
                }
                if std::time::Instant::now() >= timeout_deadline {
                    if let Some(context) = current_desktop_cancellation_context() {
                        context.cancel();
                    }
                    return Err(format!("Operation timed out after {}ms", timeout.as_millis()));
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err("Thread panicked".to_string())
            }
        }
    }
}
