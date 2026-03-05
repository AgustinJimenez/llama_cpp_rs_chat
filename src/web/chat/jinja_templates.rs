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
pub fn get_available_tools_openai() -> Vec<Value> {
    get_available_tools()
        .into_iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": tool
            })
        })
        .collect()
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
    vec![
        json!({
            "name": "read_file",
            "description": "Read the contents of a file. Supports PDF text extraction. Returns the file text (truncated at 100KB for large files).",
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
            "description": "Execute a shell command (git, npm, curl, etc.). Set background=true for commands that run indefinitely (dev servers, watchers, listeners) to avoid blocking.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "background": {
                        "type": "boolean",
                        "description": "Run in background for long-running processes (dev servers, watchers). Returns after 5s with initial output and the PID. Use check_background_process to check on it later."
                    }
                },
                "required": ["command"]
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
            "description": "Check on a background process launched with execute_command(background=true). Returns whether it is still running and any new output since last check.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pid": {
                        "type": "integer",
                        "description": "The PID returned by execute_command with background=true"
                    }
                },
                "required": ["pid"]
            }
        }),
    ]
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
