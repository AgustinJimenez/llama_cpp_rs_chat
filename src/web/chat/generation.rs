use std::num::NonZeroU32;
use tokio::sync::mpsc;
use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_batch::LlamaBatch,
    model::{AddBos, Special},
    sampling::LlamaSampler,
};

use super::super::models::*;
use super::super::config::load_config;
use super::super::command::execute_command;
use super::super::model_manager::load_model;
use super::templates::apply_model_chat_template;
use crate::{log_debug, log_info, log_warn};

// Constants for LLaMA configuration
const CONTEXT_SIZE: u32 = 32768;
const MODEL_PATH: &str = "/app/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf";

/// Generate response from LLaMA model with streaming support.
///
/// Handles token generation, stop conditions, command execution, and conversation logging.
/// Supports multiple sampling strategies and automatic context size validation.
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
        log_warn!("⚠️  Requested context {} exceeded VRAM capacity", requested_context_size);
        log_warn!("⚠️  Automatically reduced to {} tokens to prevent crashes", context_size);

        // Log warning to conversation file
        let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
        logger.log_message("SYSTEM", &format!("⚠️ Context Size Reduced"));
        logger.log_message("SYSTEM", &format!("Requested: {} tokens, but this exceeds available VRAM", requested_context_size));
        logger.log_message("SYSTEM", &format!("Auto-reduced to: {} tokens to prevent memory errors", context_size));
        logger.log_message("SYSTEM", "");
        drop(logger);
    }

    log_info!("Using context size: {} (requested: {}, model max: {:?})",
        context_size, requested_context_size, state.model_context_length);

    // Create sampler based on configuration
    let mut sampler = match config.sampler_type.as_str() {
        "Temperature" => {
            log_info!("Using Temperature sampler: temp={}", config.temperature);
            LlamaSampler::temp(config.temperature as f32)
        }
        "Mirostat" => {
            log_info!("Using Mirostat sampler: tau={}, eta={}", config.mirostat_tau, config.mirostat_eta);
            LlamaSampler::mirostat(
                0,    // n_vocab
                1234, // seed
                config.mirostat_tau as f32,
                config.mirostat_eta as f32,
                100,  // m
            )
        }
        "Greedy" | _ => {
            log_info!("Using Greedy sampler (default)");
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
    log_debug!("Step 1: Applying chat template (type: {:?})", template_type);
    let prompt = apply_model_chat_template(&conversation_content, template_type.as_deref())?;
    log_debug!("Step 1 complete: Prompt length = {} chars", prompt.len());

    // Tokenize
    log_debug!("Step 2: Starting tokenization...");
    let tokens = model
        .str_to_token(&prompt, AddBos::Never)
        .map_err(|e| format!("Tokenization failed: {}", e))?;
    log_debug!("Step 2 complete: Tokenized to {} tokens", tokens.len());

    // Create context with configured size
    log_debug!("Step 3: Creating context (size={}K tokens)...", context_size / 1024);
    let n_ctx = NonZeroU32::new(context_size)
        .expect("Context size must be non-zero");
    let ctx_params = LlamaContextParams::default().with_n_ctx(Some(n_ctx));
    let mut context = model
        .new_context(&state.backend, ctx_params)
        .map_err(|e| format!("Context creation failed: {}", e))?;
    log_debug!("Step 3 complete: Context created");

    // Prepare batch
    let batch_size = std::cmp::min(tokens.len() + 512, 2048);
    log_debug!("Step 4: Preparing batch (size={}, tokens={})...", batch_size, tokens.len());
    let mut batch = LlamaBatch::new(batch_size, 1);
    for (i, &token) in tokens.iter().enumerate() {
        let is_last = i == tokens.len() - 1;
        batch
            .add(token, i as i32, &[0], is_last)
            .map_err(|e| format!("Batch add failed: {}", e))?;
    }
    log_debug!("Step 4 complete: Batch prepared with {} tokens", tokens.len());

    // Process initial tokens
    log_debug!("Step 5: Starting initial decode...");
    context
        .decode(&mut batch)
        .map_err(|e| format!("Initial decode failed: {}", e))?;
    log_debug!("Step 5 complete: Initial decode successful");

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

    log_info!("Context size: {}, Prompt tokens: {}, Max tokens to generate: {}",
             context_size, token_pos, max_total_tokens);

    // Outer loop to handle command execution and continuation
    loop {
        let mut command_executed = false;
        let mut hit_stop_condition = false;

        // Inner loop for token generation
        let tokens_to_generate = std::cmp::min(2048, max_total_tokens - total_tokens_generated);

        log_debug!("Starting generation cycle: tokens_to_generate={}, total_tokens_generated={}",
                 tokens_to_generate, total_tokens_generated);

        for i in 0..tokens_to_generate {
            // Sample next token
            if i % 50 == 0 {
                log_debug!("Generated {} tokens so far...", total_tokens_generated);
            }

            // Extra logging around the 150 token mark
            if total_tokens_generated >= 145 && total_tokens_generated <= 155 {
                log_debug!("Token #{}: About to sample...", total_tokens_generated);
            }

            let next_token = sampler.sample(&context, -1);

            if total_tokens_generated >= 145 && total_tokens_generated <= 155 {
                log_debug!("Token #{}: Sampled token ID {}", total_tokens_generated, next_token);
            }

            // Check for end-of-sequence token
            if next_token == model.token_eos() {
                log_debug!("Stopping generation - EOS token detected (token ID: {}) at position {}", next_token, total_tokens_generated);
                hit_stop_condition = true;
                break;
            }

            // Add token to batch and decode
            if total_tokens_generated >= 145 && total_tokens_generated <= 155 {
                log_debug!("Token #{}: About to add to batch and decode...", total_tokens_generated);
            }

            batch.clear();
            batch
                .add(next_token, token_pos, &[0], true)
                .map_err(|e| format!("Batch add failed at token {}: {}", total_tokens_generated, e))?;

            context
                .decode(&mut batch)
                .map_err(|e| format!("Decode failed at token {}: {}", total_tokens_generated, e))?;

            if total_tokens_generated >= 145 && total_tokens_generated <= 155 {
                log_debug!("Token #{}: Decode successful", total_tokens_generated);
            }

            token_pos += 1;
            total_tokens_generated += 1;

            // Convert to string for display
            let token_str = match model.token_to_str(next_token, Special::Tokenize) {
                Ok(s) => s,
                Err(e) => {
                    log_warn!("Token {} can't be displayed as UTF-8: {}. Continuing generation.", next_token, e);
                    continue;
                }
            };

            if total_tokens_generated >= 145 && total_tokens_generated <= 155 {
                log_debug!("Token #{}: Converted to string: {:?}", total_tokens_generated, token_str);
            }

            // Check for stop sequences
            let test_response = format!("{}{}", response, token_str);
            let mut should_stop = false;
            let mut partial_to_remove = 0;
            let in_command_block = response.contains("<COMMAND>") && !response.contains("</COMMAND>");

            if total_tokens_generated >= 145 && total_tokens_generated <= 155 {
                log_debug!("Token #{}: Checking {} stop tokens...", total_tokens_generated, stop_tokens.len());
            }

            for stop_token in &stop_tokens {
                if test_response.contains(stop_token) {
                    log_debug!("Stopping generation due to stop token detected: '{}' at position {}", stop_token, total_tokens_generated);

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

                // Skip partial matching for "</s>" as it matches too many HTML/XML tags
                if stop_token == "</s>" {
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
                if total_tokens_generated >= 145 && total_tokens_generated <= 155 {
                    log_debug!("Token #{}: Should stop = true, breaking loop", total_tokens_generated);
                }
                if partial_to_remove > 0 {
                    let new_len = response.len().saturating_sub(partial_to_remove);
                    response.truncate(new_len);
                }
                hit_stop_condition = true;
                break;
            }

            if total_tokens_generated >= 145 && total_tokens_generated <= 155 {
                log_debug!("Token #{}: No stop condition, adding token to response", total_tokens_generated);
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
                        log_debug!("Executing command: {}", command_text);

                        let output = execute_command(command_text);
                        log_debug!("Command output: {}", output);

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
            log_debug!("Breaking generation loop:");
            log_debug!("  hit_stop_condition: {}", hit_stop_condition);
            log_debug!("  total_tokens_generated: {}", total_tokens_generated);
            log_debug!("  max_total_tokens: {}", max_total_tokens);
            log_debug!("  Reached max? {}", total_tokens_generated >= max_total_tokens);
            break;
        }

        if !command_executed {
            log_debug!("Continuing generation: no stop condition hit");
        }
    }

    log_debug!("Exited generation loop. Final stats:");
    log_debug!("  Total tokens generated: {}", total_tokens_generated);
    log_debug!("  Response length: {} chars", response.len());

    // Finish assistant message
    {
        let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
        logger.finish_assistant_message();
    }

    Ok((response.trim().to_string(), token_pos, max_total_tokens))
}
