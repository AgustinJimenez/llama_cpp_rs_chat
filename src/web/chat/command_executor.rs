use regex::Regex;
use tokio::sync::mpsc;
use llama_cpp_2::{
    llama_batch::LlamaBatch,
    model::AddBos,
};

use super::super::models::*;
use super::super::command::execute_command;
use crate::log_info;

// Command execution tokens
pub const OUTPUT_OPEN: &str = "\n<||SYSTEM.OUTPUT>\n";
pub const OUTPUT_CLOSE: &str = "\n<SYSTEM.OUTPUT||>\n";

// Flexible regex pattern for command detection
lazy_static::lazy_static! {
    pub static ref EXEC_PATTERN: Regex = Regex::new(
        r"SYSTEM\.EXEC>(.+?)<SYSTEM\.EXEC\|{1,2}>"
    ).unwrap();
}

/// Result of command execution
pub struct CommandExecutionResult {
    pub output_block: String,
    pub output_tokens: Vec<i32>,
}

/// Check if response contains an unprocessed command and execute it.
///
/// Returns the output block and tokens to inject if a command was found and executed.
pub fn check_and_execute_command(
    response: &str,
    last_scan_pos: usize,
    conversation_id: &str,
    model: &llama_cpp_2::model::LlamaModel,
) -> Result<Option<CommandExecutionResult>, String> {
    // Only scan new content since last command execution
    let response_to_scan = if last_scan_pos < response.len() {
        &response[last_scan_pos..]
    } else {
        return Ok(None);
    };

    // Check for command pattern
    let captures = match EXEC_PATTERN.captures(response_to_scan) {
        Some(c) => c,
        None => return Ok(None),
    };

    // Extract command from capture group
    let command_match = match captures.get(1) {
        Some(m) => m,
        None => return Ok(None),
    };

    let command_text = command_match.as_str();
    log_info!(conversation_id, "🔧 SYSTEM.EXEC detected: {}", command_text);

    // Execute the command
    let output = execute_command(command_text);
    log_info!(conversation_id, "📤 Command output length: {} chars", output.len());

    // Format output block
    let output_block = format!("{}{}{}", OUTPUT_OPEN, output.trim(), OUTPUT_CLOSE);

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
    log_debug!(conversation_id, "Injecting {} output tokens into context", tokens.len());

    for &token in tokens {
        batch.clear();
        batch
            .add(llama_cpp_2::token::LlamaToken(token), *token_pos, &[0], true)
            .map_err(|e| format!("Batch add failed for command output: {}", e))?;

        context
            .decode(batch)
            .map_err(|e| format!("Decode failed for command output: {}", e))?;

        *token_pos += 1;
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
