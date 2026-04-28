//! Desktop automation tools: mouse, keyboard, and scroll input simulation.
//!
//! Uses the `enigo` crate for cross-platform input and `xcap` for post-action screenshots.
//! Each action tool optionally captures a screenshot after the action, returning it through
//! the vision pipeline so the LLM can see what happened.

#[allow(unused_imports)]
#[macro_use]
extern crate llama_chat_types;
#[allow(unused_imports)]
#[macro_use]
extern crate lazy_static;

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

pub use llama_chat_types::NativeToolResult;

// ─── Submodules ──────────────────────────────────────────────────────────────

mod helpers;
mod trace;
pub mod yolo_detect;

// Re-export helpers/trace so sibling modules keep using `super::tool_error` etc.
#[allow(unused_imports)]
pub use helpers::{
    cached_monitors, check_modal_dialog, encode_as_jpeg, encode_image_to_png,
    get_cached_screenshot, optimize_screenshot_for_vision, parse_bool, parse_float, parse_int,
    parse_key_combo, parse_timeout, pixel_diff_pct, resize_screenshot_for_vision,
    snap_coordinates, str_to_key, tool_error, tool_not_supported, update_screenshot_cache,
    validate_coordinates, validated_monitors, wait_for_screen_settle, with_enigo,
    DEFAULT_THREAD_TIMEOUT, SCREENSHOT_MAX_DIM,
};
#[allow(unused_imports)]
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub use helpers::apply_dpi_scaling;
#[allow(unused_imports)]
pub use trace::{
    capture_post_action_screenshot, capture_post_action_screenshot_ext, capture_screen_state,
    classify_desktop_result_status, finalize_action_result, finalize_desktop_result,
    normalize_verification_region, prepare_screen_verification, summarize_value_for_trace,
    write_desktop_trace_line, VerificationRegion,
};
#[cfg(test)]
use trace::action_verification_region_from_args;

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

/// Returns `true` if the given tool name is a known desktop automation tool.
pub fn is_desktop_tool(name: &str) -> bool {
    matches!(
        name,
        "click_screen" | "type_text" | "press_key" | "move_mouse"
            | "scroll_screen" | "mouse_drag" | "mouse_button"
            | "paste" | "clear_field" | "hover_element"
            | "take_screenshot" | "screenshot_region" | "screenshot_diff"
            | "window_screenshot" | "wait_for_screen_change"
            | "ocr_screen" | "ocr_find_text" | "ocr_region"
            | "get_ui_tree" | "click_ui_element" | "invoke_ui_action"
            | "read_ui_element_value" | "wait_for_ui_element"
            | "clipboard_image" | "find_ui_elements"
            | "list_windows" | "get_active_window" | "focus_window"
            | "minimize_window" | "maximize_window" | "close_window"
            | "resize_window" | "wait_for_window" | "click_window_relative"
            | "snap_window" | "set_window_topmost" | "open_application"
            | "list_processes" | "kill_process" | "send_keys_to_window"
            | "switch_virtual_desktop" | "get_process_info"
            | "read_clipboard" | "write_clipboard"
            | "get_cursor_position" | "get_pixel_color" | "list_monitors"
            | "find_and_click_text" | "type_into_element" | "get_window_text"
            | "file_dialog_navigate" | "drag_and_drop_element"
            | "wait_for_text_on_screen" | "get_context_menu"
            | "scroll_element" | "smart_wait" | "click_and_verify"
            | "handle_dialog" | "wait_for_element_state"
            | "fill_form" | "run_action_sequence"
            | "move_to_monitor" | "set_window_opacity" | "highlight_point"
            | "annotate_screenshot" | "find_color_on_screen" | "find_image_on_screen"
            | "read_registry" | "click_tray_icon" | "watch_window"
            | "execute_app_script" | "send_notification"
            | "show_status_overlay" | "update_status_overlay" | "hide_status_overlay"
            | "get_system_volume" | "set_system_volume" | "set_system_mute" | "list_audio_devices"
            | "clear_clipboard" | "clipboard_file_paths" | "clipboard_html"
            | "save_window_layout" | "restore_window_layout"
            | "wait_for_process_exit" | "get_process_tree" | "get_system_metrics"
            | "wait_for_notification" | "dismiss_all_notifications"
            | "start_screen_recording" | "stop_screen_recording" | "capture_gif"
            | "dialog_handler_start" | "dialog_handler_stop"
    )
}

// ─── Cancellation context ───────────────────────────────────────────────────

#[derive(Clone)]
pub struct DesktopCancellationContext {
    cancelled: Arc<AtomicBool>,
    deadline: std::time::Instant,
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

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed) || std::time::Instant::now() >= self.deadline
    }

    fn remaining(&self) -> Option<Duration> {
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

// ─── Platform modules ───────────────────────────────────────────────────────

#[cfg(windows)]
pub mod win32;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "linux")]
pub mod linux;

mod gpu_app_db;
mod window_tools;
pub use window_tools::*;

mod app_script_tools;
pub use app_script_tools::*;

mod screenshot_tools;
pub use screenshot_tools::*;
mod ocr_tools;
pub use ocr_tools::*;
mod ui_automation_tools;
pub use ui_automation_tools::*;
mod clipboard_tools;
pub use clipboard_tools::*;

mod compound_tools;
pub use compound_tools::*;

mod image_tools;
#[allow(unused_imports)]
pub use image_tools::*;
mod input_tools;
pub use input_tools::*;
mod dialog_tools;
pub use dialog_tools::*;
mod form_tools;
pub use form_tools::*;
mod display_tools;
pub use display_tools::*;
mod annotation_tools;
pub use annotation_tools::*;
mod system_tools;
pub use system_tools::*;
mod overlay_tools;
pub use overlay_tools::*;
mod audio_tools;
pub use audio_tools::*;
mod recording_tools;
pub use recording_tools::*;

/// Stand-alone `take_screenshot` implementation (also used by `native_tools` in the root crate).
pub fn tool_take_screenshot_with_image(args: &Value) -> NativeToolResult {
    let monitor_idx = args
        .get("monitor")
        .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
        .unwrap_or(0);

    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => return NativeToolResult::text_only(format!("Error: Failed to enumerate monitors: {e}")),
    };

    if monitors.is_empty() {
        return NativeToolResult::text_only("Error: No monitors detected".to_string());
    }

    // List monitors mode (no image captured)
    if monitor_idx == -1 {
        let mut result = format!("Available monitors ({}):\n", monitors.len());
        for (i, mon) in monitors.iter().enumerate() {
            let name = mon.name().unwrap_or_else(|_| "Unknown".to_string());
            let w = mon.width().unwrap_or(0);
            let h = mon.height().unwrap_or(0);
            let primary = mon.is_primary().unwrap_or(false);
            result.push_str(&format!(
                "  [{}] {} - {}x{}{}\n",
                i, name, w, h,
                if primary { " (primary)" } else { "" }
            ));
        }
        return NativeToolResult::text_only(result);
    }

    // Select monitor
    let monitor = if monitor_idx == 0 {
        monitors
            .iter()
            .find(|m| m.is_primary().unwrap_or(false))
            .unwrap_or(&monitors[0])
    } else {
        let idx = monitor_idx as usize;
        if idx >= monitors.len() {
            return NativeToolResult::text_only(format!(
                "Error: Monitor index {} out of range (0-{})",
                idx,
                monitors.len() - 1
            ));
        }
        &monitors[idx]
    };

    // Capture
    let image = match monitor.capture_image() {
        Ok(img) => img,
        Err(e) => return NativeToolResult::text_only(format!("Error: Screenshot capture failed: {e}")),
    };

    let width = image.width();
    let height = image.height();
    let mon_name = monitor.name().unwrap_or_else(|_| "Unknown".to_string());
    let is_primary = monitor.is_primary().unwrap_or(false);

    // Save to temp directory
    let screenshots_dir = std::env::temp_dir().join("llama_screenshots");
    if let Err(e) = std::fs::create_dir_all(&screenshots_dir) {
        return NativeToolResult::text_only(format!("Error: Failed to create screenshots directory: {e}"));
    }

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let filename = format!("screenshot_{timestamp}.png");
    let filepath = screenshots_dir.join(&filename);

    if let Err(e) = image.save(&filepath) {
        return NativeToolResult::text_only(format!("Error: Failed to save screenshot: {e}"));
    }

    let text = format!(
        "Screenshot saved: {}\nResolution: {}x{}\nMonitor: {} (primary: {})",
        filepath.display(),
        width,
        height,
        mon_name,
        if is_primary { "yes" } else { "no" }
    );

    // Also encode the image as PNG bytes for vision pipeline injection
    let png_bytes = std::fs::read(&filepath).unwrap_or_default();
    if png_bytes.is_empty() {
        NativeToolResult::text_only(text)
    } else {
        // Resize + JPEG-compress for vision models (saves tokens)
        let optimized = optimize_screenshot_for_vision(&png_bytes);
        NativeToolResult::with_image(text, optimized)
    }
}

/// Dispatch a desktop tool by name. Returns `None` if the tool name is not recognized.
/// Used by the MCP server binary to route tool calls to existing implementations.
#[allow(dead_code)]
pub fn dispatch_desktop_tool(name: &str, args: &Value) -> Option<NativeToolResult> {
    // Check global abort flag before executing any desktop tool
    if check_desktop_abort() {
        return Some(NativeToolResult::text_only(
            "Desktop action aborted by user".to_string(),
        ));
    }
    let started_at = std::time::Instant::now();
    let result = match name {
        // Core input tools (input_tools.rs)
        "click_screen" => tool_click_screen(args),
        "type_text" => tool_type_text(args),
        "press_key" => tool_press_key(args),
        "move_mouse" => tool_move_mouse(args),
        "scroll_screen" => tool_scroll_screen(args),
        "mouse_drag" => tool_mouse_drag(args),
        "mouse_button" => tool_mouse_button(args),

        // Input tools (input_tools.rs)
        "paste" => tool_paste(args),
        "clear_field" => tool_clear_field(args),
        "hover_element" => tool_hover_element(args),

        // Screenshot & OCR (ui_tools.rs)
        "take_screenshot" => tool_take_screenshot_with_image(args),
        "screenshot_region" => tool_screenshot_region(args),
        "screenshot_diff" => tool_screenshot_diff(args),
        "window_screenshot" => tool_window_screenshot(args),
        "wait_for_screen_change" => tool_wait_for_screen_change(args),
        "ocr_screen" => tool_ocr_screen(args),
        "ocr_find_text" => tool_ocr_find_text(args),
        "get_ui_tree" => tool_get_ui_tree(args),
        "click_ui_element" => tool_click_ui_element(args),
        "invoke_ui_action" => tool_invoke_ui_action(args),
        "read_ui_element_value" => tool_read_ui_element_value(args),
        "wait_for_ui_element" => tool_wait_for_ui_element(args),
        "clipboard_image" => tool_clipboard_image(args),
        "find_ui_elements" => tool_find_ui_elements(args),

        // Window tools (window_tools.rs)
        "list_windows" => tool_list_windows(args),
        "get_active_window" => tool_get_active_window(args),
        "focus_window" => tool_focus_window(args),
        "minimize_window" => tool_minimize_window(args),
        "maximize_window" => tool_maximize_window(args),
        "close_window" => tool_close_window(args),
        "resize_window" => tool_resize_window(args),
        "wait_for_window" => tool_wait_for_window(args),
        "click_window_relative" => tool_click_window_relative(args),
        "snap_window" => tool_snap_window(args),
        "set_window_topmost" => tool_set_window_topmost(args),
        "open_application" => tool_open_application(args),
        "list_processes" => tool_list_processes(args),
        "kill_process" => tool_kill_process(args),
        "send_keys_to_window" => tool_send_keys_to_window(args),
        "switch_virtual_desktop" => tool_switch_virtual_desktop(args),
        "get_process_info" => tool_get_process_info(args),
        "read_clipboard" => tool_read_clipboard(args),
        "write_clipboard" => tool_write_clipboard(args),
        "get_cursor_position" => tool_get_cursor_position(args),
        "get_pixel_color" => tool_get_pixel_color(args),
        "list_monitors" => tool_list_monitors(args),

        // Compound tools (compound_tools.rs)
        "find_and_click_text" => tool_find_and_click_text(args),
        "type_into_element" => tool_type_into_element(args),
        "get_window_text" => tool_get_window_text(args),
        "file_dialog_navigate" => tool_file_dialog_navigate(args),
        "drag_and_drop_element" => tool_drag_and_drop_element(args),
        "wait_for_text_on_screen" => tool_wait_for_text_on_screen(args),
        "get_context_menu" => tool_get_context_menu(args),
        "scroll_element" => tool_scroll_element(args),
        "smart_wait" => tool_smart_wait(args),
        "click_and_verify" => tool_click_and_verify(args),

        // Dialog & form tools
        "handle_dialog" => tool_handle_dialog(args),
        "wait_for_element_state" => tool_wait_for_element_state(args),
        "fill_form" => tool_fill_form(args),
        "run_action_sequence" => tool_run_action_sequence(args),

        // Display tools
        "move_to_monitor" => tool_move_to_monitor(args),
        "set_window_opacity" => tool_set_window_opacity(args),
        "highlight_point" => tool_highlight_point(args),

        // Annotation & image tools
        "annotate_screenshot" => tool_annotate_screenshot(args),
        "ocr_region" => tool_ocr_region(args),
        "find_color_on_screen" => tool_find_color_on_screen(args),
        "find_image_on_screen" => tool_find_image_on_screen(args),

        // System tools
        "read_registry" => tool_read_registry(args),
        "click_tray_icon" => tool_click_tray_icon(args),
        "watch_window" => tool_watch_window(args),

        // App scripting
        "execute_app_script" => tool_execute_app_script(args),

        // Notifications
        "send_notification" => tool_send_notification(args),

        // Status overlay
        "show_status_overlay" => tool_show_status_overlay(args),
        "update_status_overlay" => tool_update_status_overlay(args),
        "hide_status_overlay" => tool_hide_status_overlay(args),

        // Audio tools
        "get_system_volume" => tool_get_system_volume(args),
        "set_system_volume" => tool_set_system_volume(args),
        "set_system_mute" => tool_set_system_mute(args),
        "list_audio_devices" => tool_list_audio_devices(args),

        // Extended clipboard
        "clear_clipboard" => tool_clear_clipboard(args),
        "clipboard_file_paths" => tool_clipboard_file_paths(args),
        "clipboard_html" => tool_clipboard_html(args),

        // Window layout
        "save_window_layout" => tool_save_window_layout(args),
        "restore_window_layout" => tool_restore_window_layout(args),

        // Process monitoring
        "wait_for_process_exit" => tool_wait_for_process_exit(args),
        "get_process_tree" => tool_get_process_tree(args),
        "get_system_metrics" => tool_get_system_metrics(args),

        // Notifications
        "wait_for_notification" => tool_wait_for_notification(args),
        "dismiss_all_notifications" => tool_dismiss_all_notifications(args),

        // Screen recording
        "start_screen_recording" => tool_start_screen_recording(args),
        "stop_screen_recording" => tool_stop_screen_recording(args),
        "capture_gif" => tool_capture_gif(args),

        // Dialog auto-handler
        "dialog_handler_start" => tool_dialog_handler_start(args),
        "dialog_handler_stop" => tool_dialog_handler_stop(args),

        _ => return None,
    };

    Some(finalize_desktop_result(name, args, started_at, result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::helpers::*;
    use super::trace::*;

    #[test]
    fn test_parse_key_combo_single() {
        let (mods, key) = parse_key_combo("enter").unwrap();
        assert!(mods.is_empty());
        assert!(matches!(key, enigo::Key::Return));
    }

    #[test]
    fn test_parse_key_combo_with_modifier() {
        let (mods, key) = parse_key_combo("ctrl+c").unwrap();
        assert_eq!(mods.len(), 1);
        assert!(matches!(mods[0], enigo::Key::Control));
        assert!(matches!(key, enigo::Key::Unicode('c')));
    }

    #[test]
    fn test_parse_key_combo_multiple_modifiers() {
        let (mods, key) = parse_key_combo("ctrl+shift+s").unwrap();
        assert_eq!(mods.len(), 2);
        assert!(matches!(mods[0], enigo::Key::Control));
        assert!(matches!(mods[1], enigo::Key::Shift));
        assert!(matches!(key, enigo::Key::Unicode('s')));
    }

    #[test]
    fn test_parse_key_combo_fkey() {
        let (mods, key) = parse_key_combo("alt+f4").unwrap();
        assert_eq!(mods.len(), 1);
        assert!(matches!(mods[0], enigo::Key::Alt));
        assert!(matches!(key, enigo::Key::F4));
    }

    #[test]
    fn test_parse_key_combo_unknown() {
        assert!(parse_key_combo("ctrl+unknownkey").is_err());
    }

    #[test]
    fn test_str_to_key_special() {
        assert!(matches!(str_to_key("tab"), Ok(enigo::Key::Tab)));
        assert!(matches!(str_to_key("escape"), Ok(enigo::Key::Escape)));
        assert!(matches!(str_to_key("backspace"), Ok(enigo::Key::Backspace)));
        assert!(matches!(str_to_key("space"), Ok(enigo::Key::Space)));
    }

    #[test]
    fn test_str_to_key_char() {
        assert!(matches!(str_to_key("a"), Ok(enigo::Key::Unicode('a'))));
        assert!(matches!(str_to_key("1"), Ok(enigo::Key::Unicode('1'))));
    }

    #[test]
    fn test_validate_coordinates_on_screen() {
        // (0,0) should always be valid — it's the top-left of the primary monitor
        assert!(validate_coordinates(0, 0).is_ok());
    }

    #[test]
    fn test_validate_coordinates_off_screen() {
        // Extremely negative coords should be invalid
        assert!(validate_coordinates(-99999, -99999).is_err());
    }

    #[test]
    fn test_with_enigo_caches_instance() {
        // First call creates the instance
        let r1 = with_enigo(|_e| Ok::<_, String>(42));
        assert_eq!(r1.unwrap(), 42);
        // Second call reuses it (no error from re-init)
        let r2 = with_enigo(|_e| Ok::<_, String>(99));
        assert_eq!(r2.unwrap(), 99);
    }

    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    #[test]
    fn test_dpi_scaling_noop_when_false() {
        let (x, y) = apply_dpi_scaling(100, 200, false);
        assert_eq!((x, y), (100, 200));
    }

    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    #[test]
    fn test_dpi_scaling_applies_when_true() {
        let (x, y) = apply_dpi_scaling(100, 200, true);
        // At any DPI >= 96, the scaled values should be >= input values
        assert!(x >= 100);
        assert!(y >= 200);
    }

    #[test]
    fn test_screenshot_cache_empty() {
        // Cache starts empty
        assert!(get_cached_screenshot(1000).is_none());
    }

    #[test]
    fn test_screenshot_cache_roundtrip() {
        let raw = vec![1, 2, 3, 4];
        let png = vec![5, 6, 7, 8];
        update_screenshot_cache(raw.clone(), png.clone());
        let cached = get_cached_screenshot(5000);
        assert_eq!(cached, Some((Arc::new(raw), Arc::new(png))));
    }

    #[test]
    fn test_pixel_diff_identical() {
        let data = vec![100u8; 256 * 4];
        assert!(pixel_diff_pct(&data, &data) < 0.01);
    }

    #[test]
    fn test_pixel_diff_completely_different() {
        let a = vec![0u8; 256 * 4];
        let b = vec![255u8; 256 * 4];
        assert!(pixel_diff_pct(&a, &b) > 99.0);
    }

    #[test]
    fn test_pixel_diff_different_lengths() {
        let a = vec![0u8; 100];
        let b = vec![0u8; 200];
        assert_eq!(pixel_diff_pct(&a, &b), 100.0);
    }

    #[test]
    fn test_classify_desktop_result_status() {
        assert_eq!(classify_desktop_result_status("Pressed: enter"), "completed");
        assert_eq!(classify_desktop_result_status("Timeout: no window"), "timed_out");
        assert_eq!(classify_desktop_result_status("Error [ocr_screen]: Operation timed out"), "timed_out");
        assert_eq!(classify_desktop_result_status("Error [ocr_screen]: Operation cancelled"), "cancelled");
        assert_eq!(classify_desktop_result_status("Error [click_screen]: bad coordinate"), "error");
        assert_eq!(
            classify_desktop_result_status(
                "Pressed: enter. Verification failed: expected screen change >= 0.50%, observed 0.00% after 1200ms."
            ),
            "verification_failed"
        );
    }

    #[test]
    fn test_summarize_value_for_trace_truncates_long_strings() {
        let value = serde_json::json!({
            "text": "x".repeat(250),
            "nested": ["short", "y".repeat(250)]
        });

        let summarized = summarize_value_for_trace(&value);
        let text = summarized.get("text").and_then(|v| v.as_str()).unwrap();
        assert!(text.len() <= 203);
        assert!(text.ends_with("..."));
    }

    #[test]
    fn test_parse_float_handles_strings() {
        assert_eq!(parse_float(&serde_json::json!(1.5)), Some(1.5));
        assert_eq!(parse_float(&serde_json::json!("2.25")), Some(2.25));
        assert_eq!(parse_float(&serde_json::json!("nope")), None);
    }

    #[test]
    fn test_normalize_verification_region_rejects_zero_size() {
        assert_eq!(normalize_verification_region(10, 10, 0, 10), None);
        assert_eq!(normalize_verification_region(10, 10, 10, 0), None);
    }

    #[test]
    fn test_action_verification_region_from_args() {
        let args = serde_json::json!({
            "verify_x": 100,
            "verify_y": 200,
            "verify_width": 300,
            "verify_height": 400
        });
        assert_eq!(
            action_verification_region_from_args(&args),
            Some(VerificationRegion {
                x: 100,
                y: 200,
                width: 300,
                height: 400
            })
        );
    }

    // ─── Round 6: tool_error / tool_not_supported format tests ───────────

    #[test]
    fn test_tool_error_format() {
        let r = tool_error("click_screen", "bad coordinate");
        assert_eq!(r.text, "Error [click_screen]: bad coordinate");
        assert!(r.images.is_empty());
    }

    #[test]
    fn test_tool_error_with_format_string() {
        let r = tool_error("ocr_screen", format!("monitor {} out of range", 3));
        assert_eq!(r.text, "Error [ocr_screen]: monitor 3 out of range");
    }

    #[test]
    fn test_tool_not_supported_format() {
        let r = tool_not_supported("handle_dialog");
        assert_eq!(r.text, "Error [handle_dialog]: not available on this platform");
    }

    // ─── Round 6: validated_monitors tests ───────────────────────────────

    #[test]
    fn test_validated_monitors_index_zero_ok() {
        // Index 0 should always succeed on a machine with a monitor
        let result = validated_monitors("test_tool", 0);
        assert!(result.is_ok());
        let monitors = result.unwrap();
        assert!(!monitors.is_empty());
    }

    #[test]
    fn test_validated_monitors_out_of_range() {
        let result = validated_monitors("test_tool", 999);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.text.contains("monitor 999 out of range"));
        assert!(err.text.starts_with("Error [test_tool]:"));
    }

    // ─── Round 6: verify_text in prepare_screen_verification ─────────────

    #[test]
    fn test_prepare_verification_no_flags_returns_none() {
        let args = serde_json::json!({});
        let result = prepare_screen_verification(&args, None);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_prepare_verification_verify_text_enables_verification() {
        // verify_text alone should enable verification (no verify_screen_change needed)
        let args = serde_json::json!({
            "verify_text": "Hello World"
        });
        let result = prepare_screen_verification(&args, None);
        assert!(result.is_ok());
        let ctx = result.unwrap();
        assert!(ctx.is_some(), "verify_text should enable verification context");
        let ctx = ctx.unwrap();
        assert_eq!(ctx.expected_text, Some("Hello World".to_string()));
    }

    #[test]
    fn test_prepare_verification_screen_change_without_text() {
        let args = serde_json::json!({
            "verify_screen_change": true
        });
        let result = prepare_screen_verification(&args, None);
        assert!(result.is_ok());
        let ctx = result.unwrap();
        assert!(ctx.is_some(), "verify_screen_change=true should enable verification");
        assert_eq!(ctx.unwrap().expected_text, None);
    }

    // ─── Round 6: classify_desktop_result_status error format ────────────

    #[test]
    fn test_classify_result_status_new_tool_error_format() {
        // Verify the standardized Error [tool]: msg format is correctly classified
        assert_eq!(
            classify_desktop_result_status("Error [smart_wait]: 'text' is required when mode is 'all'"),
            "error"
        );
        assert_eq!(
            classify_desktop_result_status("Error [click_and_verify]: 'click_text' is required"),
            "error"
        );
    }

    // ─── Round 6: dispatch covers new Round 6 tools ──────────────────────

    #[test]
    fn test_dispatch_smart_wait_exists() {
        let dummy = serde_json::json!({});
        assert!(dispatch_desktop_tool("smart_wait", &dummy).is_some());
    }

    #[test]
    fn test_dispatch_click_and_verify_exists() {
        let dummy = serde_json::json!({});
        assert!(dispatch_desktop_tool("click_and_verify", &dummy).is_some());
    }

    // ─── Round 7: parse_timeout ─────────────────────────────────────────

    #[test]
    fn test_parse_timeout_default() {
        let args = serde_json::json!({});
        let dur = parse_timeout(&args);
        assert_eq!(dur, DEFAULT_THREAD_TIMEOUT);
    }

    #[test]
    fn test_parse_timeout_custom() {
        let args = serde_json::json!({"timeout_ms": 5000});
        let dur = parse_timeout(&args);
        assert_eq!(dur, std::time::Duration::from_millis(5000));
    }

    #[test]
    fn test_parse_timeout_clamp_low() {
        let args = serde_json::json!({"timeout_ms": 100});
        let dur = parse_timeout(&args);
        assert_eq!(dur, std::time::Duration::from_millis(1000));
    }

    #[test]
    fn test_parse_timeout_clamp_high() {
        let args = serde_json::json!({"timeout_ms": 999999});
        let dur = parse_timeout(&args);
        assert_eq!(dur, std::time::Duration::from_millis(60000));
    }

    // ─── Round 7: cached_monitors ───────────────────────────────────────

    #[test]
    fn test_cached_monitors_returns_list() {
        let result = cached_monitors();
        // Should succeed on any system with at least one display
        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }

    #[test]
    fn test_cached_monitors_second_call_uses_cache() {
        // First call populates cache
        let _ = cached_monitors();
        // Second call should also succeed (from cache)
        let result = cached_monitors();
        assert!(result.is_ok());
    }
}
