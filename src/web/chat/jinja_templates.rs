use minijinja::{context, Environment};
use serde_json::{json, Value};

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
) -> Result<String, String> {
    // Create Jinja2 environment
    let mut env = Environment::new();
    
    // Add the template
    env.add_template("chat_template", template_string)
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
        bos_token => "<s>",
        eos_token => "</s>",
    };

    // Render the template
    let template = env.get_template("chat_template")
        .map_err(|e| format!("Failed to get template: {e}"))?;
    
    template.render(&template_context)
        .map_err(|e| format!("Failed to render template: {e}"))
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

/// Parse conversation text into ChatMessage format
pub fn parse_conversation_to_messages(conversation: &str) -> Vec<ChatMessage> {
    let mut messages = Vec::new();
    let mut current_role = "";
    let mut current_content = String::new();

    for line in conversation.lines() {
        if line.ends_with(":")
            && (line.starts_with("SYSTEM:")
                || line.starts_with("USER:")
                || line.starts_with("ASSISTANT:"))
        {
            // Save previous role's content
            if !current_role.is_empty() && !current_content.trim().is_empty() {
                messages.push(ChatMessage {
                    role: current_role.to_lowercase(),
                    content: current_content.trim().to_string(),
                    tool_calls: None,
                });
            }

            // Set new role
            current_role = line.trim_end_matches(':');
            current_content.clear();
        } else if !current_role.is_empty() {
            if !current_content.is_empty() {
                current_content.push('\n');
            }
            current_content.push_str(line);
        }
    }

    // Add final message
    if !current_role.is_empty() && !current_content.trim().is_empty() {
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
            "description": "Read the contents of a file. Returns the file text (truncated at 100KB for large files).",
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
            "description": "Execute a shell command (git, npm, curl, etc.). Use this for commands that are not covered by other tools.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
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
            "description": "Search the web using DuckDuckGo. Returns a list of results with titles, URLs, and descriptions. Use this to find current information, documentation, or answers.",
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
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_conversation() {
        let conversation = r#"USER:
Hello, how are you?

ASSISTANT:
I'm doing well, thank you for asking!

USER:
Can you help me with something?"#;

        let messages = parse_conversation_to_messages(conversation);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "Hello, how are you?");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].content, "I'm doing well, thank you for asking!");
        assert_eq!(messages[2].role, "user");
        assert_eq!(messages[2].content, "Can you help me with something?");
    }

}