//! Window management, enumeration, keystroke injection, and process tools.
//!
//! This module is a thin orchestrator: all implementation lives in the
//! focused submodules below.  Everything is re-exported so that callers
//! continue to use the same unqualified names (e.g. `tool_list_windows`).
//!
//! Submodule layout
//! ─────────────────
//! window_enumeration  — list_windows, get_active_window, wait_for_window,
//!                       get_cursor_position, get_pixel_color, list_monitors
//! window_management   — focus/minimize/maximize/close/resize/snap/topmost,
//!                       click_window_relative, switch_virtual_desktop,
//!                       save_window_layout, restore_window_layout
//! window_keys         — send_keys_to_window (post_message / send_input / scancode)
//! window_process      — read/write_clipboard, list/kill_process, open_application,
//!                       get_process_info, wait_for_process_exit,
//!                       get_process_tree, get_system_metrics

// ─── Re-export crate-root items for submodule access via `super::` ───────────
//
// Submodules use `super::X` which resolves to `window_tools::X`.
// These re-exports bridge that path to the actual crate-root definitions.

pub(crate) use super::NativeToolResult;
#[allow(unused_imports)]
pub(crate) use super::{
    parse_bool, parse_int, parse_key_combo,
    tool_error, tool_click_screen, tool_press_key,
    ensure_desktop_not_cancelled, interruptible_sleep,
    validated_monitors,
};
pub(crate) use super::gpu_app_db;

#[cfg(windows)]
pub(crate) use super::win32;
#[cfg(target_os = "macos")]
pub(crate) use super::macos as win32;
#[cfg(target_os = "linux")]
pub(crate) use super::linux as win32;

// ─── Submodules ───────────────────────────────────────────────────────────────

mod window_enumeration;
mod window_management;
mod window_keys;
mod window_process;

pub use window_enumeration::*;
pub use window_management::*;
pub use window_keys::*;
pub use window_process::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_windows_runs_without_crash() {
        let args = serde_json::json!({});
        let result = tool_list_windows(&args);
        assert!(!result.text.is_empty());
    }

    #[test]
    fn test_focus_window_no_title_returns_error() {
        let args = serde_json::json!({});
        let result = tool_focus_window(&args);
        assert!(result.text.contains("Error [focus_window]"));
        assert!(result.text.contains("'title'"));
    }

    #[test]
    fn test_kill_process_no_args_returns_error() {
        let args = serde_json::json!({});
        let result = tool_kill_process(&args);
        assert!(result.text.contains("Error") || result.text.contains("required"));
    }

    #[test]
    fn test_close_window_no_title_returns_error() {
        let args = serde_json::json!({});
        let result = tool_close_window(&args);
        assert!(result.text.contains("Error [close_window]") || result.text.contains("'title'"));
    }

    #[test]
    fn test_snap_window_missing_title_and_pid() {
        let args = serde_json::json!({"position": "left"});
        let result = tool_snap_window(&args);
        assert!(result.text.contains("Error [snap_window]"));
        assert!(result.text.contains("'title' or 'pid'"));
    }

    #[test]
    fn test_snap_window_nonexistent_window() {
        let args = serde_json::json!({"position": "left", "title": "__nonexistent_window_xyz__"});
        let result = tool_snap_window(&args);
        assert!(
            result.text.contains("window")
                || result.text.contains("Error")
                || result.text.contains("match"),
            "unexpected result: {}", result.text
        );
    }
}
