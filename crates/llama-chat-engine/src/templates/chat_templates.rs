use crate::tool_tags::ToolTags;
use llama_chat_tools::McpToolDefInfo as McpToolDef;
use super::system_prompts::get_universal_system_prompt_with_tags;

/// Apply chat template formatting to conversation history (uses default tags).
#[cfg(test)]
pub fn apply_model_chat_template(
    conversation: &str,
    template_type: Option<&str>,
) -> Result<String, String> {
    use crate::tool_tags;
    apply_model_chat_template_with_tags(conversation, template_type, &tool_tags::default_tags(), None, None)
}

/// Apply chat template formatting to conversation history.
pub fn apply_model_chat_template_with_tags(
    conversation: &str,
    template_type: Option<&str>,
    tags: &ToolTags,
    mcp_tools: Option<&[McpToolDef]>,
    custom_system_prompt: Option<&str>,
) -> Result<String, String> {
    let mut user_messages = Vec::new();
    let mut assistant_messages = Vec::new();
    let mut compaction_summaries: Vec<String> = Vec::new();
    let mut current_role = "";
    let mut current_content = String::new();

    for line in conversation.lines() {
        if line.ends_with(":")
            && (line.starts_with("SYSTEM:")
                || line.starts_with("USER:")
                || line.starts_with("ASSISTANT:"))
        {
            if !current_role.is_empty() && !current_content.trim().is_empty() {
                match current_role {
                    "USER" => user_messages.push(current_content.trim().to_string()),
                    "ASSISTANT" => assistant_messages.push(current_content.trim().to_string()),
                    "SYSTEM" => {
                        if current_content.trim().starts_with("[Conversation summary") {
                            compaction_summaries.push(current_content.trim().to_string());
                        }
                    }
                    _ => {}
                }
            }

            current_role = line.trim_end_matches(":");
            current_content.clear();
        } else if !line.starts_with("[COMMAND:") {
            if !line.trim().is_empty() {
                current_content.push_str(line);
                current_content.push('\n');
            }
        }
    }

    if !current_role.is_empty() && !current_content.trim().is_empty() {
        match current_role {
            "USER" => user_messages.push(current_content.trim().to_string()),
            "ASSISTANT" => assistant_messages.push(current_content.trim().to_string()),
            "SYSTEM" => {
                if current_content.trim().starts_with("[Conversation summary") {
                    compaction_summaries.push(current_content.trim().to_string());
                }
            }
            _ => {}
        }
    }

    let universal_prompt = match custom_system_prompt {
        Some(custom) => custom.to_string(),
        None => get_universal_system_prompt_with_tags(tags),
    };

    let mut final_system_message = universal_prompt;
    for summary in &compaction_summaries {
        final_system_message.push_str("\n\n---\n");
        final_system_message.push_str(summary);
    }

    if let Some(mcp) = mcp_tools {
        if !mcp.is_empty() {
            let exec_open = &tags.exec_open;
            let exec_close = &tags.exec_close;
            final_system_message.push_str("\n\n## MCP (External) Tools\n\n");
            final_system_message.push_str("The following tools are provided by external MCP servers:\n\n");
            for tool in mcp {
                final_system_message.push_str(&format!(
                    "### {} — [MCP:{}] {}\n{exec_open}{{\"name\": \"{}\", \"arguments\": <see schema>}}{exec_close}\nParameters: {}\n\n",
                    tool.qualified_name,
                    tool.server_name,
                    tool.description,
                    tool.qualified_name,
                    serde_json::to_string(&tool.input_schema).unwrap_or_else(|_| "{}".to_string()),
                ));
            }
        }
    }

    let prompt = match template_type {
        Some("ChatML") => {
            let mut p = String::new();

            p.push_str("<|im_start|>system\n");
            p.push_str(&final_system_message);
            p.push_str("<|im_end|>\n");

            log_debug!("system", "=== CHATML SYSTEM PROMPT ===");
            log_debug!("system", "{}", &final_system_message);
            log_debug!("system", "=== END SYSTEM PROMPT ===");

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

            p.push_str("<|im_start|>assistant\n");
            p
        }
        Some("Mistral") | None => {
            let mut p = String::new();
            p.push_str("<s>");

            p.push_str("[SYSTEM_PROMPT]");
            p.push_str(&final_system_message);
            p.push_str("[/SYSTEM_PROMPT]");

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
            let mut p = String::new();
            p.push_str("<|begin_of_text|>");

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
            let mut p = String::new();

            let first_user_prefix = format!("{final_system_message}\n\n");

            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("<start_of_turn>user\n");
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

            p.push_str("<start_of_turn>model\n");
            p
        }
        Some("Phi") => {
            let mut p = String::new();

            p.push_str("<|system|>\n");
            p.push_str(&final_system_message);
            p.push_str("<|end|>\n");

            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("<|user|>\n");
                    p.push_str(&user_messages[i]);
                    p.push_str("<|end|>\n");
                }
                if i < assistant_messages.len() {
                    p.push_str("<|assistant|>\n");
                    p.push_str(&assistant_messages[i]);
                    p.push_str("<|end|>\n");
                }
            }

            p.push_str("<|assistant|>\n");
            p
        }
        Some("GLM") => {
            let mut p = String::new();

            p.push_str("[gMASK]<sop><|system|>\n");
            p.push_str(&final_system_message);

            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("<|user|>\n");
                    p.push_str(&user_messages[i]);
                }
                if i < assistant_messages.len() {
                    p.push_str("<|assistant|>\n");
                    p.push_str(&assistant_messages[i]);
                }
            }

            p.push_str("<|assistant|>\n");
            p
        }
        Some(_) => {
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

    log_debug!("system", "\nTemplate type: {:?}", template_type);
    log_debug!("system", "Constructed prompt (first 1000 chars):");
    log_debug!(
        "system",
        "{}",
        &prompt.chars().take(1000).collect::<String>()
    );

    Ok(prompt)
}
