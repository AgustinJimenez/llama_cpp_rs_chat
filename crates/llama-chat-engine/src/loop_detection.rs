use llama_cpp_2::model::AddBos;

use super::command_executor::wrap_output_for_model;
use super::command_executor::CommandExecutionResult;
use super::tool_tags::ToolTags;
/// Maximum number of times the same command can be repeated before blocking.
pub(crate) const MAX_COMMAND_REPEATS: usize = 3;

/// Result of loop detection check.
pub(crate) enum LoopCheckResult {
    /// No loop detected, continue with execution.
    /// Contains an optional fuzzy warning string to prepend to model output.
    Continue(Option<String>),
    /// Loop detected, return this result immediately instead of executing.
    Blocked(CommandExecutionResult),
    /// Infinite loop: too many consecutive blocks. Generation must stop immediately.
    /// Contains the finish reason tag for the UI.
    ForceStop(CommandExecutionResult),
}

/// Check if the given command is being repeated (exact or fuzzy match).
///
/// Pushes the command to `recent_commands` for tracking.
/// Returns `LoopCheckResult::Blocked` with a pre-built `CommandExecutionResult`
/// if execution should be refused, or `LoopCheckResult::Continue` with an
/// optional fuzzy warning string otherwise.
/// Maximum consecutive blocked loops before force-stopping generation.
const MAX_CONSECUTIVE_BLOCKS: usize = 3;

pub(crate) fn check_loop(
    command_text: &str,
    recent_commands: &mut Vec<String>,
    consecutive_blocks: &mut usize,
    tags: &ToolTags,
    template_type: Option<&str>,
    model: &llama_cpp_2::model::LlamaModel,
    conversation_id: &str,
) -> Result<LoopCheckResult, String> {
    let normalized_cmd = command_text.trim().to_string();

    // Match tool names across all model formats:
    // JSON: "name": "wait"  |  Llama3 XML: <function=wait>  |  GLM: wait\n<arg_key>  |  Mistral: wait,{
    let cmd_lower = normalized_cmd.to_lowercase();
    let is_wait_or_poll = cmd_lower.contains("wait")
        || cmd_lower.contains("sleep")
        || cmd_lower.contains("check_background_process");

    // Browser read tools return different content after each navigation,
    // so they should not be counted as loops. Navigation resets context.
    let is_browser_read = cmd_lower.contains("browser_get_text")
        || cmd_lower.contains("browser_get_html")
        || cmd_lower.contains("browser_get_links")
        || cmd_lower.contains("browser_screenshot")
        || cmd_lower.contains("get_text")
        || cmd_lower.contains("get_html")
        || cmd_lower.contains("get_links");
    if is_browser_read {
        return Ok(LoopCheckResult::Continue(None));
    }

    let repeat_count = recent_commands.iter().filter(|c| *c == &normalized_cmd).count();

    // Fuzzy similarity: compare middle 100 chars (skips XML wrapper, catches actual arguments)
    let cmd_mid: String = normalized_cmd.chars().skip(50).take(100).collect();
    let similar_count = if !is_wait_or_poll && cmd_mid.len() >= 20 {
        recent_commands.iter().filter(|c| {
            let other_mid: String = c.chars().skip(50).take(100).collect();
            if other_mid.len() < 20 || cmd_mid.len() < 20 { return false; }
            let matches = cmd_mid.chars().zip(other_mid.chars()).filter(|(a, b)| a == b).count();
            let max_len = cmd_mid.len().max(other_mid.len());
            (matches * 100 / max_len) >= 80
        }).count()
    } else { 0 };

    recent_commands.push(normalized_cmd.clone()); // Always track, even on loop

    // Helper: check if we should escalate to ForceStop
    let maybe_force_stop = |result: CommandExecutionResult, blocks: &mut usize| -> LoopCheckResult {
        *blocks += 1;
        if *blocks >= MAX_CONSECUTIVE_BLOCKS {
            eprintln!("[LOOP] {} consecutive blocks — force-stopping generation", blocks);
            LoopCheckResult::ForceStop(result)
        } else {
            LoopCheckResult::Blocked(result)
        }
    };

    // Fuzzy loop: warn at 3+, block at 6+
    let fuzzy_warning = if !is_wait_or_poll && similar_count >= 3 && repeat_count < MAX_COMMAND_REPEATS {
        eprintln!("[FUZZY_LOOP] {} similar commands detected", similar_count);
        if similar_count >= 6 {
            // Escalate: block execution like exact match loop
            let output = format!(
                "LOOP BLOCKED: You have run {} very similar commands. Execution REFUSED. \
                 You MUST use a completely different tool or approach. Do NOT use the same tool with similar arguments.",
                similar_count
            );
            let output_open = format!("\n{}\n", tags.output_open);
            let output_close = format!("\n{}\n", tags.output_close);
            let output_block = format!("{}{}{}", output_open, output.trim(), output_close);
            let model_block = wrap_output_for_model(&output_block, template_type);
            let model_tokens = model
                .str_to_token(&model_block, AddBos::Never)
                .map_err(|e| format!("Tokenization of fuzzy loop block failed: {e}"))?;
            return Ok(maybe_force_stop(CommandExecutionResult {
                output_block,
                model_tokens: model_tokens.iter().map(|t| t.0).collect(),
                model_block,
                response_images: Vec::new(),
            }, consecutive_blocks));
        }
        Some(format!("WARNING: You have run {} very similar commands. You may be stuck in a loop. Try a completely different approach.", similar_count))
    } else {
        None
    };

    if !is_wait_or_poll && repeat_count >= MAX_COMMAND_REPEATS {
        log_info!(
            conversation_id,
            "🔁 Loop detected! Command repeated {} times: {}",
            repeat_count + 1,
            normalized_cmd
        );
        let output = if repeat_count >= MAX_COMMAND_REPEATS + 2 {
            // After 2 extra attempts beyond the warning, force the model to stop
            format!(
                "LOOP BLOCKED: This command has been repeated {} times. Execution REFUSED. \
                 You MUST use a completely different approach or ask the user for help.",
                repeat_count + 1
            )
        } else {
            format!(
                "LOOP DETECTED: You have already run this exact command {} times with the same result. \
                 STOP repeating it. Try a COMPLETELY DIFFERENT approach, or explain to the user what is blocking you.",
                repeat_count + 1
            )
        };
        let output_open = format!("\n{}\n", tags.output_open);
        let output_close = format!("\n{}\n", tags.output_close);
        let output_block = format!("{}{}{}", output_open, output.trim(), output_close);
        let model_block = wrap_output_for_model(&output_block, template_type);
        let model_tokens = model
            .str_to_token(&model_block, AddBos::Never)
            .map_err(|e| format!("Tokenization of loop detection block failed: {e}"))?;
        return Ok(maybe_force_stop(CommandExecutionResult {
            output_block,
            model_tokens: model_tokens.iter().map(|t| t.0).collect(),
            model_block,
            response_images: Vec::new(),
        }, consecutive_blocks));
    }

    // Successful execution — reset consecutive block counter
    *consecutive_blocks = 0;
    Ok(LoopCheckResult::Continue(fuzzy_warning))
}

/// Detect dead links / HTTP errors and return a hint for the model to search online.
pub(crate) fn detect_http_error_hint(model_text: &str) -> Option<&'static str> {
    let lower = model_text.to_lowercase();
    if lower.contains("404") || lower.contains("not found") || lower.contains("403") || lower.contains("forbidden")
        || lower.contains("connection refused") || lower.contains("could not resolve host")
        || lower.contains("error 1010") || lower.contains("error 1015") || lower.contains("ray id")
        || lower.contains("cloudflare") || lower.contains("enable cookies") || lower.contains("access denied")
        || (lower.contains("ssl") && lower.contains("error"))
    {
        Some("TIP: This URL appears to be dead or inaccessible. Use web_search to find the correct/current URL instead of guessing.")
    } else {
        None
    }
}
