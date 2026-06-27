//! Agent management, MCP, messaging, and session tool definitions.

use super::{p, Params, ToolDef};
use serde_json::{json, Value};

pub static AGENT_TOOLS: &[ToolDef] = &[
    // ─── send_telegram ───
    ToolDef {
        name: "send_telegram",
        description: "Send a notification message to the user via Telegram. Use to notify about task completion, errors, or important updates.",
        params: Params::Simple(&[
            p("message", "string", "The message text to send (supports Markdown formatting)"),
        ]),
        required: &["message"],
    },
    // ─── spawn_agent ───
    ToolDef {
        name: "spawn_agent",
        description: "Spawn a sub-agent to handle an isolated sub-task. The agent gets a fresh context and returns a summary of what it did. Use for installation tasks, research, or any step that might use lots of context.",
        params: Params::Simple(&[
            p("task", "string", "The sub-task description for the agent to complete"),
            p("context", "string", "Additional context to provide to the agent (file contents, error messages, etc.)"),
        ]),
        required: &["task"],
    },
    // ─── todo_write ───
    ToolDef {
        name: "todo_write",
        description: "Update the task checklist for this session. Use to track progress on multi-step tasks. Each todo has a status: pending, in_progress, or completed.",
        params: Params::Simple(&[
            p("todos", "string", "JSON array of todos: [{\"id\": 1, \"task\": \"description\", \"status\": \"pending|in_progress|completed\"}]"),
        ]),
        required: &["todos"],
    },
    // ─── todo_read ───
    ToolDef {
        name: "todo_read",
        description: "Read the current task checklist for this session.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── list_skills ───
    ToolDef {
        name: "list_skills",
        description: "List available prompt skills (reusable templates). Skills are .md files in the skills/ directory.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── use_skill ───
    ToolDef {
        name: "use_skill",
        description: "Execute a skill (prompt template) by name. The skill's content becomes your instructions.",
        params: Params::Simple(&[
            p("name", "string", "Skill name to execute"),
            p("args", "string", "Arguments to substitute in the template (JSON object, e.g. {\"language\": \"python\", \"path\": \"./myapp\"})"),
        ]),
        required: &["name"],
    },
    // ─── set_response_style ───
    ToolDef {
        name: "set_response_style",
        description: "Switch between brief and detailed response styles. Use 'brief' for short, action-focused responses (less explanation). Use 'detailed' for thorough explanations.",
        params: Params::Simple(&[
            p("style", "string", "Response style: 'brief' or 'detailed'"),
        ]),
        required: &["style"],
    },
    // ─── list_mcp_servers ───
    ToolDef {
        name: "list_mcp_servers",
        description: "List all configured MCP (Model Context Protocol) servers with their connection status and available tools.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── remove_mcp_server ───
    ToolDef {
        name: "remove_mcp_server",
        description: "Remove an MCP server by name. This disconnects the server and removes its configuration.",
        params: Params::Simple(&[
            p("name", "string", "Name of the MCP server to remove"),
        ]),
        required: &["name"],
    },
];

/// Complex agent tools that require runtime JSON construction.
pub fn complex_agent_tools() -> Vec<Value> {
    vec![
        // ─── parallel_execute — has array param ───
        json!({
            "name": "parallel_execute",
            "description": "Execute multiple independent tool calls in parallel and receive all results at once. Use when you have several operations that don't depend on each other's results (e.g. writing multiple files, reading multiple files, fetching multiple URLs). Results are returned together once all complete. Do NOT include execute_command, spawn_agent, or nested parallel_execute calls.",
            "parameters": {
                "type": "object",
                "properties": {
                    "calls": {
                        "type": "array",
                        "description": "Tool calls to run in parallel. Each item must have 'tool' (tool name string) and 'args' (object with tool arguments).",
                        "items": {
                            "type": "object",
                            "properties": {
                                "tool": { "type": "string", "description": "Name of the tool to call" },
                                "args": { "type": "object", "description": "Arguments for the tool" }
                            },
                            "required": ["tool", "args"]
                        },
                        "minItems": 2,
                        "maxItems": 10
                    }
                },
                "required": ["calls"]
            }
        }),
        // ─── add_mcp_server — has array and object params ───
        json!({
            "name": "add_mcp_server",
            "description": "Add a new MCP server to extend your capabilities with external tools. Supports stdio (command-based) and http transports. New tools become available in the next message.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Display name for the MCP server" },
                    "transport": { "type": "string", "description": "Transport type: 'stdio' (default) or 'http'" },
                    "command": { "type": "string", "description": "Command to run (required for stdio transport, e.g. 'npx', 'uvx', 'node')" },
                    "args": { "type": "array", "items": { "type": "string" }, "description": "Command arguments (for stdio transport, e.g. ['-y', '@anthropic/mcp-server'])" },
                    "url": { "type": "string", "description": "Server URL (required for http transport)" },
                    "env_vars": { "type": "object", "description": "Environment variables to set for the server process (e.g. {\"API_KEY\": \"xxx\"})" }
                },
                "required": ["name"]
            }
        }),
        // ─── dialog_handler_start — has object param ───
        json!({
            "name": "dialog_handler_start",
            "description": "Start a background monitor that auto-clicks dialog buttons matching a button map. Useful for dismissing expected popups during automated workflows.",
            "parameters": {
                "type": "object",
                "properties": {
                    "button_map": { "type": "object", "description": "Map of button names to actions, e.g. {\"OK\": \"click\", \"Cancel\": \"click\"}" },
                    "poll_interval_ms": { "type": "integer", "description": "Polling interval in ms (default 1000)" },
                    "timeout_ms": { "type": "integer", "description": "Auto-stop after this many ms (default 60000)" }
                },
                "required": ["button_map"]
            }
        }),
    ]
}
