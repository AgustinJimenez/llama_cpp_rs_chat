use minijinja::{context, Environment};
use serde_json::{json, Value};
use crate::log_debug;

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
        .map_err(|e| format!("Failed to parse chat template: {}", e))?;

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
        .map_err(|e| format!("Failed to get template: {}", e))?;
    
    template.render(&template_context)
        .map_err(|e| format!("Failed to render template: {}", e))
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
            "name": "execute_command",
            "description": "Execute system commands",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The command to execute"
                    }
                },
                "required": ["command"]
            }
        })
    ]
}

/// Extract system prompt that might be embedded in the chat template
pub fn extract_embedded_system_prompt(template: &str) -> Option<String> {
    // Look for common patterns in Jinja2 templates that define system messages
    if let Some(start) = template.find("set default_system_message = '") {
        let after_start = &template[start + "set default_system_message = '".len()..];
        if let Some(end) = after_start.find("'") {
            return Some(after_start[..end].to_string());
        }
    }

    // Look for other patterns
    if let Some(start) = template.find("set system_message = \"") {
        let after_start = &template[start + "set system_message = \"".len()..];
        if let Some(end) = after_start.find("\"") {
            return Some(after_start[..end].to_string());
        }
    }

    None
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

    #[test]
    fn test_extract_embedded_system_prompt() {
        let template = r#"
        {%- set default_system_message = 'You are a helpful assistant.' %}
        ...rest of template...
        "#;
        
        let prompt = extract_embedded_system_prompt(template);
        assert_eq!(prompt, Some("You are a helpful assistant.".to_string()));
    }
}