//! Prevent Windows from sleeping during long-running tasks.
//! Uses SetThreadExecutionState to keep the display/system awake.
//! Reference count based — multiple retain/release calls are safe.

use std::sync::atomic::{AtomicUsize, Ordering};

static PREVENT_SLEEP_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Start preventing sleep. Call when generation or tool execution begins.
pub fn retain() {
    let prev = PREVENT_SLEEP_COUNT.fetch_add(1, Ordering::Relaxed);
    if prev == 0 {
        set_execution_state(true);
        eprintln!("[SLEEP] Preventing system sleep");
    }
}

/// Allow sleep again. Call when generation or tool execution ends.
pub fn release() {
    let prev = PREVENT_SLEEP_COUNT.fetch_sub(1, Ordering::Relaxed);
    if prev == 0 || prev == 1 {
        // Clamp to 0
        PREVENT_SLEEP_COUNT.store(0, Ordering::Relaxed);
        set_execution_state(false);
        eprintln!("[SLEEP] Allowing system sleep");
    }
}

/// Force release — reset regardless of count. Use on shutdown.
pub fn force_release() {
    PREVENT_SLEEP_COUNT.store(0, Ordering::Relaxed);
    set_execution_state(false);
}

#[cfg(target_os = "windows")]
fn set_execution_state(prevent: bool) {
    // ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
    const ES_CONTINUOUS: u32 = 0x80000000;
    const ES_SYSTEM_REQUIRED: u32 = 0x00000001;
    const ES_DISPLAY_REQUIRED: u32 = 0x00000002;

    extern "system" {
        fn SetThreadExecutionState(flags: u32) -> u32;
    }

    unsafe {
        if prevent {
            SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED);
        } else {
            SetThreadExecutionState(ES_CONTINUOUS);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn set_execution_state(prevent: bool) {
    // macOS: could use caffeinate, Linux: could use systemd-inhibit
    // For now, no-op on non-Windows
    let _ = prevent;
}
