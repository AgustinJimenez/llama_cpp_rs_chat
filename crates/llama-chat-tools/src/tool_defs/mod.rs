//! Tool definitions — thin assembler that merges all category modules.
//!
//! Tool data lives in `tool_defs_categories/` split by domain:
//!   - `file_tools`          — file I/O, search, execute_command, lsp_query
//!   - `browser_tools`       — browser_*, open_url, open_browser_view, close_browser_view
//!   - `screenshot_tools`    — screenshots, OCR, screen recording, GIF
//!   - `window_tools`        — window management, get_ui_tree, fill_form, run_action_sequence
//!   - `input_tools`         — click_screen, type_text, press_key, scroll, drag (with verify params)
//!   - `clipboard_tools`     — clipboard read/write/html/files
//!   - `ui_automation_tools` — UIA interactions, OCR-click combos
//!   - `system_tools`        — processes, audio, registry, notifications, git, sleep
//!   - `agent_tools`         — MCP, spawn_agent, todo, skills, add_mcp_server

mod tool_defs_categories;

use serde_json::Value;
use tool_defs_categories::{
    agent_tools, browser_tools, clipboard_tools, file_tools, input_tools, screenshot_tools,
    system_tools, ui_automation_tools, window_tools,
};

// ─── Verification helper (re-exported for tests) ────────────────────────────
#[cfg(test)]
pub const EXPECTED_TOOL_COUNT: usize = 147;

// ─── Public API ─────────────────────────────────────────────────────────────

/// Build the complete list of all tool definitions.
///
/// Merges compact static definitions with runtime-built complex tools.
/// Complex tools override same-named simple ones (dedup by name).
pub fn all_tool_definitions() -> Vec<Value> {
    // --- Simple (static) tools ---
    let simple_slices: &[&[tool_defs_categories::ToolDef]] = &[
        file_tools::FILE_TOOLS,
        browser_tools::BROWSER_TOOLS,
        screenshot_tools::SCREENSHOT_TOOLS,
        window_tools::WINDOW_TOOLS,
        input_tools::SIMPLE_INPUT_TOOLS,
        clipboard_tools::CLIPBOARD_TOOLS,
        ui_automation_tools::UI_AUTOMATION_TOOLS,
        system_tools::SYSTEM_TOOLS,
        agent_tools::AGENT_TOOLS,
    ];

    let mut tools: Vec<Value> = simple_slices
        .iter()
        .flat_map(|s| s.iter().map(|t| t.to_json()))
        .collect();

    // --- Complex (runtime-built) tools ---
    let mut complex: Vec<Value> = Vec::new();
    complex.extend(screenshot_tools::complex_screenshot_tools());
    complex.extend(window_tools::complex_window_tools());
    complex.extend(input_tools::complex_input_tools());
    complex.extend(clipboard_tools::complex_clipboard_tools());
    complex.extend(agent_tools::complex_agent_tools());

    // Remove simple tools overridden by complex versions
    let complex_names: Vec<&str> = complex
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
        .collect();
    tools.retain(|t| {
        let name = t.get("name").and_then(|n| n.as_str()).unwrap_or("");
        !complex_names.contains(&name)
    });

    tools.extend(complex);
    tools
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_tool_count() {
        let tools = all_tool_definitions();
        assert_eq!(
            tools.len(),
            EXPECTED_TOOL_COUNT,
            "Expected {} tools, got {}. Tool names: {:?}",
            EXPECTED_TOOL_COUNT,
            tools.len(),
            tools
                .iter()
                .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_no_duplicate_names() {
        let tools = all_tool_definitions();
        let mut seen = HashSet::new();
        for tool in &tools {
            let name = tool.get("name").and_then(|n| n.as_str()).unwrap();
            assert!(seen.insert(name), "Duplicate tool name: {}", name);
        }
    }

    #[test]
    fn test_all_tools_have_required_fields() {
        let tools = all_tool_definitions();
        for tool in &tools {
            assert!(tool.get("name").is_some(), "Tool missing name: {:?}", tool);
            assert!(
                tool.get("description").is_some(),
                "Tool missing description: {:?}",
                tool
            );
            assert!(
                tool.get("parameters").is_some(),
                "Tool missing parameters: {:?}",
                tool
            );
            let params = tool.get("parameters").unwrap();
            assert_eq!(params.get("type").and_then(|t| t.as_str()), Some("object"));
            assert!(params.get("properties").is_some());
            assert!(params.get("required").is_some());
        }
    }

    #[test]
    fn test_click_screen_has_verify_params() {
        let tools = all_tool_definitions();
        let click = tools.iter().find(|t| t["name"] == "click_screen").unwrap();
        let props = click["parameters"]["properties"].as_object().unwrap();
        assert!(props.contains_key("verify_screen_change"));
        assert!(props.contains_key("verify_text"));
        assert!(props.contains_key("x"));
        assert!(props.contains_key("y"));
    }
}
