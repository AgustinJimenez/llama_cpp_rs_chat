use llama_cpp_2::model::AddBos;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;

use llama_chat_types::*;
use super::batch_exec;
use super::output_assembly::{self, AssemblyParams};
use super::CommandExecutionResult;

/// Execute all `<tool_call>` blocks found inside a `<parallel_calls>` fence.
///
/// All tool calls are executed concurrently regardless of read/write classification,
/// because the model explicitly declared them independent by wrapping in the fence.
///
/// `block_start` is the byte offset in `response` where `<parallel_calls>` starts.
#[allow(clippy::too_many_arguments)]
pub fn execute_parallel_block(
    response: &str,
    block_start: usize,
    conversation_id: &str,
    model: &llama_cpp_2::model::LlamaModel,
    tags: &crate::tool_tags::ToolTags,
    template_type: Option<&str>,
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
    let block_content = match response.get(block_start..) {
        Some(s) if !s.is_empty() => s,
        _ => return Ok(None),
    };

    // Parse all <tool_call> blocks within the fence
    let all_calls = llama_chat_tools::try_parse_all_from_raw(block_content);
    if all_calls.is_empty() {
        log_warn!(conversation_id, "parallel_block: no tool calls found in fence content");
        return Ok(None);
    }

    let tool_names: Vec<&str> = all_calls.iter().map(|(n, _)| n.as_str()).collect();
    log_info!(conversation_id, "⚡ Parallel block: {} tools: {:?}", all_calls.len(), tool_names);
    llama_chat_db::event_log::log_event(
        conversation_id,
        "parallel_batch",
        &format!("{} tools: {:?}", all_calls.len(), tool_names),
    );

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

    let (output, all_response_images, image_summary_prompt) = batch_exec::execute_batch_tools(
        &all_calls,
        conversation_id,
        token_sender,
        token_pos,
        context_size,
        cancel,
        use_htmd,
        browser_backend,
        mcp_manager,
        db.clone(),
        model,
        backend,
        chat_template_string,
        tags,
        true, // force_parallel: run all concurrently
    );

    log_info!(conversation_id, "📤 Parallel block output: {} chars", output.len());

    let tool_name_for_log = format!("parallel[{}]", tool_names.join(","));

    let ap = AssemblyParams {
        command_text: block_content,
        raw_output: &output,
        tool_name_for_log: &tool_name_for_log,
        conversation_id,
        output_open: &output_open,
        output_close: &output_close,
        fuzzy_warning: None,
        template_type,
        token_sender,
        token_pos,
        context_size,
        model,
        backend,
        chat_template_string,
    };

    let (display_text, model_text) = output_assembly::sanitize_and_summarize(&ap);

    if let Some(ref sender) = token_sender {
        let _ = sender.send(TokenData {
            token: output_close.clone(),
            tokens_used: token_pos,
            max_tokens: context_size as i32,
            status: None,
            ..Default::default()
        });
    }

    let assembled = output_assembly::assemble_output(&ap, &display_text, &model_text, None);

    let model_tokens = model
        .str_to_token(&assembled.model_block, AddBos::Never)
        .map_err(|e| format!("Tokenization of parallel block injection failed: {e}"))?;

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
