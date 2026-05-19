//! Dispatch handlers for desktop automation tools.

use crate::NativeToolResult;

/// Dispatch a desktop tool call by name. Returns `Some(result)` if the name
/// matches a desktop tool, `None` otherwise.
pub(super) fn dispatch_desktop_tool(name: &str, args: &serde_json::Value) -> Option<NativeToolResult> {
    if name == "click_screen" {
        return Some(llama_chat_desktop_tools::tool_click_screen(args));
    }
    if name == "type_text" {
        return Some(llama_chat_desktop_tools::tool_type_text(args));
    }
    if name == "press_key" {
        return Some(llama_chat_desktop_tools::tool_press_key(args));
    }
    if name == "move_mouse" {
        return Some(llama_chat_desktop_tools::tool_move_mouse(args));
    }
    if name == "scroll_screen" {
        return Some(llama_chat_desktop_tools::tool_scroll_screen(args));
    }
    if name == "list_windows" {
        return Some(llama_chat_desktop_tools::tool_list_windows(args));
    }
    if name == "mouse_drag" {
        return Some(llama_chat_desktop_tools::tool_mouse_drag(args));
    }
    if name == "get_cursor_position" {
        return Some(llama_chat_desktop_tools::tool_get_cursor_position(args));
    }
    if name == "focus_window" {
        return Some(llama_chat_desktop_tools::tool_focus_window(args));
    }
    if name == "minimize_window" {
        return Some(llama_chat_desktop_tools::tool_minimize_window(args));
    }
    if name == "maximize_window" {
        return Some(llama_chat_desktop_tools::tool_maximize_window(args));
    }
    if name == "close_window" {
        return Some(llama_chat_desktop_tools::tool_close_window(args));
    }
    if name == "read_clipboard" {
        return Some(llama_chat_desktop_tools::tool_read_clipboard(args));
    }
    if name == "write_clipboard" {
        return Some(llama_chat_desktop_tools::tool_write_clipboard(args));
    }
    if name == "resize_window" {
        return Some(llama_chat_desktop_tools::tool_resize_window(args));
    }
    if name == "get_active_window" {
        return Some(llama_chat_desktop_tools::tool_get_active_window(args));
    }
    if name == "wait_for_window" {
        return Some(llama_chat_desktop_tools::tool_wait_for_window(args));
    }
    if name == "get_pixel_color" {
        return Some(llama_chat_desktop_tools::tool_get_pixel_color(args));
    }
    if name == "click_window_relative" {
        return Some(llama_chat_desktop_tools::tool_click_window_relative(args));
    }
    if name == "list_monitors" {
        return Some(llama_chat_desktop_tools::tool_list_monitors(args));
    }
    if name == "screenshot_region" {
        return Some(llama_chat_desktop_tools::tool_screenshot_region(args));
    }
    if name == "screenshot_diff" {
        return Some(llama_chat_desktop_tools::tool_screenshot_diff(args));
    }
    if name == "ocr_screen" {
        return Some(llama_chat_desktop_tools::tool_ocr_screen(args));
    }
    if name == "get_ui_tree" {
        return Some(llama_chat_desktop_tools::tool_get_ui_tree(args));
    }
    if name == "detect_ui_elements" {
        return Some(llama_chat_desktop_tools::yolo_detect::tool_detect_ui_elements(args));
    }
    if name == "ocr_find_text" {
        return Some(llama_chat_desktop_tools::tool_ocr_find_text(args));
    }
    if name == "click_ui_element" {
        return Some(llama_chat_desktop_tools::tool_click_ui_element(args));
    }
    if name == "window_screenshot" {
        return Some(llama_chat_desktop_tools::tool_window_screenshot(args));
    }
    if name == "open_application" {
        return Some(llama_chat_desktop_tools::tool_open_application(args));
    }
    if name == "wait_for_screen_change" {
        return Some(llama_chat_desktop_tools::tool_wait_for_screen_change(args));
    }
    if name == "set_window_topmost" {
        return Some(llama_chat_desktop_tools::tool_set_window_topmost(args));
    }
    if name == "invoke_ui_action" {
        return Some(llama_chat_desktop_tools::tool_invoke_ui_action(args));
    }
    if name == "read_ui_element_value" {
        return Some(llama_chat_desktop_tools::tool_read_ui_element_value(args));
    }
    if name == "wait_for_ui_element" {
        return Some(llama_chat_desktop_tools::tool_wait_for_ui_element(args));
    }
    if name == "clipboard_image" {
        return Some(llama_chat_desktop_tools::tool_clipboard_image(args));
    }
    if name == "find_ui_elements" {
        return Some(llama_chat_desktop_tools::tool_find_ui_elements(args));
    }
    if name == "execute_app_script" {
        return Some(llama_chat_desktop_tools::tool_execute_app_script(args));
    }
    if name == "send_keys_to_window" {
        return Some(llama_chat_desktop_tools::tool_send_keys_to_window(args));
    }
    if name == "snap_window" {
        return Some(llama_chat_desktop_tools::tool_snap_window(args));
    }
    if name == "list_processes" {
        return Some(llama_chat_desktop_tools::tool_list_processes(args));
    }
    if name == "kill_process" {
        return Some(llama_chat_desktop_tools::tool_kill_process(args));
    }
    if name == "find_and_click_text" {
        return Some(llama_chat_desktop_tools::tool_find_and_click_text(args));
    }
    if name == "type_into_element" {
        return Some(llama_chat_desktop_tools::tool_type_into_element(args));
    }
    if name == "get_window_text" {
        return Some(llama_chat_desktop_tools::tool_get_window_text(args));
    }
    if name == "file_dialog_navigate" {
        return Some(llama_chat_desktop_tools::tool_file_dialog_navigate(args));
    }
    if name == "drag_and_drop_element" {
        return Some(llama_chat_desktop_tools::tool_drag_and_drop_element(args));
    }
    if name == "wait_for_text_on_screen" {
        return Some(llama_chat_desktop_tools::tool_wait_for_text_on_screen(args));
    }
    if name == "get_context_menu" {
        return Some(llama_chat_desktop_tools::tool_get_context_menu(args));
    }
    if name == "scroll_element" {
        return Some(llama_chat_desktop_tools::tool_scroll_element(args));
    }
    if name == "mouse_button" {
        return Some(llama_chat_desktop_tools::tool_mouse_button(args));
    }
    if name == "switch_virtual_desktop" {
        return Some(llama_chat_desktop_tools::tool_switch_virtual_desktop(args));
    }
    if name == "find_image_on_screen" {
        return Some(llama_chat_desktop_tools::tool_find_image_on_screen(args));
    }
    if name == "get_process_info" {
        return Some(llama_chat_desktop_tools::tool_get_process_info(args));
    }
    if name == "paste" {
        return Some(llama_chat_desktop_tools::tool_paste(args));
    }
    if name == "clear_field" {
        return Some(llama_chat_desktop_tools::tool_clear_field(args));
    }
    if name == "hover_element" {
        return Some(llama_chat_desktop_tools::tool_hover_element(args));
    }
    if name == "handle_dialog" {
        return Some(llama_chat_desktop_tools::tool_handle_dialog(args));
    }
    if name == "wait_for_element_state" {
        return Some(llama_chat_desktop_tools::tool_wait_for_element_state(args));
    }
    if name == "fill_form" {
        return Some(llama_chat_desktop_tools::tool_fill_form(args));
    }
    if name == "run_action_sequence" {
        return Some(llama_chat_desktop_tools::tool_run_action_sequence(args));
    }
    if name == "move_to_monitor" {
        return Some(llama_chat_desktop_tools::tool_move_to_monitor(args));
    }
    if name == "set_window_opacity" {
        return Some(llama_chat_desktop_tools::tool_set_window_opacity(args));
    }
    if name == "highlight_point" {
        return Some(llama_chat_desktop_tools::tool_highlight_point(args));
    }
    if name == "annotate_screenshot" {
        return Some(llama_chat_desktop_tools::tool_annotate_screenshot(args));
    }
    if name == "ocr_region" {
        return Some(llama_chat_desktop_tools::tool_ocr_region(args));
    }
    if name == "find_color_on_screen" {
        return Some(llama_chat_desktop_tools::tool_find_color_on_screen(args));
    }
    if name == "read_registry" {
        return Some(llama_chat_desktop_tools::tool_read_registry(args));
    }
    if name == "click_tray_icon" {
        return Some(llama_chat_desktop_tools::tool_click_tray_icon(args));
    }
    if name == "watch_window" {
        return Some(llama_chat_desktop_tools::tool_watch_window(args));
    }
    None
}
