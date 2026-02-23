use llama_cpp_2::{llama_batch::LlamaBatch, model::AddBos};
use regex::Regex;
use tokio::sync::mpsc;

use super::super::command::execute_command;
use super::super::models::*;
use super::super::native_tools;
use super::tool_tags::ToolTags;
use crate::log_info;

// Default SYSTEM.EXEC regex (always tried as fallback)
// (?s) enables DOTALL mode so . matches newlines (multi-line commands)
lazy_static::lazy_static! {
    pub static ref EXEC_PATTERN: Regex = Regex::new(
        r"(?s)SYSTEM\.EXEC>(.+?)<SYSTEM\.EXEC\|{1,2}>"
    ).unwrap();

    // Llama3/Hermes XML format: <function=tool_name> ... </function>
    // Some models (Qwen3-Coder) output this without a <tool_call> wrapper.
    static ref LLAMA3_FUNC_PATTERN: Regex = Regex::new(
        r"(?s)(<function=[a-z_]+>.*?</function>)"
    ).unwrap();

    // Harmony format (gpt-oss-20b): to= tool_name ... code<|message|>{...}<|call|>
    // Note: model may emit space after "to=" (e.g. "to= list_directory")
    static ref HARMONY_CALL_PATTERN: Regex = Regex::new(
        r"(?s)to=\s*(\w+)[\s\S]*?code<\|message\|>(.*?)<\|call\|>"
    ).unwrap();
}

/// Build a regex that matches the model-specific exec tags.
/// Returns None if the tags are already covered by the default EXEC_PATTERN.
fn build_model_exec_regex(tags: &ToolTags) -> Option<Regex> {
    // Skip if using default SYSTEM.EXEC tags (already handled by EXEC_PATTERN)
    if tags.exec_open.contains("SYSTEM.EXEC") {
        return None;
    }

    // Escape special regex characters in the tags
    let open = regex::escape(&tags.exec_open);
    let close = regex::escape(&tags.exec_close);

    // Build pattern: open_tag(.+?)close_tag
    // (?s) enables DOTALL mode so . matches newlines (multi-line commands like python -c)
    let pattern = format!(r"(?s){open}(.+?){close}");
    Regex::new(&pattern).ok()
}

/// Result of command execution
pub struct CommandExecutionResult {
    /// Display block for frontend/logging (just the output tags, no chat template wrapping)
    pub output_block: String,
    /// Tokens for model context injection (wrapped in chat template turn structure)
    pub model_tokens: Vec<i32>,
}

/// Check for and execute commands using model-specific tool tags.
pub fn check_and_execute_command_with_tags(
    response: &str,
    last_scan_pos: usize,
    conversation_id: &str,
    model: &llama_cpp_2::model::LlamaModel,
    tags: &ToolTags,
    template_type: Option<&str>,
    web_search_provider: Option<&str>,
) -> Result<Option<CommandExecutionResult>, String> {
    // Only scan new content since last command execution
    let response_to_scan = if last_scan_pos < response.len() {
        &response[last_scan_pos..]
    } else {
        return Ok(None);
    };

    // Try model-specific regex first, then default EXEC_PATTERN
    let command_text = {
        let model_regex = build_model_exec_regex(tags);
        let mut found: Option<String> = None;

        // Try model-specific pattern first
        if let Some(ref re) = model_regex {
            if let Some(captures) = re.captures(response_to_scan) {
                if let Some(m) = captures.get(1) {
                    found = Some(m.as_str().to_string());
                }
            }
        }

        // Fall back to default SYSTEM.EXEC pattern
        if found.is_none() {
            if let Some(captures) = EXEC_PATTERN.captures(response_to_scan) {
                if let Some(m) = captures.get(1) {
                    found = Some(m.as_str().to_string());
                }
            }
        }

        // Fall back to Llama3/Hermes <function=...> pattern (no wrapping <tool_call> tag)
        if found.is_none() {
            if let Some(captures) = LLAMA3_FUNC_PATTERN.captures(response_to_scan) {
                if let Some(m) = captures.get(1) {
                    found = Some(m.as_str().to_string());
                }
            }
        }

        // Fall back to Harmony format: to=tool_name ... code<|message|>{...}<|call|>
        if found.is_none() {
            if let Some(captures) = HARMONY_CALL_PATTERN.captures(response_to_scan) {
                if let (Some(tool_name), Some(args_json)) = (captures.get(1), captures.get(2)) {
                    // Reconstruct as standard JSON so dispatch_native_tool can parse it
                    found = Some(format!(
                        r#"{{"name":"{}","arguments":{}}}"#,
                        tool_name.as_str(),
                        args_json.as_str().trim()
                    ));
                }
            }
        }

        match found {
            Some(cmd) => cmd,
            None => return Ok(None),
        }
    };

    log_info!(conversation_id, "🔧 Command detected: {}", command_text);

    // Try native tool dispatch (JSON format) first, fall back to shell execution
    let output = if let Some(native_output) = native_tools::dispatch_native_tool(&command_text, web_search_provider) {
        log_info!(conversation_id, "📦 Dispatched to native tool handler");
        native_output
    } else {
        let trimmed_cmd = command_text.trim();
        if trimmed_cmd.starts_with('{') || trimmed_cmd.starts_with('[') {
            // Looks like a JSON tool call that failed to parse — don't execute as shell.
            // This prevents `sh: {name:: command not found` errors.
            log_info!(conversation_id, "⚠️ JSON-like tool call failed to parse, returning error to model");
            "Error: Failed to parse tool call JSON. The JSON may be malformed (check for unescaped backslashes, missing braces, or literal newlines in strings). Please try the execute_command tool to write files instead.".to_string()
        } else {
            log_info!(conversation_id, "🐚 Falling back to shell execution");
            execute_command(&command_text)
        }
    };
    log_info!(
        conversation_id,
        "📤 Command output length: {} chars",
        output.len()
    );

    // Format output block for frontend/logging (just output tags, no template wrapping)
    let output_open = format!("\n{}\n", tags.output_open);
    let output_close = format!("\n{}\n", tags.output_close);
    let output_block = format!("{}{}{}", output_open, output.trim(), output_close);

    // Build model injection block with chat template turn wrapping.
    // The model needs proper turn structure to know the tool response is from
    // a different role and that it should continue as assistant.
    let model_block = wrap_output_for_model(&output_block, template_type);
    log_info!(
        conversation_id,
        "🔄 Model injection block (template={:?}):\n{}",
        template_type,
        model_block
    );

    let model_tokens = model
        .str_to_token(&model_block, AddBos::Never)
        .map_err(|e| format!("Tokenization of model injection block failed: {e}"))?;

    Ok(Some(CommandExecutionResult {
        output_block,
        model_tokens: model_tokens.iter().map(|t| t.0).collect(),
    }))
}

/// Inject command output tokens into the LLM context.
pub fn inject_output_tokens(
    tokens: &[i32],
    batch: &mut LlamaBatch,
    context: &mut llama_cpp_2::context::LlamaContext,
    token_pos: &mut i32,
    conversation_id: &str,
) -> Result<(), String> {
    use crate::log_debug;
    log_debug!(
        conversation_id,
        "Injecting {} output tokens into context",
        tokens.len()
    );

    // Decode in chunks for performance (single-token decode is extremely slow for large outputs)
    const INJECT_CHUNK_SIZE: usize = 512;
    for chunk in tokens.chunks(INJECT_CHUNK_SIZE) {
        batch.clear();

        for (idx, &token) in chunk.iter().enumerate() {
            let is_last = idx == chunk.len() - 1;
            batch
                .add(
                    llama_cpp_2::token::LlamaToken(token),
                    *token_pos + (idx as i32),
                    &[0],
                    is_last,
                )
                .map_err(|e| format!("Batch add failed for command output: {e}"))?;
        }

        context
            .decode(batch)
            .map_err(|e| format!("Decode failed for command output: {e}"))?;

        *token_pos += chunk.len() as i32;
    }

    Ok(())
}

/// Stream command output to frontend via token sender.
pub fn stream_command_output(
    output_block: &str,
    token_sender: &Option<mpsc::UnboundedSender<TokenData>>,
    token_pos: i32,
    context_size: u32,
) {
    if let Some(ref sender) = token_sender {
        let _ = sender.send(TokenData {
            token: output_block.to_string(),
            tokens_used: token_pos,
            max_tokens: context_size as i32,
        });
    }
}

/// Wrap tool output in the model's chat template turn structure.
///
/// After the model generates a tool call, we need to:
/// 1. Close the assistant's turn
/// 2. Present the tool response as a separate turn (role varies by template)
/// 3. Re-open an assistant turn so the model continues naturally
///
/// Without this wrapping, the model sees raw tool output injected mid-turn
/// and gets confused (e.g., Qwen loops on `<|im_start|>` tokens).
///
/// The `output_block` already contains the output tags (e.g. `<tool_response>...</tool_response>`).
/// This function adds the surrounding chat template structure for model injection only.
/// The frontend continues to see the unwrapped `output_block`.
fn wrap_output_for_model(output_block: &str, template_type: Option<&str>) -> String {
    match template_type {
        Some("ChatML") => {
            // Qwen/ChatML: <|im_end|>\n<|im_start|>user\n...output...<|im_end|>\n<|im_start|>assistant\n
            format!(
                "<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
                output_block
            )
        }
        Some("Llama3") => {
            // Llama 3: <|eot_id|><|start_header_id|>tool<|end_header_id|>\n\n...output...<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n
            format!(
                "<|eot_id|><|start_header_id|>tool<|end_header_id|>\n\n{}<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n",
                output_block
            )
        }
        Some("Gemma") => {
            // Gemma: <end_of_turn>\n<start_of_turn>user\n...output...<end_of_turn>\n<start_of_turn>model\n
            format!(
                "<end_of_turn>\n<start_of_turn>user\n{}<end_of_turn>\n<start_of_turn>model\n",
                output_block
            )
        }
        Some("Harmony") => {
            // Harmony (gpt-oss-20b): Close assistant turn, inject tool result, re-open assistant analysis turn.
            // Using "analysis" channel (not "final") so the model continues reasoning and can make
            // more tool calls. If we re-open with "final", the model writes a user-facing summary
            // immediately instead of executing further steps.
            // output_block already contains <|start|>tool<|message|>...result...<|end|>
            format!(
                "<|end|>\n{}\n<|start|>assistant<|channel|>analysis<|message|>",
                output_block
            )
        }
        Some("GLM") => {
            // GLM-4 family: output_block contains <|observation|>\n{result}\n
            // Re-open assistant turn so model continues generating
            format!("{}\n<|assistant|>\n", output_block)
        }
        Some("Mistral") | _ => {
            // Mistral and default: output tags are sufficient, no extra turn wrapping needed.
            // Mistral's tool format is inline within the conversation flow.
            output_block.to_string()
        }
    }
}
