//! Shared recording state (the active ffmpeg child process).

use std::sync::Mutex;
use std::time::Instant;

// ─── Recording state ─────────────────────────────────────────────────────────

pub(crate) struct RecordingState {
    pub(crate) child: Option<std::process::Child>,
    pub(crate) output_path: String,
    pub(crate) started_at: Instant,
}

impl Drop for RecordingState {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

lazy_static::lazy_static! {
    pub(crate) static ref RECORDING_STATE: Mutex<Option<RecordingState>> = Mutex::new(None);
}
