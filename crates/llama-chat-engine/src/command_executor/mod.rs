use llama_cpp_2::model::AddBos;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;

use llama_chat_types::*;
use super::loop_detection::{self, LoopCheckResult};
use super::tool_parser::FORMAT_PRIORITY;
use super::tool_tags::ToolTags;

mod inject;
mod batch_exec;
mod single_exec;
mod output_assembly;

pub(crate) use super::tool_output::{
    run_summary_pass_public,
    run_summary_reusing_ctx,
    wrap_output_for_model,
};

pub use inject::inject_output_tokens;

/// Result of command execution
pub struct CommandExecutionResult {
    /// Display block for frontend/logging (just the output tags, no chat template wrapping)
    pub output_block: String,
    /// Tokens for model context injection (wrapped in chat template turn structure)
    pub model_tokens: Vec<i32>,
    /// The template-wrapped text used for model context injection.
    #[allow(dead_code)]
    pub model_block: String,
    /// Raw image bytes from tool responses (e.g., screenshots) for vision pipeline injection.
    #[allow(dead_code)]
    pub response_images: Vec<Vec<u8>>,
    /// If `Some`, run a vision summary pass with this prompt before injecting images.
    /// `None` means inject images directly into the vision context (default / summary=false).
    pub image_summary_prompt: Option<String>,
}

/// Check for and execute commands using model-specific tool tags.
#[allow(clippy::too_many_arguments)]
pub fn check_and_execute_command_with_tags(
    response: &str,
    last_scan_pos: usize,
    conversation_id: &str,
    model: &llama_cpp_2::model::LlamaModel,
    tags: &ToolTags,
    template_type: Option<&str>,
    recent_commands: &mut Vec<String>,
    consecutive_loop_blocks: &mut usize,
    token_sender: &Option<mpsc::UnboundedSender<TokenData>>,
    token_pos: i32,
    context_size: u32,
    cancel: Option<Arc<AtomicBool>>,
    use_htmd: bool,
    browser_backend: &crate::browser::BrowserBackend,
    mcp_manager: Option<Arc<dyn llama_chat_tools::McpManagerOps>>,
    db: llama_chat_db::SharedDatabase,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    chat_template_string: Option<&str>,
) -> Result<Option<CommandExecutionResult>, String> {
    // Only scan new content since last command execution
    let response_to_scan = if last_scan_pos < response.len() {
        let mut pos = last_scan_pos;
        while pos < response.len() && !response.is_char_boundary(pos) {
            pos += 1;
        }
        &response[pos..]
    } else {
        return Ok(None);
    };

    // Fast path: skip expensive regex checks unless we see a closing tag character.
    let has_gt = response_to_scan.contains('>');
    let has_bracket = response_to_scan.contains(']');
    let ends_brace = response_to_scan.ends_with('}');
    if !has_gt && !has_bracket && !ends_brace {
        return Ok(None);
    }

    // Try each format detector in priority order (first match wins)
    let command_text = {
        let mut found: Option<(&str, String)> = None;
        for &(name, detect) in FORMAT_PRIORITY {
            if let Some(cmd) = detect(response_to_scan, tags) {
                log_debug!(conversation_id, "tool_detect: format='{}' matched, cmd_len={}", name, cmd.len());
                found = Some((name, cmd));
                break;
            }
        }
        match found {
            Some((_name, cmd)) => cmd,
            None => {
                let slen = response_to_scan.len();
                if (slen <= 300 || slen % 500 == 0)
                    && (response_to_scan.contains("tool_call") || response_to_scan.contains("SYSTEM.EXEC"))
                {
                    log_debug!(
                        conversation_id,
                        "tool_detect: no format matched but tool-related text found. scan_len={}, tail={:?}",
                        slen,
                        {
                            let mut tail = slen.saturating_sub(200);
                            while tail > 0 && !response_to_scan.is_char_boundary(tail) { tail += 1; }
                            &response_to_scan[tail..]
                        }
                    );
                }
                return Ok(None);
            }
        }
    };

    log_info!(conversation_id, "🔧 Command detected: {}", command_text);

    // Extract tool name for logging
    let tool_name_for_log = extract_tool_name(&command_text);
    llama_chat_db::event_log::log_event(conversation_id, "tool_call", &format!("{} (cmd #{})", tool_name_for_log, recent_commands.len() + 1));

    // Loop detection
    match loop_detection::check_loop(&command_text, recent_commands, consecutive_loop_blocks, tags, template_type, model, conversation_id)? {
        LoopCheckResult::ForceStop(mut result) => {
            llama_chat_db::event_log::log_event(conversation_id, "infinite_loop", &format!("Force-stop after {} consecutive blocks", consecutive_loop_blocks));
            result.output_block.push_str("\n[INFINITE_LOOP_DETECTED]\n");
            return Ok(Some(result));
        }
        LoopCheckResult::Blocked(result) => {
            llama_chat_db::event_log::log_event(conversation_id, "loop_blocked", &format!("{} blocked (consecutive: {})", tool_name_for_log, consecutive_loop_blocks));
            return Ok(Some(result));
        }
        LoopCheckResult::Continue(fuzzy_warning) => {
            let all_calls = llama_chat_tools::try_parse_all_from_raw(&command_text);
            let is_batch = all_calls.len() > 1;

            if is_batch {
                log_info!(conversation_id, "📦 Batch tool call: {} tools detected", all_calls.len());
            }

            let output_open = format!("\n{}\n", tags.output_open);
            let output_close = format!("\n{}\n", tags.output_close);

            if let Some(ref sender) = token_sender {
                let _ = sender.send(TokenData {
                    token: output_open.clone(),
                    tokens_used: token_pos,
                    max_tokens: context_size as i32,
                    status: None,
                    ..Default::default()
                });
            }

            let (output, all_response_images, image_summary_prompt) = if is_batch {
                batch_exec::execute_batch_tools(
                    &all_calls,
                    conversation_id,
                    token_sender,
                    token_pos,
                    context_size,
                    cancel.clone(),
                    use_htmd,
                    browser_backend,
                    mcp_manager.clone(),
                    db.clone(),
                    model, backend, chat_template_string, tags,
                )
            } else {
                single_exec::execute_single_call(
                    &command_text,
                    &tool_name_for_log,
                    conversation_id,
                    token_sender,
                    token_pos,
                    context_size,
                    cancel.clone(),
                    use_htmd,
                    browser_backend,
                    mcp_manager.clone(),
                    db.clone(),
                    model, backend, chat_template_string, tags,
                    recent_commands,
                )
            };

            log_info!(conversation_id, "📤 Command output length: {} chars", output.len());

            let ap = output_assembly::AssemblyParams {
                command_text: &command_text,
                raw_output: &output,
                tool_name_for_log: &tool_name_for_log,
                conversation_id,
                output_open: &output_open,
                output_close: &output_close,
                fuzzy_warning: fuzzy_warning.as_deref(),
                template_type,
                token_sender,
                token_pos,
                context_size,
                model,
                backend,
                chat_template_string,
            };

            let (display_text, model_text) = output_assembly::sanitize_and_summarize(&ap);

            // Stream the output_close tag to frontend
            if let Some(ref sender) = token_sender {
                let _ = sender.send(TokenData {
                    token: output_close.clone(),
                    tokens_used: token_pos,
                    max_tokens: context_size as i32,
                    status: None,
                    ..Default::default()
                });
            }

            let http_error_hint = loop_detection::detect_http_error_hint(model_text.trim());
            let assembled = output_assembly::assemble_output(&ap, &display_text, &model_text, http_error_hint);

            log_info!(
                conversation_id,
                "🔄 Model injection block (template={:?}):\n{}",
                template_type,
                assembled.model_block
            );

            let model_tokens = model
                .str_to_token(&assembled.model_block, AddBos::Never)
                .map_err(|e| format!("Tokenization of model injection block failed: {e}"))?;

            if !all_response_images.is_empty() {
                eprintln!(
                    "[TOOL_RESULT] {} image(s) for vision pipeline, sizes: {:?}",
                    all_response_images.len(),
                    all_response_images.iter().map(|img| img.len()).collect::<Vec<_>>()
                );
            }

            let mut output_block = assembled.output_block;
            output_assembly::append_image_links(&mut output_block, &all_response_images, conversation_id);

            Ok(Some(CommandExecutionResult {
                output_block,
                model_tokens: model_tokens.iter().map(|t| t.0).collect(),
                model_block: assembled.model_block,
                response_images: all_response_images,
                image_summary_prompt,
            }))
        }
    }
}

fn extract_tool_name(command_text: &str) -> String {
    let lower = command_text.to_lowercase();
    if let Some(start) = lower.find("\"name\"") {
        let rest = &command_text[start..];
        if let Some(q1) = rest.find(':').and_then(|c| rest[c..].find('"').map(|q| c + q + 1)) {
            if let Some(q2) = rest[q1..].find('"') {
                return rest[q1..q1 + q2].to_string();
            }
        }
    } else if let Some(start) = lower.find("<function=") {
        let rest = &command_text[start + 10..];
        return rest.split('>').next().unwrap_or("unknown").to_string();
    }
    "unknown".to_string()
}
