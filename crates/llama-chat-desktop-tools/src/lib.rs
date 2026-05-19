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

// ─── Cancellation / abort ────────────────────────────────────────────────────

mod cancel;
pub use cancel::{
    check_desktop_abort, current_desktop_cancellation_context, desktop_call_cancelled,
    desktop_cancel_error, ensure_desktop_not_cancelled, interruptible_sleep, set_desktop_abort,
    spawn_with_timeout, with_desktop_cancellation_context, DesktopCancellationContext,
};

// ─── Screenshot implementation ───────────────────────────────────────────────

mod screenshot_impl;
pub use screenshot_impl::tool_take_screenshot_with_image;

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
mod tests;
