// OLD: use super::super::utils::get_available_tools_json;
use crate::log_debug;
use super::jinja_templates::{apply_native_chat_template, parse_conversation_to_messages, get_available_tools};
use super::super::models::SystemPromptType;

/// Get the universal system prompt for command execution.
/// This prompt teaches ANY model how to use the <||SYSTEM.EXEC> tags.
pub fn get_universal_system_prompt() -> String {
    let os_name = std::env::consts::OS;
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_string());

    let shell = if os_name == "windows" {
        "cmd/powershell"
    } else {
        "bash"
    };

    // OS-specific command examples
    let (list_cmd, read_cmd, write_cmd) = if os_name == "windows" {
        ("dir", "type filename.txt", "echo content > filename.txt")
    } else {
        (
            "ls -la",
            "cat filename.txt",
            "echo 'content' > filename.txt",
        )
    };

    format!(
        r#"You are a helpful AI assistant with full system access.

## CRITICAL: Command Execution Format

To execute system commands, you MUST use EXACTLY this format (copy it exactly):

<||SYSTEM.EXEC>command_here<SYSTEM.EXEC||>

The format is: opening tag <||SYSTEM.EXEC> then command then closing tag <SYSTEM.EXEC||>

IMPORTANT RULES:
1. Use ONLY this exact format - do NOT use [TOOL_CALLS], <function>, <tool_call>, or any other format
2. The opening tag MUST start with <|| (less-than, pipe, pipe)
3. The closing tag MUST end with ||> (pipe, pipe, greater-than)
4. Do NOT add any prefix before <||SYSTEM.EXEC>
5. Do NOT modify or abbreviate the tags

Examples (copy exactly):
<||SYSTEM.EXEC>{list_cmd}<SYSTEM.EXEC||>
<||SYSTEM.EXEC>{read_cmd}<SYSTEM.EXEC||>
<||SYSTEM.EXEC>{write_cmd}<SYSTEM.EXEC||>

After execution, the output will appear in:
<||SYSTEM.OUTPUT>
...output here...
<SYSTEM.OUTPUT||>

Wait for the output before continuing your response.

## Web Browsing

You can fetch web pages to read their content. Use this to find download URLs, read documentation, or investigate errors:

<||SYSTEM.EXEC>curl "http://localhost:8000/api/tools/web-fetch?url=https://example.com"<SYSTEM.EXEC||>

This returns the page content as clean text (HTML is stripped). Use this to:
- Find correct download URLs instead of guessing
- Read documentation and installation instructions
- Search for solutions to errors
- Verify URLs before downloading from them

## Current Environment
- OS: {os_name}
- Working Directory: {cwd}
- Shell: {shell}
"#,
        list_cmd = list_cmd,
        read_cmd = read_cmd,
        write_cmd = write_cmd,
        os_name = os_name,
        cwd = cwd,
        shell = shell
    )
}

/// Apply system prompt based on the selected type
/// 
/// This is the main function that handles all 3 system prompt types:
/// - Default: Use model's native Jinja2 chat template  
/// - Custom: Use our curated universal system prompt
/// - UserDefined: Use user-provided system prompt
pub fn apply_system_prompt_by_type(
    conversation: &str,
    prompt_type: SystemPromptType,
    template_type: Option<&str>,
    chat_template_string: Option<&str>,
    user_system_prompt: Option<&str>,
) -> Result<String, String> {
    match prompt_type {
        SystemPromptType::Default => {
            // Try to use model's native Jinja2 template first
            if let Some(template) = chat_template_string {
                log_debug!("templates", "Using native Jinja2 chat template");
                
                let messages = parse_conversation_to_messages(conversation);
                let tools = Some(get_available_tools());
                
                match apply_native_chat_template(
                    template, 
                    messages, 
                    tools, 
                    None, // documents
                    true  // add_generation_prompt
                ) {
                    Ok(result) => return Ok(result),
                    Err(e) => {
                        log_debug!("templates", "Jinja2 template failed: {}, falling back to custom logic", e);
                        // Fall back to custom implementation
                    }
                }
            }
            
            // Fallback to your existing custom template logic
            log_debug!("templates", "Using fallback custom template logic");
            apply_model_chat_template(conversation, template_type)
        }
        
        SystemPromptType::Custom => {
            // Use your curated universal system prompt
            log_debug!("templates", "Using custom universal system prompt");
            apply_model_chat_template(conversation, template_type)
        }
        
        SystemPromptType::UserDefined => {
            // Use user-provided system prompt
            log_debug!("templates", "Using user-defined system prompt");
            if let Some(user_prompt) = user_system_prompt {
                apply_user_defined_template(conversation, user_prompt)
            } else {
                // Fallback if no user prompt provided
                apply_model_chat_template(conversation, template_type)
            }
        }
    }
}

/// Apply user-defined system prompt
fn apply_user_defined_template(conversation: &str, user_system_prompt: &str) -> Result<String, String> {
    // Parse conversation into messages  
    let system_message: Option<String> = Some(user_system_prompt.to_string());
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
                    "SYSTEM" => {
                        // Override with user-defined prompt instead
                    }
                    "USER" => user_messages.push(current_content.trim().to_string()),
                    "ASSISTANT" => assistant_messages.push(current_content.trim().to_string()),
                    _ => {}
                }
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

    // Add final message
    if !current_role.is_empty() && !current_content.trim().is_empty() {
        match current_role {
            "USER" => user_messages.push(current_content.trim().to_string()),
            "ASSISTANT" => assistant_messages.push(current_content.trim().to_string()),
            _ => {}
        }
    }

    // Format using simple template with user's system prompt
    let mut formatted = String::new();
    
    if let Some(sys_msg) = system_message {
        formatted.push_str(&format!("<|start_of_role|>system<|end_of_role|>{}<|end_of_text|>\n", sys_msg));
    }

    // Interleave user and assistant messages
    let max_len = user_messages.len().max(assistant_messages.len());
    for i in 0..max_len {
        if i < user_messages.len() {
            formatted.push_str(&format!("<|start_of_role|>user<|end_of_role|>{}<|end_of_text|>\n", user_messages[i]));
        }
        if i < assistant_messages.len() {
            formatted.push_str(&format!("<|start_of_role|>assistant<|end_of_role|>{}<|end_of_text|>\n", assistant_messages[i]));
        }
    }

    formatted.push_str("<|start_of_role|>assistant<|end_of_role|>");
    Ok(formatted)
}

/// Apply chat template formatting to conversation history.
///
/// Parses conversation text and formats it according to the model's chat template type.
/// Now uses universal SYSTEM.EXEC prompt for all models instead of model-specific tool injection.
pub fn apply_model_chat_template(
    conversation: &str,
    template_type: Option<&str>,
) -> Result<String, String> {
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
            // Skip old command execution logs, add content
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

    // Get the universal system prompt (same for ALL models)
    let universal_prompt = get_universal_system_prompt();

    // Combine user's system message (if any) with our universal prompt
    let final_system_message = match system_message {
        Some(user_sys) => format!("{}\n\n{}", user_sys, universal_prompt),
        None => universal_prompt,
    };

    // Construct prompt based on detected template type
    let prompt = match template_type {
        Some("ChatML") => {
            // Qwen/ChatML format: <|im_start|>role\ncontent<|im_end|>
            let mut p = String::new();

            // System message with universal SYSTEM.EXEC prompt
            p.push_str("<|im_start|>system\n");
            p.push_str(&final_system_message);
            p.push_str("<|im_end|>\n");

            // DEBUG: Print the system prompt
            log_debug!("system", "=== CHATML SYSTEM PROMPT ===");
            log_debug!("system", "{}", &final_system_message);
            log_debug!("system", "=== END SYSTEM PROMPT ===");

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

            // System prompt with universal SYSTEM.EXEC instructions
            p.push_str("[SYSTEM_PROMPT]");
            p.push_str(&final_system_message);
            p.push_str("[/SYSTEM_PROMPT]");

            // OLD: Tool injection commented out
            // let tools_json = get_available_tools_json();
            // p.push_str("[AVAILABLE_TOOLS]");
            // p.push_str(&tools_json);
            // p.push_str("[/AVAILABLE_TOOLS]");

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

            // System message with universal SYSTEM.EXEC prompt
            p.push_str("<|start_header_id|>system<|end_header_id|>\n\n");
            p.push_str(&final_system_message);
            p.push_str("<|eot_id|>");

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

            // Gemma doesn't have a system role, so prepend to first user message
            let first_user_prefix = format!("{}\n\n", final_system_message);

            // Add conversation history
            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("<start_of_turn>user\n");
                    // Add system prompt prefix to first user message
                    if i == 0 {
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
            // Generic fallback
            let mut p = String::new();

            p.push_str("System: ");
            p.push_str(&final_system_message);
            p.push_str("\n\n");

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
    log_debug!("system", "\nTemplate type: {:?}", template_type);
    log_debug!("system", "Constructed prompt (first 1000 chars):");
    log_debug!(
        "system",
        "{}",
        &prompt.chars().take(1000).collect::<String>()
    );

    Ok(prompt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_universal_system_prompt_contains_exec_tags() {
        let prompt = get_universal_system_prompt();
        assert!(prompt.contains("<||SYSTEM.EXEC>"));
        assert!(prompt.contains("<SYSTEM.EXEC||>"));
        assert!(prompt.contains("<||SYSTEM.OUTPUT>"));
        assert!(prompt.contains("<SYSTEM.OUTPUT||>"));
    }

    #[test]
    fn test_universal_system_prompt_contains_os_info() {
        let prompt = get_universal_system_prompt();
        // Should contain OS info
        assert!(prompt.contains("OS:"));
        assert!(prompt.contains("Working Directory:"));
        assert!(prompt.contains("Shell:"));
    }

    #[test]
    fn test_template_preserves_multiline_content() {
        let conversation = "USER:\nLine 1\nLine 2\nLine 3";
        let result = apply_model_chat_template(conversation, Some("ChatML")).unwrap();

        assert!(result.contains("Line 1"));
        assert!(result.contains("Line 2"));
        assert!(result.contains("Line 3"));
    }

    #[test]
    fn test_template_handles_empty_content() {
        let conversation = "USER:\n\n\nASSISTANT:\n";
        let result = apply_model_chat_template(conversation, Some("ChatML"));

        // Should not error, should handle empty content gracefully
        assert!(result.is_ok());
    }

    #[test]
    fn test_template_includes_universal_prompt() {
        let conversation = "USER:\nTest message";
        let result = apply_model_chat_template(conversation, Some("ChatML")).unwrap();

        // Should include the universal SYSTEM.EXEC tags
        assert!(result.contains("<||SYSTEM.EXEC>"));
    }

    #[test]
    fn test_all_templates_include_system_exec() {
        let conversation = "USER:\nTest message";

        for template in &["ChatML", "Mistral", "Llama3", "Gemma"] {
            let result = apply_model_chat_template(conversation, Some(template)).unwrap();
            assert!(
                result.contains("<||SYSTEM.EXEC>"),
                "Template {} should include SYSTEM.EXEC",
                template
            );
        }
    }
}
