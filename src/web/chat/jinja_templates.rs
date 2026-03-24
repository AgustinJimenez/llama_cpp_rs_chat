use minijinja::{context, Environment, Error, ErrorKind};
use serde_json::{json, Value};

/// Preprocess a Jinja2 template string for minijinja compatibility.
///
/// Fixes Python-specific syntax that minijinja doesn't support:
/// - `tojson(ensure_ascii=False)` → `tojson` (minijinja doesn't escape non-ASCII by default)
/// - `.endswith("x")` → ` is endingwith("x")` (Python method → minijinja test)
/// - `.startswith("x")` → ` is startingwith("x")` (Python method → minijinja test)
/// - `.strip()` → ` | trim` (Python method → minijinja filter)
/// - `.items()` → ` | items` (Python dict method → minijinja filter)
fn preprocess_template(template: &str) -> String {
    use regex::Regex;

    let mut result = template
        .replace("tojson(ensure_ascii=False)", "tojson")
        .replace("tojson(ensure_ascii=True)", "tojson");

    // Convert .endswith("x") → is endingwith("x")
    // Handles: expr.endswith("...") or expr.endswith('...')
    if let Ok(re) = Regex::new(r"\.endswith\(") {
        result = re.replace_all(&result, " is endingwith(").to_string();
    }

    // Convert .startswith("x") → is startingwith("x")
    if let Ok(re) = Regex::new(r"\.startswith\(") {
        result = re.replace_all(&result, " is startingwith(").to_string();
    }

    // Convert .strip() → | trim (Python str.strip → Jinja trim filter)
    result = result.replace(".strip()", " | trim");

    // Convert .items() → | items (Python dict.items() → minijinja filter)
    // Used by Harmony templates: `for key, val in dict.items()`
    result = result.replace(".items()", " | items");

    result
}

/// Apply native Jinja2 chat template from model metadata
///
/// This function takes the raw Jinja2 template from the model's tokenizer.chat_template
/// and applies it with the provided messages, tools, and documents.
pub fn apply_native_chat_template(
    template_string: &str,
    messages: Vec<ChatMessage>,
    tools: Option<Vec<Value>>,
    documents: Option<Vec<Value>>,
    add_generation_prompt: bool,
    bos_token: &str,
    eos_token: &str,
) -> Result<String, String> {
    // Preprocess template for minijinja compatibility
    let processed_template = preprocess_template(template_string);

    // Create Jinja2 environment
    let mut env = Environment::new();

    // Register custom functions that real GGUF templates use
    // raise_exception(msg) — used by GLM-4.6, Devstral, Ministral for validation
    env.add_function("raise_exception", |msg: String| -> Result<String, Error> {
        Err(Error::new(ErrorKind::InvalidOperation, msg))
    });

    // strftime_now(fmt) — used by Mistral templates for current date
    env.add_function("strftime_now", |fmt: String| -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let secs = now.as_secs() as i64;
        // Simple date formatting without chrono dependency
        if fmt.contains("%Y") || fmt.contains("%m") || fmt.contains("%d") {
            // Convert epoch to YYYY-MM-DD (UTC)
            let days = secs / 86400;
            let (year, month, day) = epoch_days_to_ymd(days);
            fmt.replace("%Y", &format!("{year:04}"))
                .replace("%m", &format!("{month:02}"))
                .replace("%d", &format!("{day:02}"))
        } else {
            // Fallback: return ISO date
            let days = secs / 86400;
            let (year, month, day) = epoch_days_to_ymd(days);
            format!("{year:04}-{month:02}-{day:02}")
        }
    });

    // Add the template
    env.add_template("chat_template", &processed_template)
        .map_err(|e| format!("Failed to parse chat template: {e}"))?;

    // Prepare context variables that the template expects
    let tools_vec = tools.unwrap_or_default();
    let documents_vec = documents.unwrap_or_default();
    let template_context = context! {
        messages => messages,
        tools => &tools_vec,
        documents => &documents_vec,
        add_generation_prompt => add_generation_prompt,
        // Common Jinja2 template variables
        available_tools => &tools_vec,
        bos_token => bos_token,
        eos_token => eos_token,
        // Disable thinking/reasoning mode — models like GLM-4 check this variable
        // and enter <think> mode if it's undefined, causing immediate EOS
        enable_thinking => false,
    };

    // Render the template
    let template = env.get_template("chat_template")
        .map_err(|e| format!("Failed to get template: {e}"))?;

    template.render(&template_context)
        .map_err(|e| format!("Failed to render template: {e}"))
}

/// Convert epoch days (since 1970-01-01) to (year, month, day).
fn epoch_days_to_ymd(days: i64) -> (i64, u32, u32) {
    // Civil calendar algorithm from Howard Hinnant
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

/// Chat message structure for Jinja2 templates
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub tool_calls: Option<Vec<ToolCall>>,
}

/// Tool call structure for chat templates
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
    pub function: Option<ToolFunction>,
}

/// Tool function structure
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ToolFunction {
    pub name: String,
    pub arguments: String,
}

/// Get available tools in OpenAI function-calling format for Jinja templates.
///
/// Models are trained on OpenAI API format: `{"type": "function", "function": {...}}`.
/// The Jinja templates serialize these via `tools | tojson`, so the format matters
/// for model comprehension.
#[allow(dead_code)]
pub fn get_available_tools_openai() -> Vec<Value> {
    get_available_tools_openai_with_mcp(None)
}

/// Get available tools in OpenAI format, optionally including MCP tools.
pub fn get_available_tools_openai_with_mcp(mcp_tools: Option<&[super::super::mcp::McpToolDef]>) -> Vec<Value> {
    let mut tools: Vec<Value> = get_available_tools()
        .into_iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": tool
            })
        })
        .collect();

    // Append MCP tool definitions
    if let Some(mcp) = mcp_tools {
        for t in mcp {
            tools.push(t.to_openai_function());
        }
    }

    tools
}

/// Parse conversation text into ChatMessage format for Jinja rendering.
///
/// Unlike `parse_conversation_to_messages()`, this version:
/// - Replaces the stored SYSTEM: block with a provided system prompt
/// - Keeps tool calls/responses as inline text in assistant content (phase 1)
pub fn parse_conversation_for_jinja(
    conversation: &str,
    system_prompt: &str,
) -> Vec<ChatMessage> {
    let mut messages = Vec::new();

    // Always start with our behavioral system prompt
    messages.push(ChatMessage {
        role: "system".to_string(),
        content: system_prompt.to_string(),
        tool_calls: None,
    });

    let mut current_role = "";
    let mut current_content = String::new();

    for line in conversation.lines() {
        if line.ends_with(":")
            && (line.starts_with("SYSTEM:")
                || line.starts_with("USER:")
                || line.starts_with("ASSISTANT:"))
        {
            // Save previous role's content (skip SYSTEM — replaced above)
            if !current_role.is_empty()
                && current_role != "SYSTEM"
                && !current_content.trim().is_empty()
            {
                messages.push(ChatMessage {
                    role: current_role.to_lowercase(),
                    content: current_content.trim().to_string(),
                    tool_calls: None,
                });
            }

            current_role = line.trim_end_matches(':');
            current_content.clear();
        } else if !current_role.is_empty() {
            if !current_content.is_empty() {
                current_content.push('\n');
            }
            current_content.push_str(line);
        }
    }

    // Add final message (skip SYSTEM)
    if !current_role.is_empty()
        && current_role != "SYSTEM"
        && !current_content.trim().is_empty()
    {
        messages.push(ChatMessage {
            role: current_role.to_lowercase(),
            content: current_content.trim().to_string(),
            tool_calls: None,
        });
    }

    messages
}

/// Get available tools for the template context
pub fn get_available_tools() -> Vec<Value> {
    let tools = vec![
        json!({
            "name": "read_file",
            "description": "Read the contents of a file. Supports PDF, DOCX, XLSX, PPTX, EPUB, ODT, RTF, CSV, EML, ZIP, and non-UTF8 encoded files. Returns the file text (truncated at 100KB for large files).",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to read"
                    }
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "write_file",
            "description": "Write content to a file. Creates parent directories if needed. Overwrites existing files.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to write the file to"
                    },
                    "content": {
                        "type": "string",
                        "description": "The content to write to the file"
                    }
                },
                "required": ["path", "content"]
            }
        }),
        json!({
            "name": "edit_file",
            "description": "Replace exact text in a file. old_string must match exactly once in the file. Use this for small edits instead of rewriting the whole file with write_file.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to edit"
                    },
                    "old_string": {
                        "type": "string",
                        "description": "Exact text to find in the file (must appear exactly once)"
                    },
                    "new_string": {
                        "type": "string",
                        "description": "Text to replace it with"
                    }
                },
                "required": ["path", "old_string", "new_string"]
            }
        }),
        json!({
            "name": "undo_edit",
            "description": "Revert the last edit_file operation on a file. Restores the file from its backup.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to restore"
                    }
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "insert_text",
            "description": "Insert text at a specific line number in a file. Line is 1-based. The text is inserted before the specified line.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file"
                    },
                    "line": {
                        "type": "integer",
                        "description": "Line number to insert at (1-based)"
                    },
                    "text": {
                        "type": "string",
                        "description": "Text to insert"
                    }
                },
                "required": ["path", "line", "text"]
            }
        }),
        json!({
            "name": "search_files",
            "description": "Search file contents across a directory by regex or literal pattern. Returns matching lines with file paths and line numbers. Use include to filter by file type (e.g. \"*.rs\").",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Regex or literal pattern to search for"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search in (default: current directory)"
                    },
                    "include": {
                        "type": "string",
                        "description": "Glob filter for file names (e.g. \"*.py\", \"*.rs\")"
                    },
                    "context": {
                        "type": "integer",
                        "description": "Number of context lines before/after each match (default: 0)"
                    },
                    "exclude": {
                        "type": "string",
                        "description": "Glob pattern to exclude (e.g. \"*_test.rs\", \"*.generated.*\")"
                    }
                },
                "required": ["pattern"]
            }
        }),
        json!({
            "name": "find_files",
            "description": "Find files by name pattern recursively. Returns a list of matching file paths.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "File name pattern (e.g. \"*.tsx\", \"Cargo.*\", \"README*\")"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search in (default: current directory)"
                    },
                    "exclude": {
                        "type": "string",
                        "description": "Glob pattern to exclude (e.g. \"*.min.js\", \"*_test.*\")"
                    }
                },
                "required": ["pattern"]
            }
        }),
        json!({
            "name": "execute_python",
            "description": "Execute Python code. The code is written to a temp file and run with the Python interpreter. Supports multi-line code, imports, regex, and any valid Python. Returns stdout and stderr.",
            "parameters": {
                "type": "object",
                "properties": {
                    "code": {
                        "type": "string",
                        "description": "The Python code to execute"
                    }
                },
                "required": ["code"]
            }
        }),
        json!({
            "name": "execute_command",
            "description": "Execute a shell command (git, npm, curl, etc.). You MUST set the background flag for every call.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "background": {
                        "type": "boolean",
                        "description": "REQUIRED. Set true for long-running processes (dev servers, watchers, daemons like 'php artisan serve', 'npm run dev', 'python -m http.server'). Set false for everything else (installs, builds, one-shot commands). If true, returns after 5s with initial output and the PID."
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Optional. Max seconds of inactivity (no output) before the command is killed. Default 120 (2 min). Resets every time the command produces output. Use higher values for commands with long silent phases."
                    }
                },
                "required": ["command", "background"]
            }
        }),
        json!({
            "name": "list_directory",
            "description": "List files and directories in a path. Shows name, size, and type for each entry.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path to list (defaults to current directory)"
                    }
                },
                "required": []
            }
        }),
        json!({
            "name": "web_search",
            "description": "Search the web using the configured provider. Returns a list of results with titles, URLs, and descriptions. Use this to find current information, documentation, or answers.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: 8)"
                    }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "web_fetch",
            "description": "Fetch a web page and return its content as plain text (HTML is stripped). Use this to read articles, documentation, or any web page after finding its URL via web_search.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch"
                    },
                    "max_length": {
                        "type": "integer",
                        "description": "Maximum characters to return (default: 15000)"
                    }
                },
                "required": ["url"]
            }
        }),
        json!({
            "name": "open_url",
            "description": "Open a URL in the user's default web browser. Use this to show web apps, documentation, or results to the user. The URL opens in a new browser tab.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to open (must start with http:// or https://)"
                    }
                },
                "required": ["url"]
            }
        }),
        json!({
            "name": "git_status",
            "description": "Show the working tree status. Returns modified, staged, and untracked files.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Repository path (defaults to current directory)"
                    }
                },
                "required": []
            }
        }),
        json!({
            "name": "git_diff",
            "description": "Show git diff. By default shows unstaged changes. Set staged=true for staged changes.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to diff, or omit for all changes"
                    },
                    "staged": {
                        "type": "boolean",
                        "description": "If true, show staged changes instead of unstaged (default: false)"
                    }
                },
                "required": []
            }
        }),
        json!({
            "name": "git_commit",
            "description": "Commit changes with a message. By default commits staged changes only. Use all=true to auto-stage tracked modified files.",
            "parameters": {
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "Commit message"
                    },
                    "all": {
                        "type": "boolean",
                        "description": "If true, auto-stage tracked modified files before committing (git commit -a)"
                    }
                },
                "required": ["message"]
            }
        }),
        json!({
            "name": "check_background_process",
            "description": "Check on a background process launched with execute_command(background=true). Returns whether it is still running and any new output since last check. Use wait_seconds to pause before checking (combines wait + check in one call).",
            "parameters": {
                "type": "object",
                "properties": {
                    "pid": {
                        "type": "integer",
                        "description": "The PID returned by execute_command with background=true"
                    },
                    "wait_seconds": {
                        "type": "integer",
                        "description": "Seconds to wait before checking (1-30). Use this instead of calling wait separately."
                    }
                },
                "required": ["pid"]
            }
        }),
        json!({
            "name": "take_screenshot",
            "description": "Capture a screenshot of the user's screen. Returns the file path and image dimensions. Use monitor=-1 to list available monitors without capturing.",
            "parameters": {
                "type": "object",
                "properties": {
                    "monitor": {
                        "type": "integer",
                        "description": "Monitor index (0=primary, 1,2..=other monitors). Use -1 to list available monitors."
                    }
                },
                "required": []
            }
        }),
        // Desktop automation tools (computer use)
        json!({
            "name": "click_screen",
            "description": "Click the mouse at screen coordinates. Automatically takes a screenshot after clicking so you can see the result. Use take_screenshot first to see the screen and identify coordinates.",
            "parameters": {
                "type": "object",
                "properties": {
                    "x": { "type": "integer", "description": "X coordinate in pixels from left edge of screen" },
                    "y": { "type": "integer", "description": "Y coordinate in pixels from top edge of screen" },
                    "button": { "type": "string", "description": "Mouse button: 'left' (default), 'right', 'middle', 'double' (double left click)" },
                    "delay_ms": { "type": "integer", "description": "Milliseconds to wait after clicking before taking screenshot (default: 500). Increase for slow UI animations." },
                    "dpi_aware": { "type": "boolean", "description": "If true, coordinates are logical (96 DPI basis) and will be scaled to physical pixels by the system DPI factor (default: false)" },
                    "verify_screen_change": { "type": "boolean", "description": "If true, verify that the screen visibly changed after the action before returning." },
                    "verify_threshold_pct": { "type": "number", "description": "Minimum percentage of sampled pixels that must change for verification to pass (default: 0.5)." },
                    "verify_timeout_ms": { "type": "integer", "description": "Maximum time to wait for a visible change when verification is enabled (default: 1200)." },
                    "verify_poll_ms": { "type": "integer", "description": "Polling interval for verification screenshots (default: 150)." },
                    "verify_x": { "type": "integer", "description": "Optional absolute X for a custom verification region." },
                    "verify_y": { "type": "integer", "description": "Optional absolute Y for a custom verification region." },
                    "verify_width": { "type": "integer", "description": "Optional width for a custom verification region." },
                    "verify_height": { "type": "integer", "description": "Optional height for a custom verification region." },
                    "verify_text": { "type": "string", "description": "After action, OCR the verification region and confirm this text appears. Enables verify_screen_change automatically." },
                    "snap_to_screen": { "type": "boolean", "description": "Clamp off-screen coordinates to nearest monitor edge" },
                    "timeout_ms": { "type": "integer", "description": "Operation timeout in ms (1000-60000, default 20000)" }
                },
                "required": ["x", "y"]
            }
        }),
        json!({
            "name": "type_text",
            "description": "Type text using the keyboard. Simulates real keyboard input character by character. Falls back to SendInput Unicode on Windows for non-Latin characters. Use click_screen first to focus the target input field.",
            "parameters": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "The text to type" },
                    "screenshot": { "type": "boolean", "description": "Take a screenshot after typing (default: true)" },
                    "delay_ms": { "type": "integer", "description": "Milliseconds to wait after typing before screenshot (default: 300)" },
                    "verify_screen_change": { "type": "boolean", "description": "If true, verify that the screen visibly changed after typing before returning." },
                    "verify_threshold_pct": { "type": "number", "description": "Minimum percentage of sampled pixels that must change for verification to pass (default: 0.5)." },
                    "verify_timeout_ms": { "type": "integer", "description": "Maximum time to wait for a visible change when verification is enabled (default: 1200)." },
                    "verify_poll_ms": { "type": "integer", "description": "Polling interval for verification screenshots (default: 150)." },
                    "verify_x": { "type": "integer", "description": "Optional absolute X for a custom verification region." },
                    "verify_y": { "type": "integer", "description": "Optional absolute Y for a custom verification region." },
                    "verify_width": { "type": "integer", "description": "Optional width for a custom verification region." },
                    "verify_height": { "type": "integer", "description": "Optional height for a custom verification region." },
                    "verify_text": { "type": "string", "description": "After action, OCR the verification region and confirm this text appears. Enables verify_screen_change automatically." },
                    "retry": { "type": "integer", "description": "Retry count 0-3 on failure (default 0)" },
                    "timeout_ms": { "type": "integer", "description": "Operation timeout in ms (1000-60000, default 20000)" }
                },
                "required": ["text"]
            }
        }),
        json!({
            "name": "press_key",
            "description": "Press a key or key combination. Supports modifiers (ctrl, alt, shift, meta/win) and special keys (enter, tab, escape, backspace, delete, up, down, left, right, home, end, pageup, pagedown, f1-f12, space). For combinations use '+': 'ctrl+c', 'ctrl+shift+s', 'alt+tab', 'alt+f4'.",
            "parameters": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Key or key combination. Examples: 'enter', 'tab', 'ctrl+c', 'ctrl+shift+s', 'alt+tab', 'f5'" },
                    "screenshot": { "type": "boolean", "description": "Take a screenshot after key press (default: true)" },
                    "delay_ms": { "type": "integer", "description": "Milliseconds to wait after key press before screenshot (default: 500)" },
                    "verify_screen_change": { "type": "boolean", "description": "If true, verify that the screen visibly changed after the key press before returning." },
                    "verify_threshold_pct": { "type": "number", "description": "Minimum percentage of sampled pixels that must change for verification to pass (default: 0.5)." },
                    "verify_timeout_ms": { "type": "integer", "description": "Maximum time to wait for a visible change when verification is enabled (default: 1200)." },
                    "verify_poll_ms": { "type": "integer", "description": "Polling interval for verification screenshots (default: 150)." },
                    "verify_x": { "type": "integer", "description": "Optional absolute X for a custom verification region." },
                    "verify_y": { "type": "integer", "description": "Optional absolute Y for a custom verification region." },
                    "verify_width": { "type": "integer", "description": "Optional width for a custom verification region." },
                    "verify_height": { "type": "integer", "description": "Optional height for a custom verification region." },
                    "verify_text": { "type": "string", "description": "After action, OCR the verification region and confirm this text appears. Enables verify_screen_change automatically." },
                    "retry": { "type": "integer", "description": "Retry count 0-3 on failure (default 0)" },
                    "timeout_ms": { "type": "integer", "description": "Operation timeout in ms (1000-60000, default 20000)" }
                },
                "required": ["key"]
            }
        }),
        json!({
            "name": "move_mouse",
            "description": "Move the mouse cursor to screen coordinates without clicking. Does not take a screenshot.",
            "parameters": {
                "type": "object",
                "properties": {
                    "x": { "type": "integer", "description": "X coordinate in pixels from left edge of screen" },
                    "y": { "type": "integer", "description": "Y coordinate in pixels from top edge of screen" }
                },
                "required": ["x", "y"]
            }
        }),
        json!({
            "name": "scroll_screen",
            "description": "Scroll the mouse wheel at the current or specified position. Positive amount scrolls down, negative scrolls up. Each unit is about 3 lines of text.",
            "parameters": {
                "type": "object",
                "properties": {
                    "amount": { "type": "integer", "description": "Scroll amount: positive = down, negative = up. Each unit is ~3 lines." },
                    "x": { "type": "integer", "description": "X coordinate to scroll at (optional, uses current position if omitted)" },
                    "y": { "type": "integer", "description": "Y coordinate to scroll at (optional, uses current position if omitted)" },
                    "horizontal": { "type": "boolean", "description": "Scroll horizontally instead of vertically (default: false)" },
                    "screenshot": { "type": "boolean", "description": "Take a screenshot after scrolling (default: true)" },
                    "delay_ms": { "type": "integer", "description": "Milliseconds to wait after scrolling before screenshot (default: 300)" },
                    "verify_screen_change": { "type": "boolean", "description": "If true, verify that the screen visibly changed after scrolling before returning." },
                    "verify_threshold_pct": { "type": "number", "description": "Minimum percentage of sampled pixels that must change for verification to pass (default: 0.5)." },
                    "verify_timeout_ms": { "type": "integer", "description": "Maximum time to wait for a visible change when verification is enabled (default: 1200)." },
                    "verify_poll_ms": { "type": "integer", "description": "Polling interval for verification screenshots (default: 150)." },
                    "verify_x": { "type": "integer", "description": "Optional absolute X for a custom verification region." },
                    "verify_y": { "type": "integer", "description": "Optional absolute Y for a custom verification region." },
                    "verify_width": { "type": "integer", "description": "Optional width for a custom verification region." },
                    "verify_height": { "type": "integer", "description": "Optional height for a custom verification region." },
                    "mode": { "type": "string", "description": "'amount' (default) or 'to_text' (scroll until text appears via OCR)" },
                    "text": { "type": "string", "description": "Text to find when mode='to_text'" },
                    "max_scrolls": { "type": "integer", "description": "Max scroll attempts for to_text mode (default 20)" },
                    "snap_to_screen": { "type": "boolean", "description": "Clamp off-screen coordinates to nearest monitor edge" },
                    "dpi_aware": { "type": "boolean", "description": "Apply DPI scaling to coordinates" }
                },
                "required": ["amount"]
            }
        }),
        json!({
            "name": "list_windows",
            "description": "List all visible windows on the desktop with their titles, positions, sizes, process names, and state (minimized/maximized/focused). Use this to find windows before clicking or interacting with them. Returns an indexed list you can reference by number.",
            "parameters": {
                "type": "object",
                "properties": {
                    "filter": { "type": "string", "description": "Optional case-insensitive filter. Only windows whose title or process name contains this string will be returned." },
                    "pid": { "type": "integer", "description": "Filter to windows of this process ID" }
                },
                "required": []
            }
        }),
        json!({
            "name": "mouse_drag",
            "description": "Click and drag the mouse from one position to another. Useful for resizing windows, selecting text, moving objects, or drawing.",
            "parameters": {
                "type": "object",
                "properties": {
                    "from_x": { "type": "integer", "description": "Starting X coordinate (pixels from left edge)" },
                    "from_y": { "type": "integer", "description": "Starting Y coordinate (pixels from top edge)" },
                    "to_x": { "type": "integer", "description": "Ending X coordinate" },
                    "to_y": { "type": "integer", "description": "Ending Y coordinate" },
                    "button": { "type": "string", "description": "Mouse button to use: left (default) or right" },
                    "delay_ms": { "type": "integer", "description": "Milliseconds to wait after drag before screenshot (default: 500)" },
                    "verify_screen_change": { "type": "boolean", "description": "If true, verify that the screen visibly changed after dragging before returning." },
                    "verify_threshold_pct": { "type": "number", "description": "Minimum percentage of sampled pixels that must change for verification to pass (default: 0.5)." },
                    "verify_timeout_ms": { "type": "integer", "description": "Maximum time to wait for a visible change when verification is enabled (default: 1200)." },
                    "verify_poll_ms": { "type": "integer", "description": "Polling interval for verification screenshots (default: 150)." },
                    "verify_x": { "type": "integer", "description": "Optional absolute X for a custom verification region." },
                    "verify_y": { "type": "integer", "description": "Optional absolute Y for a custom verification region." },
                    "verify_width": { "type": "integer", "description": "Optional width for a custom verification region." },
                    "verify_height": { "type": "integer", "description": "Optional height for a custom verification region." },
                    "steps": { "type": "integer", "description": "Intermediate points for smooth drag (1=instant, max 100). Increase for drawing or slider control." },
                    "snap_to_screen": { "type": "boolean", "description": "Clamp off-screen coordinates to nearest monitor edge" },
                    "timeout_ms": { "type": "integer", "description": "Operation timeout in ms (1000-60000, default 20000)" }
                },
                "required": ["from_x", "from_y", "to_x", "to_y"]
            }
        }),
        json!({
            "name": "get_cursor_position",
            "description": "Get the current mouse cursor position on screen. Returns x,y coordinates in pixels.",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "focus_window",
            "description": "Bring a window to the foreground and give it focus. Prefer pid when you already know the target window identity; otherwise use a case-insensitive title or process-name filter. If the window is minimized, it will be restored first.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Case-insensitive filter to match window title or process name (e.g. 'chrome', 'notepad')" },
                    "pid": { "type": "integer", "description": "Specific process ID to target. Prefer this once you know the window identity." }
                },
                "required": []
            }
        }),
        json!({
            "name": "minimize_window",
            "description": "Minimize a window to the taskbar. Prefer pid when you already know the target window identity; otherwise use a case-insensitive title or process-name filter.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Case-insensitive filter to match window title or process name" },
                    "pid": { "type": "integer", "description": "Specific process ID to target. Prefer this once you know the window identity." }
                },
                "required": []
            }
        }),
        json!({
            "name": "maximize_window",
            "description": "Maximize a window to fill the screen. Prefer pid when you already know the target window identity; otherwise use a case-insensitive title or process-name filter.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Case-insensitive filter to match window title or process name" },
                    "pid": { "type": "integer", "description": "Specific process ID to target. Prefer this once you know the window identity." }
                },
                "required": []
            }
        }),
        json!({
            "name": "close_window",
            "description": "Close a window gracefully by sending WM_CLOSE. Prefer pid when you already know the target window identity; otherwise use a case-insensitive title or process-name filter. The application may show a save dialog before closing.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Case-insensitive filter to match window title or process name" },
                    "pid": { "type": "integer", "description": "Specific process ID to target. Prefer this once you know the window identity." }
                },
                "required": []
            }
        }),
        json!({
            "name": "read_clipboard",
            "description": "Read the current text content from the system clipboard.",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "write_clipboard",
            "description": "Write text to the system clipboard, replacing its current content.",
            "parameters": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "The text to write to the clipboard" }
                },
                "required": ["text"]
            }
        }),
        // ─── New desktop tools ───────────────────────────────────────
        json!({
            "name": "resize_window",
            "description": "Move and/or resize a window by pid, title, or process name. Prefer pid when you already know the target window identity. Provide at least one of x, y, width, height.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Window title or process name to match (case-insensitive substring)" },
                    "pid": { "type": "integer", "description": "Specific process ID to target. Prefer this once you know the window identity." },
                    "x": { "type": "integer", "description": "New X position (screen coordinates)" },
                    "y": { "type": "integer", "description": "New Y position (screen coordinates)" },
                    "width": { "type": "integer", "description": "New width in pixels" },
                    "height": { "type": "integer", "description": "New height in pixels" }
                },
                "required": []
            }
        }),
        json!({
            "name": "get_active_window",
            "description": "Get info about the currently active (foreground) window: title, process, position, size.",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "wait_for_window",
            "description": "Wait for a window with matching pid, title, or process name to appear. Polls until found or timeout. Prefer pid when you already know the target window identity.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Window title or process name to wait for" },
                    "pid": { "type": "integer", "description": "Specific process ID to wait for. Prefer this once you know the window identity." },
                    "timeout_ms": { "type": "integer", "description": "Maximum wait time in ms (default 10000, max 60000)" },
                    "poll_ms": { "type": "integer", "description": "Polling interval in ms (default 200)" }
                },
                "required": []
            }
        }),
        json!({
            "name": "get_pixel_color",
            "description": "Get the color of a pixel at screen coordinates. Returns RGB values and hex code.",
            "parameters": {
                "type": "object",
                "properties": {
                    "x": { "type": "integer", "description": "X coordinate (screen pixels)" },
                    "y": { "type": "integer", "description": "Y coordinate (screen pixels)" }
                },
                "required": ["x", "y"]
            }
        }),
        json!({
            "name": "click_window_relative",
            "description": "Click at coordinates relative to a window's top-left corner. Focuses the window first. Prefer pid when you already know the target window identity.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Window title or process name to match" },
                    "pid": { "type": "integer", "description": "Specific process ID to target. Prefer this once you know the window identity." },
                    "x": { "type": "integer", "description": "X offset from window's left edge" },
                    "y": { "type": "integer", "description": "Y offset from window's top edge" },
                    "button": { "type": "string", "description": "Mouse button: left, right, middle, double (default: left)" },
                    "delay_ms": { "type": "integer", "description": "Delay before screenshot in ms (default 500)" },
                    "verify_screen_change": { "type": "boolean", "description": "If true, verify that the screen visibly changed after the click before returning." },
                    "verify_threshold_pct": { "type": "number", "description": "Minimum percentage of sampled pixels that must change for verification to pass (default: 0.5)." },
                    "verify_timeout_ms": { "type": "integer", "description": "Maximum time to wait for a visible change when verification is enabled (default: 1200)." },
                    "verify_poll_ms": { "type": "integer", "description": "Polling interval for verification screenshots (default: 150)." },
                    "verify_x": { "type": "integer", "description": "Optional absolute X for a custom verification region." },
                    "verify_y": { "type": "integer", "description": "Optional absolute Y for a custom verification region." },
                    "verify_width": { "type": "integer", "description": "Optional width for a custom verification region." },
                    "verify_height": { "type": "integer", "description": "Optional height for a custom verification region." }
                },
                "required": ["x", "y"]
            }
        }),
        json!({
            "name": "list_monitors",
            "description": "List all connected monitors with name, resolution, position, scale factor, and primary status.",
            "parameters": {
                "type": "object",
                "properties": {
                    "index": { "type": "integer", "description": "Get info for a specific monitor index only" }
                },
                "required": []
            }
        }),
        json!({
            "name": "screenshot_region",
            "description": "Capture a screenshot of a specific rectangular region of the screen. Returns the cropped image.",
            "parameters": {
                "type": "object",
                "properties": {
                    "x": { "type": "integer", "description": "Left edge X coordinate" },
                    "y": { "type": "integer", "description": "Top edge Y coordinate" },
                    "width": { "type": "integer", "description": "Width of the region in pixels" },
                    "height": { "type": "integer", "description": "Height of the region in pixels" },
                    "monitor": { "type": "integer", "description": "Monitor index (default 0)" }
                },
                "required": ["x", "y", "width", "height"]
            }
        }),
        json!({
            "name": "screenshot_diff",
            "description": "Compare current screen to a baseline. First call with save_baseline=true to save, then call again to compare. Reports percentage of changed pixels and bounding box.",
            "parameters": {
                "type": "object",
                "properties": {
                    "save_baseline": { "type": "boolean", "description": "If true, save current screen as baseline instead of comparing (default false)" },
                    "monitor": { "type": "integer", "description": "Monitor index (default 0)" },
                    "highlight": { "type": "boolean", "description": "Return image with red rectangle highlighting changed region" }
                },
                "required": []
            }
        }),
        json!({
            "name": "ocr_screen",
            "description": "Extract text from the screen using OCR. Prefer pid when you already know the target window identity; otherwise use window/title or a manual region instead of scanning the full monitor.",
            "parameters": {
                "type": "object",
                "properties": {
                    "window": { "type": "string", "description": "Window title to auto-crop OCR to (case-insensitive)" },
                    "title": { "type": "string", "description": "Alias for window title/process filter to auto-crop OCR to" },
                    "pid": { "type": "integer", "description": "Specific process ID to auto-crop OCR to. Prefer this once you know the window identity." },
                    "x": { "type": "integer", "description": "Left edge of region to OCR" },
                    "y": { "type": "integer", "description": "Top edge of region to OCR" },
                    "width": { "type": "integer", "description": "Width of region" },
                    "height": { "type": "integer", "description": "Height of region" },
                    "monitor": { "type": "integer", "description": "Monitor index (default 0)" },
                    "language": { "type": "string", "description": "OCR language code. macOS Vision only." }
                },
                "required": []
            }
        }),
        json!({
            "name": "get_ui_tree",
            "description": "Get the UI element tree of a window using UI Automation. Shows control types and names. Useful for finding clickable elements without a screenshot.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Window title or process name (default: active window)" },
                    "depth": { "type": "integer", "description": "Max tree depth 1-8 (default 3)" },
                    "exclude_types": { "type": "array", "items": { "type": "string" }, "description": "Control types to exclude (e.g. ['image','separator','thumb'])" }
                },
                "required": []
            }
        }),
        json!({
            "name": "ocr_find_text",
            "description": "OCR the screen and find specific text, returning its bounding box coordinates. Prefer pid when you already know the target window identity; otherwise use window/title or a manual region to avoid scanning the full monitor.",
            "parameters": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Text to search for (case-insensitive)" },
                    "window": { "type": "string", "description": "Window title to auto-crop OCR search to (case-insensitive)" },
                    "title": { "type": "string", "description": "Alias for window title/process filter to auto-crop OCR search to" },
                    "pid": { "type": "integer", "description": "Specific process ID to auto-crop OCR search to. Prefer this once you know the window identity." },
                    "x": { "type": "integer", "description": "Optional region X offset" },
                    "y": { "type": "integer", "description": "Optional region Y offset" },
                    "width": { "type": "integer", "description": "Optional region width" },
                    "height": { "type": "integer", "description": "Optional region height" },
                    "monitor": { "type": "integer", "description": "Monitor index (default 0)" },
                    "language": { "type": "string", "description": "OCR language code (e.g. 'en-US', 'ja-JP'). macOS Vision only." }
                },
                "required": ["text"]
            }
        }),
        json!({
            "name": "click_ui_element",
            "description": "Find a UI element by name and/or control type using UI Automation, then click its center. Works without screenshots — finds buttons, links, text fields by their accessible name.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Element name to search for (case-insensitive substring match)" },
                    "control_type": { "type": "string", "description": "Control type filter: Button, Edit, CheckBox, ComboBox, MenuItem, Hyperlink, etc." },
                    "title": { "type": "string", "description": "Window title (default: active window)" },
                    "index": { "type": "integer", "description": "Click the Nth match (0-based, default 0). Use with find_ui_elements to see all matches first." },
                    "delay_ms": { "type": "integer", "description": "Delay before screenshot in ms (default 500)" }
                },
                "required": []
            }
        }),
        json!({
            "name": "window_screenshot",
            "description": "Capture a screenshot of a specific window by title. Smaller and more focused than a full screen screenshot.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Window title or app name to capture (case-insensitive substring match)" }
                },
                "required": ["title"]
            }
        }),
        json!({
            "name": "open_application",
            "description": "Launch an application by name or path. Can open executables, URLs, files, or system apps (e.g. 'notepad', 'calc', 'https://google.com', 'C:\\\\path\\\\to\\\\app.exe').",
            "parameters": {
                "type": "object",
                "properties": {
                    "target": { "type": "string", "description": "Application name, path, or URL to open" },
                    "args": { "type": "string", "description": "Optional command-line arguments" }
                },
                "required": ["target"]
            }
        }),
        json!({
            "name": "wait_for_screen_change",
            "description": "Wait until a screen region changes visually. Useful for waiting for loading indicators, animations, or content updates.",
            "parameters": {
                "type": "object",
                "properties": {
                    "x": { "type": "integer", "description": "Region X (default 0)" },
                    "y": { "type": "integer", "description": "Region Y (default 0)" },
                    "width": { "type": "integer", "description": "Region width (default 200)" },
                    "height": { "type": "integer", "description": "Region height (default 200)" },
                    "timeout_ms": { "type": "integer", "description": "Max wait in ms (default 10000, max 30000)" },
                    "threshold": { "type": "number", "description": "% of pixels that must change (default 5)" },
                    "monitor": { "type": "integer", "description": "Monitor index (default 0)" }
                },
                "required": []
            }
        }),
        json!({
            "name": "set_window_topmost",
            "description": "Set a window to always-on-top or remove always-on-top. Prefer pid when you already know the target window identity. Useful for keeping reference windows visible while working.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Window title to modify" },
                    "pid": { "type": "integer", "description": "Specific process ID to target. Prefer this once you know the window identity." },
                    "topmost": { "type": "boolean", "description": "true = always on top, false = remove (default true)" }
                },
                "required": []
            }
        }),
        serde_json::json!({
            "name": "invoke_ui_action",
            "description": "Invoke a UI Automation action on an element. Supports: invoke (click buttons), toggle (checkboxes), expand/collapse (tree nodes, dropdowns), select (list items), set_value (text fields). More reliable than coordinate clicking for standard Windows controls.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Element name to match (case-insensitive substring)" },
                    "control_type": { "type": "string", "description": "Control type filter (button, checkbox, edit, combobox, etc.)" },
                    "action": { "type": "string", "description": "Action: invoke, toggle, expand, collapse, select, set_value" },
                    "value": { "type": "string", "description": "Value for set_value action" },
                    "title": { "type": "string", "description": "Window title (default: active window)" }
                },
                "required": ["action"]
            }
        }),
        serde_json::json!({
            "name": "read_ui_element_value",
            "description": "Read the current text value of a UI element (text field, label, status bar, etc.) using UI Automation ValuePattern.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Element name to match (case-insensitive substring)" },
                    "control_type": { "type": "string", "description": "Control type filter" },
                    "title": { "type": "string", "description": "Window title (default: active window)" }
                },
                "required": []
            }
        }),
        serde_json::json!({
            "name": "wait_for_ui_element",
            "description": "Wait until a UI element matching name/control_type appears in a window. Useful for waiting for dialogs, loading indicators, or UI state changes.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Element name to wait for" },
                    "control_type": { "type": "string", "description": "Control type to wait for" },
                    "title": { "type": "string", "description": "Window title (default: active window)" },
                    "timeout_ms": { "type": "integer", "description": "Max wait in ms (default 10000, max 30000)" },
                    "poll_ms": { "type": "integer", "description": "Polling interval in ms (default 500, min 100)" }
                },
                "required": []
            }
        }),
        serde_json::json!({
            "name": "clipboard_image",
            "description": "Read or write images from/to the clipboard. Read returns the clipboard image as PNG. Write captures the screen and copies it to clipboard.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "read or write (default: read)" },
                    "monitor": { "type": "integer", "description": "Monitor index for write action (default 0)" }
                },
                "required": []
            }
        }),
        serde_json::json!({
            "name": "find_ui_elements",
            "description": "Search for ALL UI elements matching name/control_type in a window. Returns positions, sizes, and element descriptions. Useful for discovering available UI controls.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Element name filter (case-insensitive substring)" },
                    "control_type": { "type": "string", "description": "Control type filter (button, edit, checkbox, etc.)" },
                    "title": { "type": "string", "description": "Window title (default: active window)" },
                    "max_results": { "type": "integer", "description": "Max elements to return (default 10, max 50)" }
                },
                "required": []
            }
        }),
        serde_json::json!({
            "name": "execute_app_script",
            "description": "Execute a script inside a GPU-rendered application (Blender, etc.). These apps render with OpenGL/Vulkan so UI Automation tools don't work — use this instead. Supported apps: blender (Python/bpy).",
            "parameters": {
                "type": "object",
                "properties": {
                    "app": { "type": "string", "description": "Application name: 'blender'" },
                    "code": { "type": "string", "description": "Script source code (Python for Blender)" },
                    "file": { "type": "string", "description": "Optional file to open (e.g. scene.blend)" },
                    "background": { "type": "boolean", "description": "Run headless (default true). Set false to see GUI." }
                },
                "required": ["app", "code"]
            }
        }),
        serde_json::json!({
            "name": "send_notification",
            "description": "Send a desktop notification (Windows toast / macOS notification / Linux notify-send). Use this to alert the user about progress, completion, or when you need their attention — especially during desktop automation so the user knows when to stop waiting.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Notification title (default: 'Claude Code')" },
                    "message": { "type": "string", "description": "Notification message body" }
                },
                "required": ["message"]
            }
        }),
        serde_json::json!({
            "name": "show_status_overlay",
            "description": "Show a persistent status bar overlay on screen. The bar is semi-transparent, always-on-top, click-through, and does not steal focus. Use this at the start of multi-step desktop automation to keep the user informed of progress. The overlay persists until you call hide_status_overlay.",
            "parameters": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Text to display (e.g. '[Claude Code] Step 1/5: Opening Blender...')" },
                    "position": { "type": "string", "description": "Bar position: 'top' (default) or 'bottom'" }
                },
                "required": ["text"]
            }
        }),
        serde_json::json!({
            "name": "update_status_overlay",
            "description": "Update the text on the existing status overlay bar. Must call show_status_overlay first. Near-instant — just a text update via IPC, no new process spawned.",
            "parameters": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "New text to display on the overlay" }
                },
                "required": ["text"]
            }
        }),
        serde_json::json!({
            "name": "hide_status_overlay",
            "description": "Dismiss the status overlay bar. Call this when the automation sequence is complete.",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        serde_json::json!({
            "name": "send_keys_to_window",
            "description": "Send keystrokes to a window. Prefer pid when you already know the target window identity. Default method 'post_message' works in background. Use method 'send_input' for foreground apps that don't respond to PostMessage (games, custom UIs).",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Window title to send keys to" },
                    "pid": { "type": "integer", "description": "Specific process ID to target. Prefer this once you know the window identity." },
                    "keys": { "type": "string", "description": "Key combo to send (e.g. 'ctrl+s', 'enter', 'alt+f4')" },
                    "text": { "type": "string", "description": "Text characters to type" },
                    "method": { "type": "string", "description": "Input method: post_message (default, background) or send_input (foreground, more reliable)" }
                },
                "required": []
            }
        }),
        serde_json::json!({
            "name": "snap_window",
            "description": "Snap a window to a screen position: left, right, top-left, top-right, bottom-left, bottom-right, center, maximize, restore. Prefer pid when you already know the target window identity. Uses monitor work area (excludes taskbar).",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Window title to snap" },
                    "pid": { "type": "integer", "description": "Specific process ID to target. Prefer this once you know the window identity." },
                    "position": { "type": "string", "description": "Position: left, right, top-left, top-right, bottom-left, bottom-right, center, maximize, restore" }
                },
                "required": ["position"]
            }
        }),
        serde_json::json!({
            "name": "list_processes",
            "description": "List running processes with PID and executable name. Optionally filter by name substring.",
            "parameters": {
                "type": "object",
                "properties": {
                    "filter": { "type": "string", "description": "Filter by process name (case-insensitive substring)" }
                },
                "required": []
            }
        }),
        serde_json::json!({
            "name": "kill_process",
            "description": "Terminate a process by name or PID. Refuses to kill system-critical processes (csrss, lsass, svchost, dwm, etc.).",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Process name to kill (kills all matching)" },
                    "pid": { "type": "integer", "description": "Specific process ID to kill" },
                    "force": { "type": "boolean", "description": "true (default): immediate kill. false: graceful WM_CLOSE then wait." },
                    "grace_ms": { "type": "integer", "description": "Grace period in ms when force=false (default 5000, max 15000)" }
                },
                "required": []
            }
        }),
        // ── Compound desktop tools ──────────────────────────────────────
        serde_json::json!({
            "name": "find_and_click_text",
            "description": "OCR the screen, find specific text, and click its center — all in one step. Combines ocr_find_text + click_screen. Use 'index' to click the Nth match.",
            "parameters": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Text to find and click (case-insensitive)" },
                    "index": { "type": "integer", "description": "Click the Nth match (0-based, default 0)" },
                    "monitor": { "type": "integer", "description": "Monitor index (default 0)" },
                    "delay_ms": { "type": "integer", "description": "Delay before screenshot in ms (default 500)" }
                },
                "required": ["text"]
            }
        }),
        serde_json::json!({
            "name": "type_into_element",
            "description": "Find a UI element by name/type, click it to focus, then type text. Combines click_ui_element + type_text in one step.",
            "parameters": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Text to type into the element" },
                    "name": { "type": "string", "description": "Element name to find (case-insensitive substring)" },
                    "control_type": { "type": "string", "description": "Control type filter (Edit, ComboBox, etc.)" },
                    "title": { "type": "string", "description": "Window title (default: active window)" }
                },
                "required": ["text"]
            }
        }),
        serde_json::json!({
            "name": "get_window_text",
            "description": "Extract all text content from a window via UI Automation tree walk. Returns text from labels, edit fields, and documents. Useful for reading window content without OCR.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Window title (default: active window)" },
                    "max_chars": { "type": "integer", "description": "Max characters to return (default 50000)" }
                },
                "required": []
            }
        }),
        serde_json::json!({
            "name": "file_dialog_navigate",
            "description": "Navigate a file Open/Save dialog: sets the filename field and clicks the button. Useful for automating file selection in native dialogs.",
            "parameters": {
                "type": "object",
                "properties": {
                    "filename": { "type": "string", "description": "File path or name to enter" },
                    "button": { "type": "string", "description": "Button to click: Open, Save, etc. (default: Open)" },
                    "title": { "type": "string", "description": "Dialog window title (auto-detected if omitted)" }
                },
                "required": ["filename"]
            }
        }),
        serde_json::json!({
            "name": "drag_and_drop_element",
            "description": "Find two UI elements by name/type and drag from one to the other. Combines find_ui_element + mouse_drag.",
            "parameters": {
                "type": "object",
                "properties": {
                    "from_name": { "type": "string", "description": "Source element name" },
                    "from_type": { "type": "string", "description": "Source control type" },
                    "to_name": { "type": "string", "description": "Target element name" },
                    "to_type": { "type": "string", "description": "Target control type" },
                    "title": { "type": "string", "description": "Window title (default: active window)" },
                    "delay_ms": { "type": "integer", "description": "Delay before screenshot in ms (default 500)" }
                },
                "required": []
            }
        }),
        serde_json::json!({
            "name": "wait_for_text_on_screen",
            "description": "Poll OCR until specified text appears on screen. Useful for waiting for loading to complete, dialogs to appear, or status text changes.",
            "parameters": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Text to wait for (case-insensitive)" },
                    "timeout_ms": { "type": "integer", "description": "Max wait in ms (default 10000, max 30000)" },
                    "poll_ms": { "type": "integer", "description": "Polling interval in ms (default 1000, min 500)" },
                    "monitor": { "type": "integer", "description": "Monitor index (default 0)" }
                },
                "required": ["text"]
            }
        }),
        serde_json::json!({
            "name": "get_context_menu",
            "description": "Right-click at coordinates to open a context menu, read menu items via UI Automation, and optionally click one. Returns a numbered list of menu items.",
            "parameters": {
                "type": "object",
                "properties": {
                    "x": { "type": "integer", "description": "X coordinate to right-click" },
                    "y": { "type": "integer", "description": "Y coordinate to right-click" },
                    "click_item": { "type": "string", "description": "Menu item name to click (optional — just reads if omitted)" },
                    "delay_ms": { "type": "integer", "description": "Delay before screenshot in ms (default 500)" }
                },
                "required": ["x", "y"]
            }
        }),
        serde_json::json!({
            "name": "scroll_element",
            "description": "Find a UI element by name/type and scroll it. Uses mouse wheel at the element's center. Useful for scrolling specific panels or lists.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Element name to find" },
                    "control_type": { "type": "string", "description": "Control type filter" },
                    "direction": { "type": "string", "description": "Scroll direction: up or down (default: down)" },
                    "amount": { "type": "integer", "description": "Number of scroll clicks (default 3)" },
                    "title": { "type": "string", "description": "Window title (default: active window)" }
                },
                "required": []
            }
        }),
        // ─── New tools (batch 3) ─────────────────────────────────────
        serde_json::json!({
            "name": "mouse_button",
            "description": "Press or release a mouse button independently without clicking. Useful for hold-and-drag scenarios where you need separate press and release control.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "Action to perform: press or release" },
                    "button": { "type": "string", "description": "Mouse button: left, right, middle (default: left)" },
                    "screenshot": { "type": "boolean", "description": "Take screenshot after action (default true)" }
                },
                "required": ["action"]
            }
        }),
        serde_json::json!({
            "name": "switch_virtual_desktop",
            "description": "Switch to an adjacent virtual desktop using Ctrl+Win+Arrow keyboard shortcut.",
            "parameters": {
                "type": "object",
                "properties": {
                    "direction": { "type": "string", "description": "Direction: left/prev or right/next" }
                },
                "required": ["direction"]
            }
        }),
        serde_json::json!({
            "name": "find_image_on_screen",
            "description": "Find a template image on the screen using pixel matching (SSD). Returns the position and confidence if found. Useful for finding icons, buttons, or UI elements by their visual appearance.",
            "parameters": {
                "type": "object",
                "properties": {
                    "template": { "type": "string", "description": "Path to the template image file (PNG, JPEG, etc.)" },
                    "monitor": { "type": "integer", "description": "Monitor index (default 0)" },
                    "confidence": { "type": "number", "description": "Minimum confidence threshold 0.0-1.0 (default 0.9)" },
                    "step": { "type": "integer", "description": "Search step size in pixels — larger = faster but less precise (default 2)" }
                },
                "required": ["template"]
            }
        }),
        serde_json::json!({
            "name": "get_process_info",
            "description": "Get resource info (memory usage, CPU time) for a process by PID or name.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pid": { "type": "integer", "description": "Process ID" },
                    "name": { "type": "string", "description": "Process name (partial match)" }
                },
                "required": []
            }
        }),
        // ─── Round 3 tools ──────────────────────────────────────────────────
        json!({
            "name": "paste",
            "description": "Paste clipboard contents at the current cursor position (Ctrl+V). Takes a screenshot after pasting.",
            "parameters": {
                "type": "object",
                "properties": {
                    "delay_ms": { "type": "integer", "description": "Wait after paste before screenshot (default 300)" }
                },
                "required": []
            }
        }),
        json!({
            "name": "clear_field",
            "description": "Clear the currently focused input field (Ctrl+A → Delete). Optionally type new text after clearing.",
            "parameters": {
                "type": "object",
                "properties": {
                    "then_type": { "type": "string", "description": "Text to type after clearing the field" },
                    "delay_ms": { "type": "integer", "description": "Wait after action (default 200)" }
                },
                "required": []
            }
        }),
        json!({
            "name": "hover_element",
            "description": "Hover over a UI element by name/type to trigger tooltip or hover effects. Returns tooltip text if found.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "UI element name (partial match)" },
                    "control_type": { "type": "string", "description": "UI control type (Button, Edit, etc.)" },
                    "title": { "type": "string", "description": "Window title filter (default: active window)" },
                    "hover_ms": { "type": "integer", "description": "How long to hover before capturing (default 800)" }
                },
                "required": []
            }
        }),
        json!({
            "name": "handle_dialog",
            "description": "Detect and interact with modal dialogs. Lists dialog text and buttons, optionally clicks a button.",
            "parameters": {
                "type": "object",
                "properties": {
                    "button": { "type": "string", "description": "Button name to click (e.g. 'OK', 'Cancel', 'Yes', 'Save')" }
                },
                "required": []
            }
        }),
        json!({
            "name": "wait_for_element_state",
            "description": "Wait until a UI element reaches a specific state (exists, gone, visible, hidden).",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "UI element name (partial match)" },
                    "control_type": { "type": "string", "description": "UI control type filter" },
                    "state": { "type": "string", "description": "Target state: exists, gone, visible, hidden" },
                    "title": { "type": "string", "description": "Window title filter" },
                    "timeout_ms": { "type": "integer", "description": "Maximum wait time (default 5000)" }
                },
                "required": ["state"]
            }
        }),
        json!({
            "name": "fill_form",
            "description": "Fill multiple form fields by finding UI elements by label and typing values. Each field is clicked, cleared, and filled.",
            "parameters": {
                "type": "object",
                "properties": {
                    "fields": {
                        "type": "array",
                        "description": "Array of {label, value} objects. Each field object can include \"type\": \"text|checkbox|dropdown|radio\" to force field type handling.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "label": { "type": "string" },
                                "value": { "type": "string" },
                                "type": { "type": "string", "description": "Force field type: text, checkbox, dropdown, or radio" }
                            }
                        }
                    },
                    "title": { "type": "string", "description": "Window title filter" }
                },
                "required": ["fields"]
            }
        }),
        json!({
            "name": "run_action_sequence",
            "description": "Execute a sequence of desktop actions (click, type, press_key, paste, wait, clear, scroll, move). Each action is a JSON object with an 'action' field.",
            "parameters": {
                "type": "object",
                "properties": {
                    "actions": {
                        "type": "array",
                        "description": "Array of action objects. Each has 'action' (click/type/press_key/paste/wait/clear/scroll/move) plus params. Per-action options: 'retry' (0-3), 'if_previous' ('success'|'failure'), 'abort_on_failure' (boolean), 'screenshot_mode' ('final_only'|'all'|'none').",
                        "items": { "type": "object" }
                    },
                    "delay_between_ms": { "type": "integer", "description": "Default delay between actions (default 200)" }
                },
                "required": ["actions"]
            }
        }),
        json!({
            "name": "move_to_monitor",
            "description": "Move a window to a specific monitor by index. Preserves window size.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Window title or process name filter" },
                    "monitor": { "type": "integer", "description": "Target monitor index (default 0)" }
                },
                "required": ["title"]
            }
        }),
        json!({
            "name": "set_window_opacity",
            "description": "Set window transparency. 0 = fully transparent, 100 = fully opaque.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Window title or process name filter" },
                    "opacity": { "type": "integer", "description": "Opacity percentage 0-100 (default 100)" }
                },
                "required": ["title"]
            }
        }),
        json!({
            "name": "highlight_point",
            "description": "Draw a crosshair marker on a screenshot at specified coordinates. Useful for debugging coordinate targeting.",
            "parameters": {
                "type": "object",
                "properties": {
                    "x": { "type": "integer", "description": "X coordinate" },
                    "y": { "type": "integer", "description": "Y coordinate" },
                    "color": { "type": "string", "description": "Marker color: red, green, blue, yellow (default red)" },
                    "size": { "type": "integer", "description": "Marker size in pixels (default 20)" },
                    "monitor": { "type": "integer", "description": "Monitor index (default 0)" }
                },
                "required": ["x", "y"]
            }
        }),
        json!({
            "name": "annotate_screenshot",
            "description": "Draw shapes (rectangles, circles, lines) on a screenshot for visual annotation.",
            "parameters": {
                "type": "object",
                "properties": {
                    "shapes": {
                        "type": "array",
                        "description": "Array of shapes: {type: rect|circle|line, x, y, w, h, r, x1, y1, x2, y2, color, thickness}",
                        "items": { "type": "object" }
                    },
                    "monitor": { "type": "integer", "description": "Monitor index (default 0)" }
                },
                "required": ["shapes"]
            }
        }),
        json!({
            "name": "ocr_region",
            "description": "Perform OCR on a specific rectangular region of the screen. Returns recognized text and the cropped region image.",
            "parameters": {
                "type": "object",
                "properties": {
                    "x": { "type": "integer", "description": "Left edge X coordinate" },
                    "y": { "type": "integer", "description": "Top edge Y coordinate" },
                    "width": { "type": "integer", "description": "Region width in pixels" },
                    "height": { "type": "integer", "description": "Region height in pixels" },
                    "monitor": { "type": "integer", "description": "Monitor index (default 0)" }
                },
                "required": ["width", "height"]
            }
        }),
        json!({
            "name": "find_color_on_screen",
            "description": "Find pixels on screen matching a specific color (hex #RRGGBB) within tolerance. Returns coordinates of matches.",
            "parameters": {
                "type": "object",
                "properties": {
                    "color": { "type": "string", "description": "Target color in hex format #RRGGBB" },
                    "tolerance": { "type": "integer", "description": "Color matching tolerance per channel 0-255 (default 30)" },
                    "max_results": { "type": "integer", "description": "Maximum matches to return (default 10)" },
                    "step": { "type": "integer", "description": "Pixel scan step size (default 4, use 1 for thorough)" },
                    "region_x": { "type": "integer", "description": "Optional region left X" },
                    "region_y": { "type": "integer", "description": "Optional region top Y" },
                    "region_w": { "type": "integer", "description": "Optional region width" },
                    "region_h": { "type": "integer", "description": "Optional region height" },
                    "monitor": { "type": "integer", "description": "Monitor index (default 0)" }
                },
                "required": ["color"]
            }
        }),
        json!({
            "name": "read_registry",
            "description": "Read a value from the Windows registry. Supports REG_SZ (string) and REG_DWORD (integer) types.",
            "parameters": {
                "type": "object",
                "properties": {
                    "hive": { "type": "string", "description": "Registry hive: HKCU or HKLM (default HKCU)" },
                    "key": { "type": "string", "description": "Registry subkey path (e.g. 'SOFTWARE\\Microsoft\\Windows\\CurrentVersion')" },
                    "value": { "type": "string", "description": "Value name to read (empty for default value)" }
                },
                "required": ["key"]
            }
        }),
        json!({
            "name": "click_tray_icon",
            "description": "Find and click a system tray (notification area) icon by its tooltip text.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Icon tooltip text to search for (partial match)" }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "watch_window",
            "description": "Monitor for window changes (new windows, closed windows, title changes). Returns on first change or timeout.",
            "parameters": {
                "type": "object",
                "properties": {
                    "timeout_ms": { "type": "integer", "description": "Maximum wait time (default 10000)" },
                    "filter": { "type": "string", "description": "Only report changes for windows matching this filter" },
                    "poll_ms": { "type": "integer", "description": "Polling interval (default 500)" }
                },
                "required": []
            }
        }),
        // ---- Round 4-5 tools ----
        json!({
            "name": "get_system_volume",
            "description": "Get the current system audio volume (0-100) and muted state.",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "set_system_volume",
            "description": "Set the system audio volume level.",
            "parameters": {
                "type": "object",
                "properties": {
                    "level": { "type": "integer", "description": "Volume level 0-100" }
                },
                "required": ["level"]
            }
        }),
        json!({
            "name": "set_system_mute",
            "description": "Mute or unmute the system audio.",
            "parameters": {
                "type": "object",
                "properties": {
                    "muted": { "type": "boolean", "description": "true to mute, false to unmute" }
                },
                "required": ["muted"]
            }
        }),
        json!({
            "name": "list_audio_devices",
            "description": "List available audio output devices.",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "clear_clipboard",
            "description": "Clear all content from the system clipboard.",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "clipboard_file_paths",
            "description": "Read or write file paths on the clipboard (e.g. copied files in a file manager).",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "'read' to get file paths from clipboard, 'write' to put file paths on clipboard" },
                    "paths": { "type": "array", "description": "File paths to write (required for action='write')" }
                },
                "required": ["action"]
            }
        }),
        json!({
            "name": "clipboard_html",
            "description": "Read or write HTML content on the clipboard.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "'read' to get HTML from clipboard, 'write' to put HTML on clipboard" },
                    "html": { "type": "string", "description": "HTML content to write (required for action='write')" }
                },
                "required": ["action"]
            }
        }),
        json!({
            "name": "save_window_layout",
            "description": "Save positions and sizes of all open windows to a named layout file.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Layout name (used as filename)" }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "restore_window_layout",
            "description": "Restore windows to positions saved in a named layout file.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Layout name to restore" }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "wait_for_process_exit",
            "description": "Block until a process exits or timeout. Useful for waiting on installers, builds, etc.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pid": { "type": "integer", "description": "Process ID to wait for" },
                    "name": { "type": "string", "description": "Process name to wait for (alternative to pid)" },
                    "timeout_ms": { "type": "integer", "description": "Maximum wait time (default 30000)" }
                },
                "required": []
            }
        }),
        json!({
            "name": "get_process_tree",
            "description": "Show a process and all its child processes in a tree format.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pid": { "type": "integer", "description": "Root process ID" }
                },
                "required": ["pid"]
            }
        }),
        json!({
            "name": "get_system_metrics",
            "description": "Get system CPU usage, memory usage, and disk free space.",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "wait_for_notification",
            "description": "Wait for a system notification matching a text filter (OCR-based detection).",
            "parameters": {
                "type": "object",
                "properties": {
                    "text_contains": { "type": "string", "description": "Text to search for in the notification" },
                    "timeout_ms": { "type": "integer", "description": "Maximum wait time (default 10000)" }
                },
                "required": ["text_contains"]
            }
        }),
        json!({
            "name": "dismiss_all_notifications",
            "description": "Clear/dismiss all system notifications from the notification center.",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "start_screen_recording",
            "description": "Start recording the screen to a video file using ffmpeg. Call stop_screen_recording to finish.",
            "parameters": {
                "type": "object",
                "properties": {
                    "output_path": { "type": "string", "description": "Output file path (e.g. 'recording.mp4')" },
                    "fps": { "type": "integer", "description": "Frames per second (default 15)" },
                    "monitor": { "type": "integer", "description": "Monitor index (default 0)" }
                },
                "required": ["output_path"]
            }
        }),
        json!({
            "name": "stop_screen_recording",
            "description": "Stop an active screen recording started by start_screen_recording.",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "capture_gif",
            "description": "Capture a short animated GIF of the screen (pure Rust, no ffmpeg needed).",
            "parameters": {
                "type": "object",
                "properties": {
                    "output_path": { "type": "string", "description": "Output GIF file path" },
                    "duration_ms": { "type": "integer", "description": "Recording duration in ms (default 3000)" },
                    "fps": { "type": "integer", "description": "Frames per second (default 10)" },
                    "monitor": { "type": "integer", "description": "Monitor index (default 0)" }
                },
                "required": ["output_path"]
            }
        }),
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
        json!({
            "name": "dialog_handler_stop",
            "description": "Stop the background dialog handler and return the count of dialogs that were auto-handled.",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        // ─── Phase J tools ──────────────────────────────────────────────────
        json!({
            "name": "smart_wait",
            "description": "Wait until screen changes, specific text appears via OCR, or both. Combines wait_for_screen_change + wait_for_text_on_screen.",
            "parameters": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Text to wait for via OCR (optional if just waiting for screen change)" },
                    "timeout_ms": { "type": "integer", "description": "Maximum wait time (default 10000, max 30000)" },
                    "threshold": { "type": "number", "description": "Pixel change threshold percentage (default 1.0)" },
                    "mode": { "type": "string", "description": "'any' (default) = return when either condition met, 'all' = wait for both" },
                    "monitor": { "type": "integer", "description": "Monitor index (default 0)" },
                    "poll_ms": { "type": "integer", "description": "Polling interval (default 500)" }
                },
                "required": []
            }
        }),
        json!({
            "name": "click_and_verify",
            "description": "Find text on screen via OCR, click it, then verify that different expected text appeared. Combines find_and_click_text + OCR verification.",
            "parameters": {
                "type": "object",
                "properties": {
                    "click_text": { "type": "string", "description": "Text to find and click" },
                    "expect_text": { "type": "string", "description": "Text expected to appear after clicking" },
                    "timeout_ms": { "type": "integer", "description": "Maximum wait for verification (default 5000)" },
                    "monitor": { "type": "integer", "description": "Monitor index (default 0)" }
                },
                "required": ["click_text", "expect_text"]
            }
        }),
        // MCP server management tools
        json!({
            "name": "list_mcp_servers",
            "description": "List all configured MCP (Model Context Protocol) servers with their connection status and available tools.",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
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
        json!({
            "name": "remove_mcp_server",
            "description": "Remove an MCP server by name. This disconnects the server and removes its configuration.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name of the MCP server to remove" }
                },
                "required": ["name"]
            }
        }),
        // Background process tracking
        json!({
            "name": "list_background_processes",
            "description": "List all tracked background processes (running servers, daemons, etc.) with their PIDs, commands, and status. Also shows orphaned processes from previous sessions that are still running.",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        // Sub-agent spawning
        json!({
            "name": "spawn_agent",
            "description": "Spawn a sub-agent to handle an isolated sub-task. The agent gets a fresh context and returns a summary of what it did. Use for installation tasks, research, or any step that might use lots of context.",
            "parameters": {
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "The sub-task description for the agent to complete"
                    },
                    "context": {
                        "type": "string",
                        "description": "Additional context to provide to the agent (file contents, error messages, etc.)"
                    }
                },
                "required": ["task"]
            }
        }),
    ];

    tools
        .into_iter()
        .filter(|tool| {
            tool.get("name")
                .and_then(|name| name.as_str())
                .map(desktop_tool_available_on_current_platform)
                .unwrap_or(true)
        })
        .collect()
}

/// Desktop tool names exposed via the MCP server.
/// This list must stay in sync with `dispatch_desktop_tool()` in desktop_tools/mod.rs.
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
    // Round 4-5 tools
    "get_system_volume", "set_system_volume", "set_system_mute", "list_audio_devices",
    "clear_clipboard", "clipboard_file_paths", "clipboard_html",
    "save_window_layout", "restore_window_layout",
    "wait_for_process_exit", "get_process_tree", "get_system_metrics",
    "wait_for_notification", "dismiss_all_notifications",
    "start_screen_recording", "stop_screen_recording", "capture_gif",
    "dialog_handler_start", "dialog_handler_stop",
];

fn desktop_tool_available_on_current_platform(name: &str) -> bool {
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

/// Get only the desktop automation tool definitions (for the MCP server).
/// Returns the subset of `get_available_tools()` that matches desktop tool names.
#[allow(dead_code)]
pub fn get_desktop_tool_definitions() -> Vec<Value> {
    get_available_tools()
        .into_iter()
        .filter(|tool| {
            tool.get("name")
                .and_then(|n| n.as_str())
                .map_or(false, |name| DESKTOP_TOOL_NAMES.contains(&name))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_conversation_for_jinja_replaces_system() {
        let conversation = r#"SYSTEM:
Old system prompt that should be replaced.

USER:
Hello!

ASSISTANT:
Hi there!"#;

        let messages = parse_conversation_for_jinja(conversation, "My behavioral prompt");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[0].content, "My behavioral prompt");
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[1].content, "Hello!");
        assert_eq!(messages[2].role, "assistant");
        assert_eq!(messages[2].content, "Hi there!");
    }

    #[test]
    fn test_get_available_tools_openai_format() {
        let tools = get_available_tools_openai();
        assert!(!tools.is_empty());
        // Each tool should have "type": "function" wrapper
        for tool in &tools {
            assert_eq!(tool["type"], "function");
            assert!(tool["function"]["name"].is_string());
            assert!(tool["function"]["description"].is_string());
        }
    }

    #[test]
    fn test_preprocess_template_strips_ensure_ascii() {
        let input = r#"{{ tool | tojson(ensure_ascii=False) }}"#;
        let output = preprocess_template(input);
        assert_eq!(output, "{{ tool | tojson }}");
    }

    #[test]
    fn test_preprocess_template_converts_endswith() {
        let input = r#"not visible_text(m.content).endswith("/nothink")"#;
        let output = preprocess_template(input);
        assert_eq!(output, r#"not visible_text(m.content) is endingwith("/nothink")"#);
    }

    #[test]
    fn test_preprocess_template_converts_startswith() {
        let input = r#"message.content.startswith('<tool_response>')"#;
        let output = preprocess_template(input);
        assert_eq!(output, r#"message.content is startingwith('<tool_response>')"#);
    }

    #[test]
    fn test_preprocess_template_converts_strip() {
        let input = r#"{{ content.strip() }}"#;
        let output = preprocess_template(input);
        assert_eq!(output, r#"{{ content | trim }}"#);
    }

    #[test]
    fn test_simple_chatml_jinja_render() {
        // Minimal ChatML template for testing
        let template = r#"{%- for message in messages %}
<|im_start|>{{ message.role }}
{{ message.content }}<|im_end|>
{%- endfor %}
{%- if add_generation_prompt %}
<|im_start|>assistant
{%- endif %}"#;

        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "You are helpful.".to_string(),
                tool_calls: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: "Hello!".to_string(),
                tool_calls: None,
            },
        ];

        let result = apply_native_chat_template(
            template,
            messages,
            None,
            None,
            true,
            "<s>",
            "</s>",
        );
        assert!(result.is_ok());
        let prompt = result.unwrap();
        assert!(prompt.contains("<|im_start|>system"));
        assert!(prompt.contains("You are helpful."));
        assert!(prompt.contains("<|im_start|>user"));
        assert!(prompt.contains("Hello!"));
        assert!(prompt.contains("<|im_start|>assistant"));
    }

    #[test]
    fn test_raise_exception_works() {
        let template = r#"{% if true %}{{ raise_exception("test error") }}{% endif %}"#;
        let messages = vec![];
        let result = apply_native_chat_template(template, messages, None, None, false, "", "");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("test error"));
    }

    #[test]
    fn test_strftime_now_works() {
        let template = r#"{{ strftime_now("%Y-%m-%d") }}"#;
        let messages = vec![];
        let result = apply_native_chat_template(template, messages, None, None, false, "", "");
        assert!(result.is_ok());
        let date = result.unwrap();
        // Should be a valid YYYY-MM-DD format
        assert_eq!(date.len(), 10);
        assert_eq!(&date[4..5], "-");
        assert_eq!(&date[7..8], "-");
    }

    #[test]
    fn test_epoch_days_to_ymd() {
        // 2026-03-01 = day 20513 since epoch
        let (y, m, d) = epoch_days_to_ymd(20513);
        assert_eq!((y, m, d), (2026, 3, 1));
        // 1970-01-01 = day 0
        let (y, m, d) = epoch_days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }
}
