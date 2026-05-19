use serde_json::{json, Value};

/// Core tool names always included in the system prompt.
pub(crate) const CORE_TOOL_NAMES: &[&str] = &[
    "read_file", "write_file", "edit_file", "undo_edit", "insert_text",
    "search_files", "find_files", "lsp_query", "execute_python", "execute_command",
    "list_directory",
    "git_status", "git_diff", "git_commit",
    "check_background_process", "list_background_processes",
    "send_telegram", "spawn_agent",
    "list_skills", "use_skill", "set_response_style",
    "ocr_screen",
    "browser_navigate", "browser_click", "browser_type", "browser_eval",
    "browser_get_html", "browser_screenshot", "browser_wait", "browser_close",
    "browser_get_text", "browser_get_links", "browser_snapshot",
    "browser_scroll", "browser_press_key",
];

/// Admin tool names (MCP server management).
pub(crate) const ADMIN_TOOL_NAMES: &[&str] = &[
    "list_mcp_servers", "add_mcp_server", "remove_mcp_server",
];

/// Desktop tool names exposed via the MCP server.
#[allow(dead_code)]
pub(crate) const DESKTOP_TOOL_NAMES: &[&str] = &[
    "take_screenshot", "click_screen", "type_text", "press_key", "move_mouse",
    "scroll_screen", "mouse_drag", "mouse_button", "paste", "clear_field", "hover_element",
    "screenshot_region", "screenshot_diff", "window_screenshot", "wait_for_screen_change",
    "ocr_screen", "ocr_find_text", "get_ui_tree", "click_ui_element", "invoke_ui_action",
    "read_ui_element_value", "wait_for_ui_element", "clipboard_image", "find_ui_elements",
    "list_windows", "get_active_window", "focus_window", "minimize_window", "maximize_window",
    "close_window", "resize_window", "wait_for_window", "click_window_relative", "snap_window",
    "set_window_topmost", "open_application", "list_processes", "kill_process",
    "send_keys_to_window", "switch_virtual_desktop", "get_process_info",
    "read_clipboard", "write_clipboard", "get_cursor_position", "get_pixel_color", "list_monitors",
    "find_and_click_text", "type_into_element", "get_window_text", "file_dialog_navigate",
    "drag_and_drop_element", "wait_for_text_on_screen", "get_context_menu", "scroll_element",
    "smart_wait", "click_and_verify",
    "handle_dialog", "wait_for_element_state", "fill_form", "run_action_sequence",
    "move_to_monitor", "set_window_opacity", "highlight_point",
    "annotate_screenshot", "ocr_region", "find_color_on_screen", "find_image_on_screen",
    "read_registry", "click_tray_icon", "watch_window", "execute_app_script",
    "send_notification",
    "show_status_overlay", "update_status_overlay", "hide_status_overlay",
    "get_system_volume", "set_system_volume", "set_system_mute", "list_audio_devices",
    "clear_clipboard", "clipboard_file_paths", "clipboard_html",
    "save_window_layout", "restore_window_layout",
    "wait_for_process_exit", "get_process_tree", "get_system_metrics",
    "wait_for_notification", "dismiss_all_notifications",
    "start_screen_recording", "stop_screen_recording", "capture_gif",
    "dialog_handler_start", "dialog_handler_stop",
];

pub(crate) fn desktop_tool_available_on_current_platform(name: &str) -> bool {
    if !DESKTOP_TOOL_NAMES.contains(&name) {
        return true;
    }

    #[cfg(windows)]
    {
        true
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        !matches!(
            name,
            "get_ui_tree"
                | "click_ui_element"
                | "invoke_ui_action"
                | "read_ui_element_value"
                | "wait_for_ui_element"
                | "find_ui_elements"
                | "file_dialog_navigate"
                | "get_context_menu"
                | "handle_dialog"
                | "wait_for_element_state"
                | "fill_form"
                | "hover_element"
                | "move_to_monitor"
                | "set_window_opacity"
                | "dialog_handler_start"
        )
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        false
    }
}

/// Get ALL tools (core + desktop + admin). Used internally for tool lookup and MCP server.
pub fn get_all_tools() -> Vec<Value> {
    crate::tool_defs::all_tool_definitions()
        .into_iter()
        .filter(|tool| {
            tool.get("name")
                .and_then(|name| name.as_str())
                .map(desktop_tool_available_on_current_platform)
                .unwrap_or(true)
        })
        .collect()
}

/// Get available tools for the template context — core tools + catalog tools only.
pub fn get_available_tools() -> Vec<Value> {
    let mut tools: Vec<Value> = get_all_tools()
        .into_iter()
        .filter(|tool| {
            tool.get("name")
                .and_then(|n| n.as_str())
                .map_or(false, |name| CORE_TOOL_NAMES.contains(&name))
        })
        .collect();

    tools.push(json!({
        "name": "list_tools",
        "description": "List available tools in a category. Categories: 'desktop' (screen automation, mouse, keyboard, windows, OCR, clipboard), 'mcp' (connected MCP server tools), 'admin' (MCP server management). Returns tool names with brief descriptions.",
        "parameters": {
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "description": "Tool category: 'desktop', 'mcp', or 'admin'"
                }
            },
            "required": ["category"]
        }
    }));
    tools.push(json!({
        "name": "get_tool_details",
        "description": "Get the full parameter schema for a specific tool. Use after list_tools to see the exact parameters for a tool you want to use.",
        "parameters": {
            "type": "object",
            "properties": {
                "tool_name": {
                    "type": "string",
                    "description": "Name of the tool to get details for"
                }
            },
            "required": ["tool_name"]
        }
    }));

    tools
}

/// Get only the desktop automation tool definitions (for the MCP server).
#[allow(dead_code)]
pub fn get_desktop_tool_definitions() -> Vec<Value> {
    get_all_tools()
        .into_iter()
        .filter(|tool| {
            tool.get("name")
                .and_then(|n| n.as_str())
                .map_or(false, |name| DESKTOP_TOOL_NAMES.contains(&name))
        })
        .collect()
}

/// Get brief descriptions for tools in a category.
pub fn get_tool_catalog(category: &str) -> String {
    let all_tools = get_all_tools();
    let filter_names: &[&str] = match category {
        "desktop" => DESKTOP_TOOL_NAMES,
        "admin" => ADMIN_TOOL_NAMES,
        "mcp" => {
            return "MCP tools are dynamically loaded from connected servers. Use list_mcp_servers to see connected servers and their tools.".to_string();
        }
        _ => return format!("Unknown category '{}'. Valid categories: 'desktop', 'mcp', 'admin'", category),
    };

    let mut lines = Vec::new();
    for tool in &all_tools {
        let name = match tool.get("name").and_then(|n| n.as_str()) {
            Some(n) => n,
            None => continue,
        };
        if !filter_names.contains(&name) {
            continue;
        }
        let desc = tool.get("description").and_then(|d| d.as_str()).unwrap_or("");
        let brief = match desc.find('.') {
            Some(pos) => &desc[..=pos],
            None => desc,
        };
        lines.push(format!("{}: {}", name, brief));
    }

    if lines.is_empty() {
        format!("No tools found in category '{}'.", category)
    } else {
        lines.join("\n")
    }
}

/// Get full JSON schema for a specific tool by name.
pub fn get_tool_schema(tool_name: &str) -> Option<String> {
    get_all_tools()
        .into_iter()
        .find(|tool| {
            tool.get("name").and_then(|n| n.as_str()) == Some(tool_name)
        })
        .map(|tool| serde_json::to_string_pretty(&tool).unwrap_or_else(|_| tool.to_string()))
}
