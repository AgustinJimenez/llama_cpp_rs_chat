use std::num::NonZeroU32;
use tokio::sync::mpsc;
use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_batch::LlamaBatch,
    model::{AddBos, Special},
    sampling::LlamaSampler,
};

use super::models::*;
use super::config::load_config;
use super::command::execute_command;
use super::model_manager::load_model;
use super::utils::get_available_tools_json;

// Constants for LLaMA configuration
const CONTEXT_SIZE: u32 = 32768;
const MODEL_PATH: &str = "/app/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf";

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
            eprintln!("=== QWEN3 SYSTEM PROMPT ===");
            eprintln!("{}", &system_content);
            eprintln!("=== END SYSTEM PROMPT ===");

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
    eprintln!("\n[DEBUG] Template type: {:?}", template_type);
    eprintln!("[DEBUG] Constructed prompt (first 1000 chars):");
    eprintln!("{}", &prompt.chars().take(1000).collect::<String>());

    Ok(prompt)
}

pub async fn generate_llama_response(
    user_message: &str,
    llama_state: SharedLlamaState,
    conversation_logger: SharedConversationLogger,
    token_sender: Option<mpsc::UnboundedSender<TokenData>>,
    skip_user_logging: bool
) -> Result<(String, i32, i32), String> {
    // Log user message to conversation file (unless already logged)
    if !skip_user_logging {
        let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
        logger.log_message("USER", user_message);
    }

    // Load configuration to get model path and context size
    let config = load_config();
    let model_path = config.model_path.as_deref().unwrap_or(MODEL_PATH);
    let stop_tokens = config.stop_tokens.unwrap_or_else(get_common_stop_tokens);

    // Ensure model is loaded
    load_model(llama_state.clone(), model_path).await?;

    // Now use the shared state for generation
    let state_guard = llama_state.lock().map_err(|_| "Failed to lock LLaMA state")?;
    let state = state_guard.as_ref().ok_or("LLaMA state not initialized")?;
    let model = state.model.as_ref().ok_or("No model loaded")?;

    // Get context size: prefer user config, fallback to model's context_length, then default
    let requested_context_size = config.context_size
        .or(state.model_context_length)
        .unwrap_or(CONTEXT_SIZE);

    // Validate context size against available VRAM and auto-reduce if needed
    let (context_size, was_reduced) = crate::web::model_manager::calculate_safe_context_size(
        model_path,
        requested_context_size,
        None, // Let it auto-detect VRAM
        state.gpu_layers // Use actual GPU layers from loaded model
    );

    if was_reduced {
        println!("⚠️  [VRAM WARNING] Requested context {} exceeded VRAM capacity", requested_context_size);
        println!("⚠️  [VRAM WARNING] Automatically reduced to {} tokens to prevent crashes", context_size);

        // Log warning to conversation file
        let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
        logger.log_message("SYSTEM", &format!("⚠️ Context Size Reduced"));
        logger.log_message("SYSTEM", &format!("Requested: {} tokens, but this exceeds available VRAM", requested_context_size));
        logger.log_message("SYSTEM", &format!("Auto-reduced to: {} tokens to prevent memory errors", context_size));
        logger.log_message("SYSTEM", "");
        drop(logger);
    }

    println!("Using context size: {} (requested: {}, model max: {:?})",
        context_size, requested_context_size, state.model_context_length);

    // Create sampler based on configuration
    let mut sampler = match config.sampler_type.as_str() {
        "Temperature" => {
            println!("Using Temperature sampler: temp={}", config.temperature);
            LlamaSampler::temp(config.temperature as f32)
        }
        "Mirostat" => {
            println!("Using Mirostat sampler: tau={}, eta={}", config.mirostat_tau, config.mirostat_eta);
            LlamaSampler::mirostat(
                0,    // n_vocab
                1234, // seed
                config.mirostat_tau as f32,
                config.mirostat_eta as f32,
                100,  // m
            )
        }
        "Greedy" | _ => {
            println!("Using Greedy sampler (default)");
            LlamaSampler::greedy()
        }
    };

    // Read conversation history from file and create chat prompt
    let conversation_content = {
        let logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
        logger.load_conversation_from_file().unwrap_or_else(|_| logger.get_full_conversation())
    };

    // Convert conversation to chat format using model's chat template
    let template_type = state.chat_template_type.clone();
    let prompt = apply_model_chat_template(&conversation_content, template_type.as_deref())?;

    // Tokenize
    let tokens = model
        .str_to_token(&prompt, AddBos::Never)
        .map_err(|e| format!("Tokenization failed: {}", e))?;

    // Create context with configured size
    let n_ctx = NonZeroU32::new(context_size).unwrap();
    let ctx_params = LlamaContextParams::default().with_n_ctx(Some(n_ctx));
    let mut context = model
        .new_context(&state.backend, ctx_params)
        .map_err(|e| format!("Context creation failed: {}", e))?;

    // Prepare batch
    let batch_size = std::cmp::min(tokens.len() + 512, 2048);
    let mut batch = LlamaBatch::new(batch_size, 1);
    for (i, &token) in tokens.iter().enumerate() {
        let is_last = i == tokens.len() - 1;
        batch
            .add(token, i as i32, &[0], is_last)
            .map_err(|e| format!("Batch add failed: {}", e))?;
    }

    // Process initial tokens
    context
        .decode(&mut batch)
        .map_err(|e| format!("Initial decode failed: {}", e))?;

    // Start assistant message in conversation log
    {
        let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
        logger.log_message("ASSISTANT", "");
    }

    // Generate response
    let mut response = String::new();
    let mut token_pos = tokens.len() as i32;
    let mut total_tokens_generated = 0;

    // Calculate max tokens based on remaining context space
    let remaining_context = (context_size as i32) - token_pos - 128;
    let max_total_tokens = remaining_context.max(512);

    println!("Context size: {}, Prompt tokens: {}, Max tokens to generate: {}",
             context_size, token_pos, max_total_tokens);

    // Outer loop to handle command execution and continuation
    loop {
        let mut command_executed = false;
        let mut hit_stop_condition = false;

        // Inner loop for token generation
        let tokens_to_generate = std::cmp::min(2048, max_total_tokens - total_tokens_generated);

        println!("[DEBUG] Starting generation cycle: tokens_to_generate={}, total_tokens_generated={}",
                 tokens_to_generate, total_tokens_generated);

        for _i in 0..tokens_to_generate {
            // Sample next token
            let next_token = sampler.sample(&context, -1);

            // Check for end-of-sequence token
            if next_token == model.token_eos() {
                println!("Debug: Stopping generation - EOS token detected (token ID: {})", next_token);
                hit_stop_condition = true;
                break;
            }

            // Add token to batch and decode
            batch.clear();
            batch
                .add(next_token, token_pos, &[0], true)
                .map_err(|e| format!("Batch add failed: {}", e))?;

            context
                .decode(&mut batch)
                .map_err(|e| format!("Decode failed: {}", e))?;

            token_pos += 1;
            total_tokens_generated += 1;

            // Convert to string for display
            let token_str = match model.token_to_str(next_token, Special::Tokenize) {
                Ok(s) => s,
                Err(e) => {
                    println!("[WARN] Token {} can't be displayed as UTF-8: {}. Continuing generation.", next_token, e);
                    continue;
                }
            };

            // Check for stop sequences
            let test_response = format!("{}{}", response, token_str);
            let mut should_stop = false;
            let mut partial_to_remove = 0;
            let in_command_block = response.contains("<COMMAND>") && !response.contains("</COMMAND>");

            for stop_token in &stop_tokens {
                if test_response.contains(stop_token) {
                    println!("Debug: Stopping generation due to stop token detected: '{}'", stop_token);

                    // Special case for </COMMAND>
                    if stop_token == "</COMMAND>" {
                        response.push_str(&token_str);
                        let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
                        logger.log_token(&token_str);
                    }

                    should_stop = true;
                    break;
                }

                // Handle partial matches
                if in_command_block && (stop_token.starts_with("</") || stop_token.starts_with("[/")) {
                    continue;
                }

                if stop_token == "</COMMAND>" {
                    continue;
                }

                if stop_token.len() > 2 {
                    let trimmed = test_response.trim_end();
                    for i in 2..stop_token.len() {
                        if trimmed.ends_with(&stop_token[..i]) {
                            if response.trim_end().ends_with(&stop_token[..i-token_str.len()]) && i > token_str.len() {
                                partial_to_remove = i - token_str.len();
                            }
                            should_stop = true;
                            break;
                        }
                    }
                    if should_stop {
                        break;
                    }
                }
            }

            if should_stop {
                if partial_to_remove > 0 {
                    let new_len = response.len().saturating_sub(partial_to_remove);
                    response.truncate(new_len);
                }
                hit_stop_condition = true;
                break;
            }

            // Add token to response
            response.push_str(&token_str);

            // Stream token
            if let Some(ref sender) = token_sender {
                let token_data = TokenData {
                    token: token_str.clone(),
                    tokens_used: token_pos,
                    max_tokens: context_size as i32,
                };
                let _ = sender.send(token_data);
            }

            // Log token
            {
                let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
                logger.log_token(&token_str);
            }
        }

        // Check for command execution
        if response.contains("<COMMAND>") && response.contains("</COMMAND>") {
            if let Some(start) = response.find("<COMMAND>") {
                if let Some(end) = response.find("</COMMAND>") {
                    if end > start {
                        let command_text = &response[start + 9..end];
                        println!("Debug: Executing command: {}", command_text);

                        let output = execute_command(command_text);
                        println!("Debug: Command output: {}", output);

                        // Log command execution
                        {
                            let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
                            logger.log_command_execution(command_text, &output);
                        }

                        // Replace command with output
                        let before_command = &response[..start];
                        let after_command = &response[end + 10..];
                        let command_output_text = format!(
                            "\n\n[COMMAND: {}]\n\n```\n{}\n```\n\n",
                            command_text,
                            output.trim()
                        );

                        response = format!("{}{}{}", before_command.trim(), command_output_text, after_command);

                        // Log output
                        {
                            let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
                            logger.log_token(&command_output_text);
                        }

                        // Feed command output to context
                        let output_tokens = model
                            .str_to_token(&command_output_text, AddBos::Never)
                            .map_err(|e| format!("Tokenization of command output failed: {}", e))?;

                        for token in output_tokens {
                            batch.clear();
                            batch
                                .add(token, token_pos, &[0], true)
                                .map_err(|e| format!("Batch add failed for command output: {}", e))?;

                            context
                                .decode(&mut batch)
                                .map_err(|e| format!("Decode failed for command output: {}", e))?;

                            token_pos += 1;
                        }

                        command_executed = true;
                    }
                }
            }
        }

        // Break conditions
        if hit_stop_condition || total_tokens_generated >= max_total_tokens {
            break;
        }

        if !command_executed {
            println!("[DEBUG] Continuing generation: no stop condition hit");
        }
    }

    // Finish assistant message
    {
        let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
        logger.finish_assistant_message();
    }

    Ok((response.trim().to_string(), token_pos, max_total_tokens))
}
