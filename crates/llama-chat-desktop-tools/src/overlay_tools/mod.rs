//! Status overlay tools: persistent on-screen bar showing automation progress.
//!
//! - **Windows**: PowerShell WinForms window that polls a control file for text updates.
//! - **macOS**: `osascript` running AppleScript that displays a persistent floating panel polling a control file.
//! - **Linux**: `zenity --progress` fed updates via a control file polling wrapper, with `notify-send` fallback.
//!
//! Lifecycle: show_status_overlay (spawn) → update_status_overlay (write file) → hide_status_overlay (write __EXIT__).

mod scripts;

use serde_json::Value;
use std::path::PathBuf;
use std::process::Child;
use std::sync::Mutex;

use super::NativeToolResult;

// ─── State persistence ───────────────────────────────────────────────────────

struct OverlayState {
    child: Child,
    control_file: PathBuf,
}

impl Drop for OverlayState {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.control_file, "__EXIT__");
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.control_file);
    }
}

lazy_static::lazy_static! {
    /// Holds the overlay child process and control file path (platform-agnostic).
    static ref OVERLAY: Mutex<Option<OverlayState>> = Mutex::new(None);
}

// ─── Internal helpers ────────────────────────────────────────────────────────

fn control_file_path() -> PathBuf {
    std::env::temp_dir().join("claude_overlay_ctrl.txt")
}

/// Kill an existing overlay process with a bounded wait.
fn kill_existing_overlay(old: &mut OverlayState) {
    let _ = std::fs::write(&old.control_file, "__EXIT__");
    std::thread::sleep(std::time::Duration::from_millis(300));
    let _ = old.child.kill();
    // Bounded wait — never block more than 1 second
    for _ in 0..10 {
        if old.child.try_wait().ok().flatten().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let _ = std::fs::remove_file(&old.control_file);
}

// ─── Tool functions ──────────────────────────────────────────────────────────

/// Show a persistent status overlay bar on screen.
/// Args: { text: String, position?: "top"|"bottom" }
pub fn tool_show_status_overlay(args: &Value) -> NativeToolResult {
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return super::tool_error("show_status_overlay", "'text' is required"),
    };
    let position = args
        .get("position")
        .and_then(|v| v.as_str())
        .unwrap_or("top");

    // Unsupported platforms
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = (text, position);
        return NativeToolResult::text_only(
            "Status overlay is not supported on this platform.".to_string(),
        );
    }

    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    {
        use std::process::{Command, Stdio};

        let mut lock = OVERLAY.lock().unwrap_or_else(|poisoned| {
            log_warn!("system", "Mutex poisoned in OVERLAY, recovering");
            poisoned.into_inner()
        });

        // Kill existing overlay if any
        if let Some(mut old) = lock.take() {
            kill_existing_overlay(&mut old);
        }

        // Write initial text to control file
        let ctrl = control_file_path();
        if let Err(e) = std::fs::write(&ctrl, text) {
            return super::tool_error("show_status_overlay", format!("writing control file: {e}"));
        }

        // ── Windows: PowerShell WinForms ──
        #[cfg(windows)]
        let spawn_result = {
            use std::os::windows::process::CommandExt;

            let script_body =
                &scripts::OVERLAY_PS_SCRIPT[scripts::OVERLAY_PS_SCRIPT.find("Add-Type").unwrap_or(0)..];
            let script_with_param = format!(
                "$ControlFile = '{}'; $Position = '{}'; {}",
                ctrl.to_string_lossy().replace('\'', "''"),
                position,
                script_body,
            );

            let mut cmd = Command::new("powershell");
            cmd.args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &script_with_param,
            ]);
            cmd.stdin(Stdio::null());
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
            cmd.spawn()
        };

        // ── macOS: osascript with AppleScript ──
        #[cfg(target_os = "macos")]
        let spawn_result = {
            let mut cmd = Command::new("osascript");
            cmd.arg("-");
            cmd.arg(ctrl.to_string_lossy().as_ref());
            cmd.arg(position);
            cmd.stdin(Stdio::piped());
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());

            // We pipe the script via stdin so we don't need a temp file
            match cmd.spawn() {
                Ok(mut child) => {
                    // Write the AppleScript to osascript's stdin, then close it
                    if let Some(mut stdin) = child.stdin.take() {
                        use std::io::Write;
                        let _ = stdin.write_all(scripts::OVERLAY_APPLESCRIPT.as_bytes());
                        // stdin is dropped here, closing the pipe
                    }
                    Ok(child)
                }
                Err(e) => Err(e),
            }
        };

        // ── Linux: bash script with zenity/yad/kdialog/notify-send ──
        #[cfg(target_os = "linux")]
        let spawn_result = {
            // bash -c <script> _ <arg1> <arg2>  →  $1=arg1, $2=arg2
            let mut cmd = Command::new("bash");
            cmd.arg("-c");
            cmd.arg(scripts::OVERLAY_BASH_SCRIPT);
            cmd.arg("_"); // becomes $0 (unused)
            cmd.arg(ctrl.to_string_lossy().as_ref()); // $1 = control file
            cmd.arg(position); // $2 = position
            cmd.stdin(Stdio::null());
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());
            cmd.spawn()
        };

        match spawn_result {
            Ok(child) => {
                *lock = Some(OverlayState {
                    child,
                    control_file: ctrl,
                });
                NativeToolResult::text_only(format!("Status overlay shown: \"{text}\""))
            }
            Err(e) => {
                let _ = std::fs::remove_file(&ctrl);
                super::tool_error("show_status_overlay", format!("spawning overlay: {e}"))
            }
        }
    }
}

/// Update the text on an existing status overlay.
/// Args: { text: String }
pub fn tool_update_status_overlay(args: &Value) -> NativeToolResult {
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return super::tool_error("update_status_overlay", "'text' is required"),
    };

    let mut lock = OVERLAY.lock().unwrap_or_else(|poisoned| {
        log_warn!("system", "Mutex poisoned in OVERLAY, recovering");
        poisoned.into_inner()
    });
    match lock.as_mut() {
        Some(state) => {
            // Check if process still alive
            match state.child.try_wait() {
                Ok(Some(_)) => {
                    let _ = std::fs::remove_file(&state.control_file);
                    *lock = None;
                    super::tool_error(
                        "update_status_overlay",
                        "overlay exited. Call show_status_overlay first.",
                    )
                }
                Ok(None) => {
                    match std::fs::write(&state.control_file, text) {
                        Ok(()) => NativeToolResult::text_only(format!(
                            "Overlay updated: \"{text}\""
                        )),
                        Err(e) => super::tool_error(
                            "update_status_overlay",
                            format!("updating overlay: {e}"),
                        ),
                    }
                }
                Err(e) => {
                    super::tool_error("update_status_overlay", format!("checking overlay process: {e}"))
                }
            }
        }
        None => super::tool_error(
            "update_status_overlay",
            "no overlay shown. Call show_status_overlay first.",
        ),
    }
}

/// Hide (dismiss) the status overlay.
pub fn tool_hide_status_overlay(_args: &Value) -> NativeToolResult {
    let mut lock = OVERLAY.lock().unwrap_or_else(|poisoned| {
        log_warn!("system", "Mutex poisoned in OVERLAY, recovering");
        poisoned.into_inner()
    });
    match lock.take() {
        Some(mut state) => {
            // Signal exit via control file, then force kill with bounded wait
            let _ = std::fs::write(&state.control_file, "__EXIT__");
            std::thread::sleep(std::time::Duration::from_millis(500));
            if state.child.try_wait().ok().flatten().is_none() {
                let _ = state.child.kill();
                for _ in 0..10 {
                    if state.child.try_wait().ok().flatten().is_some() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
            let _ = std::fs::remove_file(&state.control_file);
            NativeToolResult::text_only("Status overlay hidden.".to_string())
        }
        None => NativeToolResult::text_only("No overlay was shown.".to_string()),
    }
}
