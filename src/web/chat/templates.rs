use super::super::utils::get_available_tools_json;
use crate::{log_debug};

/// Apply chat template formatting to conversation history.
///
/// Parses conversation text and formats it according to the model's chat template type.
/// Supports ChatML, Mistral, Llama3, and Gemma formats with automatic tool injection.
pub fn apply_model_chat_template(conversation: &str, template_type: Option<&str>) -> Result<String, String> {
    // Parse conversation into messages
    let mut system_message: Option<String> = None;
    let mut user_messages = Vec::new();
    let mut assistant_messages = Vec::new();
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
                match current_role {
                    "SYSTEM" => system_message = Some(current_content.trim().to_string()),
                    "USER" => user_messages.push(current_content.trim().to_string()),
                    "ASSISTANT" => assistant_messages.push(current_content.trim().to_string()),
                    _ => {}
                }
            }

            // Start new role
            current_role = line.trim_end_matches(":");
            current_content.clear();
        } else if !line.starts_with("[COMMAND:") {
            // Skip command execution logs, add content
            if !line.trim().is_empty() {
                current_content.push_str(line);
                current_content.push('\n');
            }
        }
    }

    // Add the final role content
    if !current_role.is_empty() && !current_content.trim().is_empty() {
        match current_role {
            "SYSTEM" => system_message = Some(current_content.trim().to_string()),
            "USER" => user_messages.push(current_content.trim().to_string()),
            "ASSISTANT" => assistant_messages.push(current_content.trim().to_string()),
            _ => {}
        }
    }

    // Construct prompt based on detected template type
    let prompt = match template_type {
        Some("ChatML") => {
            // Qwen/ChatML format: <|im_start|>role\ncontent<|im_end|>
            let mut p = String::new();

            // Add system message with tool definitions in Hermes format for Qwen3
            let mut system_content = system_message.unwrap_or_else(|| "You are a helpful AI assistant.".to_string());

            // Inject tool definitions using Hermes-style format (CORRECT for Qwen3!)
            system_content.push_str("\n\n# Tools\n");
            system_content.push_str("You may call one or more functions to assist with the user query.\n\n");
            system_content.push_str("You are provided with function signatures within <tools></tools> XML tags:\n");
            system_content.push_str("<tools>\n");

            // Get tools as JSON array (this is the correct format!)
            let tools_json = get_available_tools_json();
            system_content.push_str(&tools_json);

            system_content.push_str("\n</tools>\n\n");
            system_content.push_str("For each function call, return a json object with function name and arguments within <tool_call></tool_call> XML tags:\n");
            system_content.push_str("<tool_call>\n");
            system_content.push_str("{\"name\": <function-name>, \"arguments\": <args-json-object>}\n");
            system_content.push_str("</tool_call>\n");

            p.push_str("<|im_start|>system\n");
            p.push_str(&system_content);
            p.push_str("<|im_end|>\n");

            // DEBUG: Print the system prompt being sent to Qwen3
            log_debug!("=== QWEN3 SYSTEM PROMPT ===");
            log_debug!("{}", &system_content);
            log_debug!("=== END SYSTEM PROMPT ===");

            // Add conversation history
            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("<|im_start|>user\n");
                    p.push_str(&user_messages[i]);
                    p.push_str("<|im_end|>\n");
                }
                if i < assistant_messages.len() {
                    p.push_str("<|im_start|>assistant\n");
                    p.push_str(&assistant_messages[i]);
                    p.push_str("<|im_end|>\n");
                }
            }

            // Add generation prompt
            p.push_str("<|im_start|>assistant\n");

            p
        }
        Some("Mistral") | None => {
            // Mistral format: <s>[INST] user [/INST] assistant </s>
            let mut p = String::new();
            p.push_str("<s>");

            // Add system prompt if present
            if let Some(sys_msg) = system_message {
                p.push_str("[SYSTEM_PROMPT]");
                p.push_str(&sys_msg);
                p.push_str("[/SYSTEM_PROMPT]");
            }

            // Inject tool definitions for Mistral-style models (Devstral, etc.)
            // This enables the model to understand available tools and generate tool calls
            let tools_json = get_available_tools_json();
            p.push_str("[AVAILABLE_TOOLS]");
            p.push_str(&tools_json);
            p.push_str("[/AVAILABLE_TOOLS]");

            // Add conversation history
            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("[INST]");
                    p.push_str(&user_messages[i]);
                    p.push_str("[/INST]");
                }
                if i < assistant_messages.len() {
                    p.push_str(&assistant_messages[i]);
                    p.push_str("</s>");
                }
            }

            p
        }
        Some("Llama3") => {
            // Llama 3 format
            let mut p = String::new();
            p.push_str("<|begin_of_text|>");

            if let Some(sys_msg) = system_message {
                p.push_str("<|start_header_id|>system<|end_header_id|>\n\n");
                p.push_str(&sys_msg);
                p.push_str("<|eot_id|>");
            }

            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("<|start_header_id|>user<|end_header_id|>\n\n");
                    p.push_str(&user_messages[i]);
                    p.push_str("<|eot_id|>");
                }
                if i < assistant_messages.len() {
                    p.push_str("<|start_header_id|>assistant<|end_header_id|>\n\n");
                    p.push_str(&assistant_messages[i]);
                    p.push_str("<|eot_id|>");
                }
            }

            p.push_str("<|start_header_id|>assistant<|end_header_id|>\n\n");

            p
        }
        Some("Gemma") => {
            // Gemma 3 format: <start_of_turn>role\ncontent<end_of_turn>\n
            // Note: Gemma uses "model" instead of "assistant"
            let mut p = String::new();

            // Add system message as first user message prefix if present
            let first_user_prefix = if let Some(sys_msg) = system_message {
                format!("{}\n\n", sys_msg)
            } else {
                String::new()
            };

            // Add conversation history
            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("<start_of_turn>user\n");
                    // Add system prompt prefix to first user message
                    if i == 0 && !first_user_prefix.is_empty() {
                        p.push_str(&first_user_prefix);
                    }
                    p.push_str(&user_messages[i]);
                    p.push_str("<end_of_turn>\n");
                }
                if i < assistant_messages.len() {
                    p.push_str("<start_of_turn>model\n");
                    p.push_str(&assistant_messages[i]);
                    p.push_str("<end_of_turn>\n");
                }
            }

            // Add generation prompt
            p.push_str("<start_of_turn>model\n");

            p
        }
        Some(_) => {
            // Generic fallback - use ChatML-style
            let mut p = String::new();

            if let Some(sys_msg) = system_message {
                p.push_str("System: ");
                p.push_str(&sys_msg);
                p.push_str("\n\n");
            }

            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("User: ");
                    p.push_str(&user_messages[i]);
                    p.push_str("\n\n");
                }
                if i < assistant_messages.len() {
                    p.push_str("Assistant: ");
                    p.push_str(&assistant_messages[i]);
                    p.push_str("\n\n");
                }
            }

            p.push_str("Assistant: ");

            p
        }
    };

    // Debug: Print first 1000 chars of prompt
    log_debug!("\nTemplate type: {:?}", template_type);
    log_debug!("Constructed prompt (first 1000 chars):");
    log_debug!("{}", &prompt.chars().take(1000).collect::<String>());

    Ok(prompt)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Full template formatting is tested in E2E tests with real models.
    // These unit tests focus on basic parsing behavior.

    #[test]
    fn test_template_preserves_multiline_content() {
        let conversation = "USER:\nLine 1\nLine 2\nLine 3";
        let result = apply_model_chat_template(conversation, Some("chatml")).unwrap();

        assert!(result.contains("Line 1"));
        assert!(result.contains("Line 2"));
        assert!(result.contains("Line 3"));
    }

    #[test]
    fn test_template_handles_empty_content() {
        let conversation = "USER:\n\n\nASSISTANT:\n";
        let result = apply_model_chat_template(conversation, Some("chatml"));

        // Should not error, should handle empty content gracefully
        assert!(result.is_ok());
    }

    #[test]
    fn test_template_function_returns_string() {
        // Basic smoke test - function should return Ok with any valid input
        let conversation = "USER:\nTest message";
        let result = apply_model_chat_template(conversation, None);

        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }
}
