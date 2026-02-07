use llama_cpp_2::{llama_batch::LlamaBatch, model::AddBos};
use regex::Regex;
use tokio::sync::mpsc;

use super::super::command::execute_command;
use super::super::models::*;
use super::tool_tags::ToolTags;
use crate::log_info;

// Default command execution tokens (used when no model-specific tags are set)
pub const OUTPUT_OPEN: &str = "\n<||SYSTEM.OUTPUT>\n";
pub const OUTPUT_CLOSE: &str = "\n<SYSTEM.OUTPUT||>\n";

// Default SYSTEM.EXEC regex (always tried as fallback)
lazy_static::lazy_static! {
    pub static ref EXEC_PATTERN: Regex = Regex::new(
        r"SYSTEM\.EXEC>(.+?)<SYSTEM\.EXEC\|{1,2}>"
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
    let open = regex::escape(tags.exec_open);
    let close = regex::escape(tags.exec_close);

    // Build pattern: open_tag(.+?)close_tag
    let pattern = format!(r"{}(.+?){}", open, close);
    Regex::new(&pattern).ok()
}

/// Result of command execution
pub struct CommandExecutionResult {
    pub output_block: String,
    pub output_tokens: Vec<i32>,
}

/// Check if response contains an unprocessed command and execute it.
///
/// Tries the model-specific exec tags first, then falls back to the default
/// SYSTEM.EXEC pattern. This ensures commands work regardless of which tag
/// format the model uses.
///
/// Returns the output block and tokens to inject if a command was found and executed.
pub fn check_and_execute_command(
    response: &str,
    last_scan_pos: usize,
    conversation_id: &str,
    model: &llama_cpp_2::model::LlamaModel,
) -> Result<Option<CommandExecutionResult>, String> {
    check_and_execute_command_with_tags(
        response,
        last_scan_pos,
        conversation_id,
        model,
        &super::tool_tags::DEFAULT_TAGS,
    )
}

/// Check for and execute commands using model-specific tool tags.
pub fn check_and_execute_command_with_tags(
    response: &str,
    last_scan_pos: usize,
    conversation_id: &str,
    model: &llama_cpp_2::model::LlamaModel,
    tags: &ToolTags,
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

        match found {
            Some(cmd) => cmd,
            None => return Ok(None),
        }
    };

    log_info!(conversation_id, "🔧 Command detected: {}", command_text);

    // Execute the command
    let output = execute_command(&command_text);
    log_info!(
        conversation_id,
        "📤 Command output length: {} chars",
        output.len()
    );

    // Format output block using model-specific output tags
    let output_open = format!("\n{}\n", tags.output_open);
    let output_close = format!("\n{}\n", tags.output_close);
    let output_block = format!("{}{}{}", output_open, output.trim(), output_close);

    // Tokenize output for injection into context
    let output_tokens = model
        .str_to_token(&output_block, AddBos::Never)
        .map_err(|e| format!("Tokenization of command output failed: {}", e))?;

    Ok(Some(CommandExecutionResult {
        output_block,
        output_tokens: output_tokens.iter().map(|t| t.0).collect(),
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
                .map_err(|e| format!("Batch add failed for command output: {}", e))?;
        }

        context
            .decode(batch)
            .map_err(|e| format!("Decode failed for command output: {}", e))?;

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
