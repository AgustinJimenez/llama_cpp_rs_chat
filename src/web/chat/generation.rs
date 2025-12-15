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
use super::super::model_manager::load_model;
use super::templates::apply_model_chat_template;
use super::command_executor::{check_and_execute_command, inject_output_tokens, stream_command_output};
use crate::{log_debug, log_info, log_warn};

// Constants for LLaMA configuration
const CONTEXT_SIZE: u32 = 32768;
const MODEL_PATH: &str = "/app/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf";

// Command tokens for detection (used for checking if inside exec block)
const EXEC_OPEN: &str = "<||SYSTEM.EXEC>";
const EXEC_CLOSE: &str = "<SYSTEM.EXEC||>";

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
    eprintln!("[GENERATION] generate_llama_response called, token_sender is {}",
        if token_sender.is_some() { "Some" } else { "None" });

    // Get conversation ID for logging
    let conversation_id = {
        let logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
        logger.get_conversation_id()
    };
    eprintln!("[GENERATION] Conversation ID: {}", conversation_id);

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

    // Since we're keeping KV cache in CPU RAM (not GPU), we don't need to reduce context size
    // Just use the requested context size directly
    let context_size = requested_context_size;

    log_info!(&conversation_id, "Using KV cache in CPU RAM - full context size available");
    log_info!(&conversation_id, "Using context size: {} (model max: {:?})",
        context_size, state.model_context_length);

    // Create sampler based on configuration
    let mut sampler = match config.sampler_type.as_str() {
        "Temperature" => {
            log_info!(&conversation_id, "Using Temperature sampler: temp={}", config.temperature);
            LlamaSampler::temp(config.temperature as f32)
        }
        "Mirostat" => {
            log_info!(&conversation_id, "Using Mirostat sampler: tau={}, eta={}", config.mirostat_tau, config.mirostat_eta);
            LlamaSampler::mirostat(
                0,    // n_vocab
                1234, // seed
                config.mirostat_tau as f32,
                config.mirostat_eta as f32,
                100,  // m
            )
        }
        "Greedy" | _ => {
            log_info!(&conversation_id, "Using Greedy sampler (default)");
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
    log_info!(&conversation_id, "=== TEMPLATE DEBUG ===");
    log_info!(&conversation_id, "Template type: {:?}", template_type);
    log_info!(&conversation_id, "Conversation content:\n{}", conversation_content);
    let prompt = apply_model_chat_template(&conversation_content, template_type.as_deref())?;
    log_info!(&conversation_id, "=== FINAL PROMPT BEING SENT TO MODEL ===");
    log_info!(&conversation_id, "{}", prompt);
    log_info!(&conversation_id, "=== END PROMPT (length: {} chars) ===", prompt.len());

    // Tokenize
    log_debug!(&conversation_id, "Step 2: Starting tokenization...");
    let tokens = model
        .str_to_token(&prompt, AddBos::Never)
        .map_err(|e| format!("Tokenization failed: {}", e))?;
    log_debug!(&conversation_id, "Step 2 complete: Tokenized to {} tokens", tokens.len());

    // Create context with configured size
    log_debug!(&conversation_id, "Step 3: Creating context (size={}K tokens)...", context_size / 1024);
    let n_ctx = NonZeroU32::new(context_size)
        .expect("Context size must be non-zero");

    // Keep KV cache in CPU RAM instead of GPU VRAM
    // This allows using the full context size even with limited VRAM
    // Trade-off: Slightly slower inference, but can use much larger contexts
    log_info!(&conversation_id, "💾 Keeping KV cache in CPU RAM to support larger context sizes");

    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(Some(n_ctx))
        .with_offload_kqv(false); // Keep KV cache in CPU RAM, not GPU

    let mut context = model
        .new_context(&state.backend, ctx_params)
        .map_err(|e| format!("Context creation failed: {}", e))?;
    log_debug!(&conversation_id, "Step 3 complete: Context created");

    // Prepare batch
    let batch_size = std::cmp::min(tokens.len() + 512, 2048);
    log_debug!(&conversation_id, "Step 4: Preparing batch (size={}, tokens={})...", batch_size, tokens.len());
    let mut batch = LlamaBatch::new(batch_size, 1);
    for (i, &token) in tokens.iter().enumerate() {
        let is_last = i == tokens.len() - 1;
        batch
            .add(token, i as i32, &[0], is_last)
            .map_err(|e| format!("Batch add failed: {}", e))?;
    }
    log_debug!(&conversation_id, "Step 4 complete: Batch prepared with {} tokens", tokens.len());

    // Process initial tokens
    log_debug!(&conversation_id, "Step 5: Starting initial decode...");
    context
        .decode(&mut batch)
        .map_err(|e| format!("Initial decode failed: {}", e))?;
    log_debug!(&conversation_id, "Step 5 complete: Initial decode successful");

    // Start assistant message in conversation log (enables streaming broadcast)
    {
        let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
        logger.start_assistant_message();
    }

    // Generate response
    let mut response = String::new();
    let mut token_pos = tokens.len() as i32;
    let mut total_tokens_generated = 0;

    // Calculate max tokens based on remaining context space
    let remaining_context = (context_size as i32) - token_pos - 128;
    let max_total_tokens = remaining_context.max(512);

    log_info!(&conversation_id, "Context size: {}, Prompt tokens: {}, Max tokens to generate: {}",
             context_size, token_pos, max_total_tokens);

    eprintln!("[GENERATION] About to start token generation loop. token_sender is {}",
        if token_sender.is_some() { "SOME" } else { "NONE" });

    // Track position in response after last executed command to prevent re-matching
    let mut last_exec_scan_pos: usize = 0;

    // Outer loop to handle command execution and continuation
    loop {
        let mut command_executed = false;
        let mut hit_stop_condition = false;

        // Inner loop for token generation
        let tokens_to_generate = std::cmp::min(2048, max_total_tokens - total_tokens_generated);

        log_debug!(&conversation_id, "Starting generation cycle: tokens_to_generate={}, total_tokens_generated={}",
                 tokens_to_generate, total_tokens_generated);

        for i in 0..tokens_to_generate {
            // Sample next token
            if i % 50 == 0 {
                log_debug!(&conversation_id, "Generated {} tokens so far...", total_tokens_generated);
            }

            // Extra logging around the 150 token mark
            if total_tokens_generated >= 145 && total_tokens_generated <= 155 {
                log_debug!(&conversation_id, "Token #{}: About to sample...", total_tokens_generated);
            }

            let next_token = sampler.sample(&context, -1);

            if total_tokens_generated >= 145 && total_tokens_generated <= 155 {
                log_debug!(&conversation_id, "Token #{}: Sampled token ID {}", total_tokens_generated, next_token);
            }

            // Check for end-of-sequence token
            if next_token == model.token_eos() {
                log_debug!(&conversation_id, "Stopping generation - EOS token detected (token ID: {}) at position {}", next_token, total_tokens_generated);
                hit_stop_condition = true;
                break;
            }

            // Add token to batch and decode
            if total_tokens_generated >= 145 && total_tokens_generated <= 155 {
                log_debug!(&conversation_id, "Token #{}: About to add to batch and decode...", total_tokens_generated);
            }

            batch.clear();
            batch
                .add(next_token, token_pos, &[0], true)
                .map_err(|e| format!("Batch add failed at token {}: {}", total_tokens_generated, e))?;

            context
                .decode(&mut batch)
                .map_err(|e| format!("Decode failed at token {}: {}", total_tokens_generated, e))?;

            if total_tokens_generated >= 145 && total_tokens_generated <= 155 {
                log_debug!(&conversation_id, "Token #{}: Decode successful", total_tokens_generated);
            }

            token_pos += 1;
            total_tokens_generated += 1;

            // Convert to string for display
            let token_str = match model.token_to_str(next_token, Special::Tokenize) {
                Ok(s) => s,
                Err(e) => {
                    log_warn!(&conversation_id, "Token {} can't be displayed as UTF-8: {}. Continuing generation.", next_token, e);
                    continue;
                }
            };

            if total_tokens_generated >= 145 && total_tokens_generated <= 155 {
                log_debug!(&conversation_id, "Token #{}: Converted to string: {:?}", total_tokens_generated, token_str);
            }

            // Check for stop sequences
            let test_response = format!("{}{}", response, token_str);
            let mut should_stop = false;
            let mut partial_to_remove = 0;
            // Check if we're inside a SYSTEM.EXEC block (don't stop until we get the closing tag)
            // Use flexible detection: look for SYSTEM.EXEC> opening without closing tag
            let has_exec_open = response.contains("SYSTEM.EXEC>") || response.contains(EXEC_OPEN);
            let has_exec_close = response.contains(EXEC_CLOSE) || response.contains("<SYSTEM.EXEC|");
            let in_exec_block = has_exec_open && !has_exec_close;

            if total_tokens_generated >= 145 && total_tokens_generated <= 155 {
                log_debug!(&conversation_id, "Token #{}: Checking {} stop tokens...", total_tokens_generated, stop_tokens.len());
            }

            for stop_token in &stop_tokens {
                // Don't stop if we're inside an exec block - let it complete
                if in_exec_block {
                    continue;
                }

                if test_response.contains(stop_token) {
                    log_debug!(&conversation_id, "Stopping generation due to stop token detected: '{}' at position {}", stop_token, total_tokens_generated);

                    // OLD: Special case for </COMMAND>
                    // if stop_token == "</COMMAND>" {
                    //     response.push_str(&token_str);
                    //     let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
                    //     logger.log_token(&token_str);
                    // }

                    should_stop = true;
                    break;
                }

                // Handle partial matches - skip if inside exec block
                // OLD: if in_command_block && (stop_token.starts_with("</") || stop_token.starts_with("[/")) {
                //     continue;
                // }

                // OLD: if stop_token == "</COMMAND>" {
                //     continue;
                // }

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
                    log_debug!(&conversation_id, "Token #{}: Should stop = true, breaking loop", total_tokens_generated);
                }
                if partial_to_remove > 0 {
                    let new_len = response.len().saturating_sub(partial_to_remove);
                    response.truncate(new_len);
                }
                hit_stop_condition = true;
                break;
            }

            if total_tokens_generated >= 145 && total_tokens_generated <= 155 {
                log_debug!(&conversation_id, "Token #{}: No stop condition, adding token to response", total_tokens_generated);
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
                match sender.send(token_data) {
                    Ok(()) => {
                        if total_tokens_generated <= 5 || total_tokens_generated % 50 == 0 {
                            eprintln!("[GENERATION] Token #{} sent via channel: {:?}", total_tokens_generated, token_str.chars().take(20).collect::<String>());
                        }
                    }
                    Err(e) => {
                        eprintln!("[GENERATION] ERROR: Failed to send token #{} via channel: {}", total_tokens_generated, e);
                    }
                }
            } else {
                if total_tokens_generated == 1 {
                    eprintln!("[GENERATION] WARNING: token_sender is None, tokens not being streamed!");
                }
            }

            // Log token
            {
                let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
                logger.log_token(&token_str);
            }
        }

        // Check for and execute any SYSTEM.EXEC commands in the response
        if let Some(exec_result) = check_and_execute_command(
            &response,
            last_exec_scan_pos,
            &conversation_id,
            model,
        )? {
            // 1. Log to conversation file (CRITICAL: prevents infinite loops)
            {
                let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
                logger.log_token(&exec_result.output_block);
            }

            // 2. Add to response string
            response.push_str(&exec_result.output_block);

            // 3. Stream to frontend
            stream_command_output(&exec_result.output_block, &token_sender, token_pos, context_size);

            // 4. Inject output tokens into LLM context
            inject_output_tokens(
                &exec_result.output_tokens,
                &mut batch,
                &mut context,
                &mut token_pos,
                &conversation_id,
            )?;

            command_executed = true;
            // CRITICAL: Reset stop condition so generation continues after command output
            hit_stop_condition = false;
            // Update scan position to end of response (past the injected output)
            last_exec_scan_pos = response.len();
            log_info!(&conversation_id, "✅ Command executed, output injected, scan position updated to {}", last_exec_scan_pos);
        }

        // Break conditions
        if hit_stop_condition || total_tokens_generated >= max_total_tokens {
            log_debug!(&conversation_id, "Breaking generation loop:");
            log_debug!(&conversation_id, "  hit_stop_condition: {}", hit_stop_condition);
            log_debug!(&conversation_id, "  total_tokens_generated: {}", total_tokens_generated);
            log_debug!(&conversation_id, "  max_total_tokens: {}", max_total_tokens);
            log_debug!(&conversation_id, "  Reached max? {}", total_tokens_generated >= max_total_tokens);
            break;
        }

        if !command_executed {
            log_debug!(&conversation_id, "Continuing generation: no stop condition hit");
        }
    }

    log_debug!(&conversation_id, "Exited generation loop. Final stats:");
    log_debug!(&conversation_id, "  Total tokens generated: {}", total_tokens_generated);
    log_debug!(&conversation_id, "  Response length: {} chars", response.len());

    // Finish assistant message
    {
        let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
        logger.finish_assistant_message();
    }

    Ok((response.trim().to_string(), token_pos, max_total_tokens))
}
