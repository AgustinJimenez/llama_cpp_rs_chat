use llama_cpp_2::{llama_batch::LlamaBatch, model::AddBos};
use llama_cpp_2::sampling::LlamaSampler;
use regex::Regex;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::Arc;
use tokio::sync::mpsc;

use super::super::background::execute_command_background;
use super::super::command::{execute_command_streaming, execute_command_streaming_with_timeout, sanitize_command_output, strip_ansi_codes};
use super::super::models::*;
use super::super::native_tools;
use super::generation::create_fresh_context;
use super::tool_tags::ToolTags;
use crate::{log_info, log_debug, log_warn};

// --- Tool output summarization via LLM sub-agent ---

/// Minimum output size (chars) to trigger LLM summarization.
/// Set to 0 to always summarize (useful for testing).
const SUMMARIZE_THRESHOLD: usize = 1500;
/// Context size for each summarization pass (tokens).
const SUMMARY_CTX_SIZE: u32 = 4096;
/// Maximum tokens to generate per summary.
const SUMMARY_MAX_TOKENS: usize = 256;
/// Maximum chars per chunk for map-reduce summarization.
const SUMMARY_CHUNK_CHARS: usize = 5000;

/// Run a single summarization pass: create temp context, eval prompt + text, generate summary.
fn run_summary_pass(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    text: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
) -> Result<String, String> {
    let system_msg = "Summarize this tool output concisely. Keep: file names, errors, key values, success/failure status. Remove: verbose logs, repeated patterns, boilerplate.";

    // Format prompt using chat template if available
    let formatted_prompt = if let Some(template_str) = chat_template_string {
        use super::jinja_templates::{apply_native_chat_template, ChatMessage};
        #[allow(deprecated)]
        use llama_cpp_2::model::Special;
        #[allow(deprecated)]
        let bos = model.token_to_str(model.token_bos(), Special::Tokenize).unwrap_or_else(|_| "<s>".into());
        #[allow(deprecated)]
        let eos = model.token_to_str(model.token_eos(), Special::Tokenize).unwrap_or_else(|_| "</s>".into());
        let messages = vec![
            ChatMessage { role: "system".into(), content: system_msg.into(), tool_calls: None },
            ChatMessage { role: "user".into(), content: text.into(), tool_calls: None },
        ];
        apply_native_chat_template(template_str, messages, None, None, true, &bos, &eos)
            .unwrap_or_else(|_| format!("SYSTEM:\n{system_msg}\n\nUSER:\n{text}\n\nASSISTANT:\n"))
    } else {
        format!("SYSTEM:\n{system_msg}\n\nUSER:\n{text}\n\nASSISTANT:\n")
    };

    let tokens = model
        .str_to_token(&formatted_prompt, AddBos::Never)
        .map_err(|e| format!("Summary tokenization failed: {e}"))?;

    // Ensure prompt fits in context (leave room for generation)
    if tokens.len() + SUMMARY_MAX_TOKENS > SUMMARY_CTX_SIZE as usize {
        return Err(format!("Summary prompt too large: {} tokens", tokens.len()));
    }

    let n_ctx = NonZeroU32::new(SUMMARY_CTX_SIZE).unwrap();
    let config = SamplerConfig::default();
    // offload_kqv=false: keep summarization context on CPU to avoid competing for VRAM
    // with the main context's KV cache (which may use nearly all GPU memory).
    let mut ctx = create_fresh_context(model, backend, n_ctx, false, &config)?;

    // Eval prompt in batches
    let batch_cap = 512usize;
    let mut batch = LlamaBatch::new(batch_cap, 1);
    let n_chunks = tokens.len().div_ceil(batch_cap);
    for chunk_idx in 0..n_chunks {
        let start = chunk_idx * batch_cap;
        let end = std::cmp::min(start + batch_cap, tokens.len());
        batch.clear();
        for (offset, &token) in tokens[start..end].iter().enumerate() {
            let pos = (start + offset) as i32;
            let is_last = start + offset == tokens.len() - 1;
            batch.add(token, pos, &[0], is_last)
                .map_err(|e| format!("Summary batch add failed: {e}"))?;
        }
        ctx.decode(&mut batch)
            .map_err(|e| format!("Summary prompt decode failed: {e}"))?;
    }

    // Generate summary (low temperature for deterministic output)
    let mut sampler = LlamaSampler::chain_simple(vec![
        LlamaSampler::temp(0.3),
        LlamaSampler::dist(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u32)
                .unwrap_or(42),
        ),
    ]);

    let mut summary = String::new();
    let mut token_pos = tokens.len() as i32;
    let eos_token = model.token_eos();

    for _ in 0..SUMMARY_MAX_TOKENS {
        let next_token = sampler.sample(&ctx, -1);
        if next_token == eos_token { break; }

        #[allow(deprecated)]
        let token_str = model
            .token_to_str(next_token, llama_cpp_2::model::Special::Tokenize)
            .unwrap_or_default();

        summary.push_str(&token_str);

        batch.clear();
        batch.add(next_token, token_pos, &[0], true)
            .map_err(|e| format!("Summary gen batch add failed: {e}"))?;
        ctx.decode(&mut batch)
            .map_err(|e| format!("Summary gen decode failed: {e}"))?;
        token_pos += 1;
    }

    drop(ctx);
    let result = summary.trim().to_string();
    log_info!(conversation_id, "📝 Summary pass: {} input chars → {} output chars", text.len(), result.len());
    Ok(result)
}

/// Public entry point for conversation compaction summarization.
/// Uses a conversation-appropriate system prompt instead of tool output prompt.
pub fn run_summary_pass_public(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    text: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
) -> Result<String, String> {
    // Override the system message for conversation summarization
    run_summary_pass_with_system(
        model, backend, text, chat_template_string, conversation_id,
        "Summarize this AI assistant conversation concisely. The ASSISTANT (not the user) is the one executing tools, writing code, and running commands. The USER only sends requests. Keep: what the user asked for, what the assistant did, key results, file paths, errors encountered. Remove: verbose tool output, repeated attempts, boilerplate. Write as a brief narrative paragraph.",
    )
}

/// Run a summary pass with a custom system message.
fn run_summary_pass_with_system(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    text: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
    system_msg: &str,
) -> Result<String, String> {
    let formatted_prompt = if let Some(template_str) = chat_template_string {
        use super::jinja_templates::apply_native_chat_template;
        #[allow(deprecated)]
        use llama_cpp_2::model::Special;
        #[allow(deprecated)]
        let bos = model.token_to_str(model.token_bos(), Special::Tokenize).unwrap_or_else(|_| "<s>".into());
        #[allow(deprecated)]
        let eos = model.token_to_str(model.token_eos(), Special::Tokenize).unwrap_or_else(|_| "</s>".into());
        let messages = vec![
            super::jinja_templates::ChatMessage { role: "system".into(), content: system_msg.into(), tool_calls: None },
            super::jinja_templates::ChatMessage { role: "user".into(), content: text.into(), tool_calls: None },
        ];
        apply_native_chat_template(template_str, messages, None, None, true, &bos, &eos)
            .unwrap_or_else(|_| format!("SYSTEM:\n{system_msg}\n\nUSER:\n{text}\n\nASSISTANT:\n"))
    } else {
        format!("SYSTEM:\n{system_msg}\n\nUSER:\n{text}\n\nASSISTANT:\n")
    };

    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::sampling::LlamaSampler;
    use llama_cpp_2::model::AddBos;
    use std::num::NonZeroU32;
    use crate::web::chat::generation::create_fresh_context;
    use crate::web::models::SamplerConfig;

    let tokens = model
        .str_to_token(&formatted_prompt, AddBos::Never)
        .map_err(|e| format!("Summary tokenization failed: {e}"))?;

    if tokens.len() + SUMMARY_MAX_TOKENS > SUMMARY_CTX_SIZE as usize {
        return Err(format!("Summary prompt too large: {} tokens", tokens.len()));
    }

    let n_ctx = NonZeroU32::new(SUMMARY_CTX_SIZE).unwrap();
    let config = SamplerConfig::default();
    let mut ctx = create_fresh_context(model, backend, n_ctx, false, &config)?;

    let batch_cap = 512usize;
    let mut batch = LlamaBatch::new(batch_cap, 1);
    let n_chunks = tokens.len().div_ceil(batch_cap);
    for chunk_idx in 0..n_chunks {
        let start = chunk_idx * batch_cap;
        let end = std::cmp::min(start + batch_cap, tokens.len());
        batch.clear();
        for (offset, &token) in tokens[start..end].iter().enumerate() {
            let pos = (start + offset) as i32;
            let is_last = start + offset == tokens.len() - 1;
            batch.add(token, pos, &[0], is_last)
                .map_err(|e| format!("Summary batch add failed: {e}"))?;
        }
        ctx.decode(&mut batch)
            .map_err(|e| format!("Summary prompt decode failed: {e}"))?;
    }

    let mut sampler = LlamaSampler::chain_simple(vec![
        LlamaSampler::temp(0.3),
        LlamaSampler::dist(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u32)
                .unwrap_or(42),
        ),
    ]);

    let mut summary = String::new();
    let mut token_pos = tokens.len() as i32;
    let eos_token = model.token_eos();

    for _ in 0..SUMMARY_MAX_TOKENS {
        let next_token = sampler.sample(&ctx, -1);
        if next_token == eos_token { break; }

        #[allow(deprecated)]
        let token_str = model
            .token_to_str(next_token, llama_cpp_2::model::Special::Tokenize)
            .unwrap_or_default();

        summary.push_str(&token_str);

        batch.clear();
        batch.add(next_token, token_pos, &[0], true)
            .map_err(|e| format!("Summary gen batch add failed: {e}"))?;
        ctx.decode(&mut batch)
            .map_err(|e| format!("Summary gen decode failed: {e}"))?;
        token_pos += 1;
    }

    drop(ctx);
    let result = summary.trim().to_string();
    log_info!(conversation_id, "📦 Conversation summary: {} input chars → {} output chars", text.len(), result.len());
    Ok(result)
}

/// Run a summary pass reusing an existing context (clears memory between uses).
/// This avoids CUDA memory fragmentation from creating/destroying many contexts.
pub fn run_summary_reusing_ctx(
    model: &llama_cpp_2::model::LlamaModel,
    ctx: &mut llama_cpp_2::context::LlamaContext<'_>,
    text: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
) -> Result<String, String> {
    let system_msg = "Summarize this AI assistant conversation concisely. The ASSISTANT (not the user) is the one executing tools, writing code, and running commands. The USER only sends requests. Keep: what the user asked for, what the assistant did, key results, file paths, errors encountered. Remove: verbose tool output, repeated attempts, boilerplate. Write as a brief narrative paragraph.";

    let formatted_prompt = if let Some(template_str) = chat_template_string {
        use super::jinja_templates::apply_native_chat_template;
        #[allow(deprecated)]
        use llama_cpp_2::model::Special;
        #[allow(deprecated)]
        let bos = model.token_to_str(model.token_bos(), Special::Tokenize).unwrap_or_else(|_| "<s>".into());
        #[allow(deprecated)]
        let eos = model.token_to_str(model.token_eos(), Special::Tokenize).unwrap_or_else(|_| "</s>".into());
        let messages = vec![
            super::jinja_templates::ChatMessage { role: "system".into(), content: system_msg.into(), tool_calls: None },
            super::jinja_templates::ChatMessage { role: "user".into(), content: text.into(), tool_calls: None },
        ];
        apply_native_chat_template(template_str, messages, None, None, true, &bos, &eos)
            .unwrap_or_else(|_| format!("SYSTEM:\n{system_msg}\n\nUSER:\n{text}\n\nASSISTANT:\n"))
    } else {
        format!("SYSTEM:\n{system_msg}\n\nUSER:\n{text}\n\nASSISTANT:\n")
    };

    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::sampling::LlamaSampler;
    use llama_cpp_2::model::AddBos;

    let tokens = model
        .str_to_token(&formatted_prompt, AddBos::Never)
        .map_err(|e| format!("Summary tokenization failed: {e}"))?;

    if tokens.len() + SUMMARY_MAX_TOKENS > SUMMARY_CTX_SIZE as usize {
        return Err(format!("Summary prompt too large: {} tokens", tokens.len()));
    }

    // Clear memory to reuse context for a fresh prompt
    ctx.clear_memory();

    let batch_cap = 512usize;
    let mut batch = LlamaBatch::new(batch_cap, 1);
    let n_chunks = tokens.len().div_ceil(batch_cap);
    for chunk_idx in 0..n_chunks {
        let start = chunk_idx * batch_cap;
        let end = std::cmp::min(start + batch_cap, tokens.len());
        batch.clear();
        for (offset, &token) in tokens[start..end].iter().enumerate() {
            let pos = (start + offset) as i32;
            let is_last = start + offset == tokens.len() - 1;
            batch.add(token, pos, &[0], is_last)
                .map_err(|e| format!("Summary batch add failed: {e}"))?;
        }
        ctx.decode(&mut batch)
            .map_err(|e| format!("Summary prompt decode failed: {e}"))?;
    }

    let mut sampler = LlamaSampler::chain_simple(vec![
        LlamaSampler::temp(0.3),
        LlamaSampler::dist(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u32)
                .unwrap_or(42),
        ),
    ]);

    let mut summary = String::new();
    let mut token_pos = tokens.len() as i32;
    let eos_token = model.token_eos();

    for _ in 0..SUMMARY_MAX_TOKENS {
        let next_token = sampler.sample(ctx, -1);
        if next_token == eos_token { break; }

        #[allow(deprecated)]
        let token_str = model
            .token_to_str(next_token, llama_cpp_2::model::Special::Tokenize)
            .unwrap_or_default();

        summary.push_str(&token_str);

        batch.clear();
        batch.add(next_token, token_pos, &[0], true)
            .map_err(|e| format!("Summary gen batch add failed: {e}"))?;
        ctx.decode(&mut batch)
            .map_err(|e| format!("Summary gen decode failed: {e}"))?;
        token_pos += 1;
    }

    let result = summary.trim().to_string();
    log_info!(conversation_id, "📦 Summary pass (reused ctx): {} input → {} output chars", text.len(), result.len());
    Ok(result)
}

/// Summarize tool output using chunked map-reduce if needed.
/// Returns the summary prefixed with `[Summarized from N chars]`.
fn summarize_tool_output(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    output: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
) -> Result<String, String> {
    let original_len = output.len();

    // Estimate if output fits in a single pass (~4 chars per token, leave room for prompt + generation)
    let estimated_tokens = output.len() / 4;
    let single_pass_limit = (SUMMARY_CTX_SIZE as usize) - SUMMARY_MAX_TOKENS - 200; // 200 tokens for prompt overhead

    let summary = if estimated_tokens < single_pass_limit {
        // Single pass — output fits in one context
        run_summary_pass(model, backend, output, chat_template_string, conversation_id)?
    } else {
        // Chunked map-reduce: split → summarize each → combine → final pass if needed
        let mut chunk_texts = Vec::new();
        let mut pos = 0;
        while pos < output.len() {
            let mut end = std::cmp::min(pos + SUMMARY_CHUNK_CHARS, output.len());
            // Adjust to char boundary
            while end < output.len() && !output.is_char_boundary(end) {
                end += 1;
            }
            chunk_texts.push(&output[pos..end]);
            pos = end;
        }

        log_info!(conversation_id, "📝 Chunked summarization: {} chars → {} chunks", original_len, chunk_texts.len());

        let mut chunk_summaries = Vec::new();
        for (i, chunk) in chunk_texts.iter().enumerate() {
            match run_summary_pass(model, backend, chunk, chat_template_string, conversation_id) {
                Ok(s) => {
                    log_info!(conversation_id, "📝 Chunk {}/{}: {} → {} chars", i + 1, chunk_texts.len(), chunk.len(), s.len());
                    chunk_summaries.push(s);
                }
                Err(e) => {
                    log_warn!(conversation_id, "📝 Chunk {}/{} failed: {}", i + 1, chunk_texts.len(), e);
                    // Use truncated original for this chunk
                    chunk_summaries.push(chunk.chars().take(200).collect::<String>() + "...");
                }
            }
        }

        let combined = chunk_summaries.join("\n");

        // If combined summaries are still large, do a final reduction pass
        if combined.len() > SUMMARIZE_THRESHOLD {
            log_info!(conversation_id, "📝 Final reduction pass: {} chars", combined.len());
            run_summary_pass(model, backend, &combined, chat_template_string, conversation_id)
                .unwrap_or(combined)
        } else {
            combined
        }
    };

    Ok(format!("[Summarized from {} chars]\n{}", original_len, summary))
}

// --- Sub-agent spawning ---

/// Context size for sub-agent (tokens). Large enough for real work.
const AGENT_CTX_SIZE: u32 = 16384;
/// Maximum tokens a sub-agent can generate.
const AGENT_MAX_TOKENS: usize = 8192;

/// Global depth counter to prevent recursive sub-agent spawning.
/// When > 0, spawn_agent calls are rejected with an error.
static AGENT_DEPTH: AtomicU32 = AtomicU32::new(0);

/// Run a sub-agent: create a fresh context, format a prompt with the task,
/// and generate a complete response (with tool calls) until EOS or max tokens.
///
/// The sub-agent shares the loaded model but gets its own KV cache, so it
/// doesn't pollute the main conversation's context window.
pub fn run_sub_agent(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    task: &str,
    extra_context: Option<&str>,
    chat_template_string: Option<&str>,
    conversation_id: &str,
    tags: &ToolTags,
    web_search_provider: Option<&str>,
    web_search_api_key: Option<&str>,
    use_rtk: bool,
    use_htmd: bool,
    browser_backend: &crate::web::browser::BrowserBackend,
    mcp_manager: Option<Arc<crate::web::mcp::McpManager>>,
    db: crate::web::database::SharedDatabase,
    token_sender: &Option<mpsc::UnboundedSender<TokenData>>,
) -> Result<String, String> {
    use super::templates::get_behavioral_system_prompt;
    use std::sync::atomic::Ordering;

    // Prevent recursive sub-agent spawning
    let depth = AGENT_DEPTH.fetch_add(1, Ordering::SeqCst);
    if depth > 0 {
        AGENT_DEPTH.fetch_sub(1, Ordering::SeqCst);
        return Err("Sub-agents cannot spawn other sub-agents (recursion prevented)".to_string());
    }

    // RAII guard to decrement depth on exit (normal or early return via ?)
    struct DepthGuard;
    impl Drop for DepthGuard {
        fn drop(&mut self) {
            AGENT_DEPTH.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let _guard = DepthGuard;

    log_info!(conversation_id, "🤖 Spawning sub-agent for task: {}", &task[..task.len().min(200)]);

    // Stream a status message to the frontend
    if let Some(ref sender) = token_sender {
        let _ = sender.send(TokenData {
            token: "\n[Sub-agent started]\n".to_string(),
            tokens_used: 0,
            max_tokens: AGENT_CTX_SIZE as i32, status: None,
        });
    }

    // Build the user message for the sub-agent
    let user_message = if let Some(ctx) = extra_context {
        if ctx.is_empty() {
            task.to_string()
        } else {
            format!("{}\n\n## Additional Context\n{}", task, ctx)
        }
    } else {
        task.to_string()
    };

    // Format the prompt using the chat template
    let system_prompt = get_behavioral_system_prompt();
    let formatted_prompt = if let Some(template_str) = chat_template_string {
        use super::jinja_templates::{apply_native_chat_template, ChatMessage};
        #[allow(deprecated)]
        use llama_cpp_2::model::Special;
        #[allow(deprecated)]
        let bos = model.token_to_str(model.token_bos(), Special::Tokenize)
            .unwrap_or_else(|_| "<s>".into());
        #[allow(deprecated)]
        let eos = model.token_to_str(model.token_eos(), Special::Tokenize)
            .unwrap_or_else(|_| "</s>".into());

        let tools = super::jinja_templates::get_available_tools_openai();
        let messages = vec![
            ChatMessage { role: "system".into(), content: system_prompt.clone(), tool_calls: None },
            ChatMessage { role: "user".into(), content: user_message, tool_calls: None },
        ];
        apply_native_chat_template(template_str, messages, Some(tools), None, true, &bos, &eos)
            .unwrap_or_else(|_| format!("SYSTEM:\n{}\n\nUSER:\n{}\n\nASSISTANT:\n", system_prompt, task))
    } else {
        format!("SYSTEM:\n{}\n\nUSER:\n{}\n\nASSISTANT:\n", system_prompt, task)
    };

    // Tokenize
    let tokens = model
        .str_to_token(&formatted_prompt, AddBos::Never)
        .map_err(|e| format!("Sub-agent tokenization failed: {e}"))?;

    if tokens.len() + AGENT_MAX_TOKENS > AGENT_CTX_SIZE as usize {
        return Err(format!(
            "Sub-agent prompt too large: {} tokens (max context {})",
            tokens.len(), AGENT_CTX_SIZE
        ));
    }

    log_info!(conversation_id, "🤖 Sub-agent prompt: {} tokens", tokens.len());

    // Create a fresh context (offload_kqv=false to avoid competing for VRAM)
    let n_ctx = NonZeroU32::new(AGENT_CTX_SIZE).unwrap();
    let config = SamplerConfig::default();
    let mut ctx = create_fresh_context(model, backend, n_ctx, false, &config)?;

    // Eval prompt in batches
    let batch_cap = 512usize;
    let mut batch = LlamaBatch::new(batch_cap, 1);
    let n_chunks = tokens.len().div_ceil(batch_cap);
    for chunk_idx in 0..n_chunks {
        let start = chunk_idx * batch_cap;
        let end = std::cmp::min(start + batch_cap, tokens.len());
        batch.clear();
        for (offset, &token) in tokens[start..end].iter().enumerate() {
            let pos = (start + offset) as i32;
            let is_last = start + offset == tokens.len() - 1;
            batch.add(token, pos, &[0], is_last)
                .map_err(|e| format!("Sub-agent batch add failed: {e}"))?;
        }
        ctx.decode(&mut batch)
            .map_err(|e| format!("Sub-agent prompt decode failed: {e}"))?;
    }

    // Create sampler (moderate temperature for tool-calling agent)
    let mut sampler = LlamaSampler::chain_simple(vec![
        LlamaSampler::temp(0.4),
        LlamaSampler::dist(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u32)
                .unwrap_or(42),
        ),
    ]);

    // Generate tokens in a loop, executing tool calls as they appear
    let mut response = String::new();
    let mut token_pos = tokens.len() as i32;
    let eos_token = model.token_eos();
    let mut last_exec_scan_pos = 0usize;
    let mut recent_commands: Vec<String> = Vec::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut tool_calls_executed = 0u32;
    const MAX_AGENT_TOOL_CALLS: u32 = 20;

    for _ in 0..AGENT_MAX_TOKENS {
        let next_token = sampler.sample(&ctx, -1);
        if next_token == eos_token { break; }

        #[allow(deprecated)]
        let token_str = model
            .token_to_str(next_token, llama_cpp_2::model::Special::Tokenize)
            .unwrap_or_default();

        response.push_str(&token_str);

        // Decode the generated token
        batch.clear();
        batch.add(next_token, token_pos, &[0], true)
            .map_err(|e| format!("Sub-agent gen batch add failed: {e}"))?;
        ctx.decode(&mut batch)
            .map_err(|e| format!("Sub-agent gen decode failed: {e}"))?;
        token_pos += 1;

        // Stream sub-agent tokens to frontend (prefixed so user can distinguish)
        if let Some(ref sender) = token_sender {
            let _ = sender.send(TokenData {
                token: token_str.clone(),
                tokens_used: token_pos,
                max_tokens: AGENT_CTX_SIZE as i32, status: None,
            });
        }

        // Check for tool calls in the generated response
        let token_has_close_char = token_str.as_bytes().iter().any(|&b| b == b'>' || b == b']' || b == b'}');
        if token_has_close_char && tool_calls_executed < MAX_AGENT_TOOL_CALLS {
            if let Ok(Some(exec_result)) = check_and_execute_command_with_tags(
                &response, last_exec_scan_pos, conversation_id, model, tags,
                None, // template_type
                web_search_provider, web_search_api_key,
                &mut recent_commands, token_sender, token_pos,
                AGENT_CTX_SIZE, Some(cancel.clone()),
                use_rtk, use_htmd, browser_backend,
                mcp_manager.clone(), db.clone(),
                backend, chat_template_string,
            ) {
                tool_calls_executed += 1;
                log_info!(conversation_id, "🤖 Sub-agent tool call #{}: output {} chars", tool_calls_executed, exec_result.output_block.len());

                // Append output to response text
                response.push_str(&exec_result.output_block);
                last_exec_scan_pos = response.len();

                // Inject output tokens into sub-agent context
                match inject_output_tokens(
                    &exec_result.model_tokens, &mut batch, &mut ctx,
                    &mut token_pos, conversation_id,
                ) {
                    Ok(()) => {},
                    Err(e) if e == "CONTEXT_EXHAUSTED" => {
                        log_info!(conversation_id, "🤖 Sub-agent context exhausted after tool call");
                        break;
                    }
                    Err(e) => {
                        log_warn!(conversation_id, "🤖 Sub-agent token injection failed: {}", e);
                        break;
                    }
                }
            }
        }
    }

    drop(ctx);

    let result = response.trim().to_string();
    log_info!(
        conversation_id,
        "🤖 Sub-agent finished: {} chars, {} tool calls",
        result.len(),
        tool_calls_executed
    );

    // Stream end marker
    if let Some(ref sender) = token_sender {
        let _ = sender.send(TokenData {
            token: "\n[Sub-agent finished]\n".to_string(),
            tokens_used: 0,
            max_tokens: AGENT_CTX_SIZE as i32, status: None,
        });
    }

    Ok(result)
}

/// Try to extract a spawn_agent tool call from command text.
/// Returns Some((task, optional_context)) if recognized, None otherwise.
fn try_extract_spawn_agent(text: &str) -> Option<(String, Option<String>)> {
    let calls = native_tools::try_parse_all_from_raw(text.trim());
    for (name, args) in calls {
        if name == "spawn_agent" {
            let task = args.get("task").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let context = args.get("context").and_then(|v| v.as_str()).map(|s| s.to_string());
            return Some((task, context));
        }
    }
    None
}

/// Prefix a command with `rtk` for output compression, if RTK mode is enabled.
fn maybe_rtk_prefix(cmd: &str, use_rtk: bool) -> String {
    if use_rtk {
        format!("rtk {}", cmd)
    } else {
        cmd.to_string()
    }
}

// Default SYSTEM.EXEC regex (always tried as fallback)
// (?s) enables DOTALL mode so . matches newlines (multi-line commands)
// Closing tag: models may emit <SYSTEM.EXEC||> (correct) or <||SYSTEM.EXEC||> (mirrored opening)
lazy_static::lazy_static! {
    pub static ref EXEC_PATTERN: Regex = Regex::new(
        r"(?s)SYSTEM\.EXEC>(.+?)<(?:\|{1,2})?SYSTEM\.EXEC\|{1,2}>"
    ).unwrap();

    // Llama3/Hermes XML format: <function=tool_name> ... </function>
    // Some models (Qwen3-Coder) output this without a <tool_call> wrapper.
    static ref LLAMA3_FUNC_PATTERN: Regex = Regex::new(
        r"(?s)(<function=[a-z_]+>.*?</function>)"
    ).unwrap();

    // Harmony format (gpt-oss-20b):
    //   Hardcoded path: to= tool_name code<|message|>{...}<|call|>
    //   Jinja path:     to=functions.tool_name <|constrain|>json<|message|>{...}<|call|>
    // Both end with <|message|>JSON<|call|>, differ in prefix and middle.
    static ref HARMONY_CALL_PATTERN: Regex = Regex::new(
        r"(?s)to=\s*(?:functions\.)?(\w+)[\s\S]*?<\|message\|>(.*?)<\|call\|>"
    ).unwrap();

    // Mistral v2 bracket format (Devstral-Small-2-2512):
    // [TOOL_CALLS]tool_name[ARGS]{"arg":"val"}
    // Only matches the prefix — JSON body is extracted via balanced-brace scanner
    // because non-greedy \{.*?\} fails on nested JSON (e.g. write_file with JSON content).
    static ref MISTRAL_BRACKET_PREFIX: Regex = Regex::new(
        r"\[TOOL_CALLS\](\w+)\[ARGS\]"
    ).unwrap();

    // Mistral JSON format (Magistral-Small-2509):
    // [TOOL_CALLS]{"name":"tool_name","arguments":{...}}
    // The [TOOL_CALLS] tag is followed directly by a JSON object (no name[ARGS] separator).
    static ref MISTRAL_JSON_PREFIX: Regex = Regex::new(
        r"\[TOOL_CALLS\]\s*\{"
    ).unwrap();
}

/// Extract balanced JSON starting at position `start` in `text`.
/// `text[start]` must be `{`. Respects string quoting so nested `{}`
/// inside JSON strings don't break the match.
/// Returns `(end_exclusive, json_slice)` on success.
fn extract_balanced_json(text: &str, start: usize) -> Option<(usize, String)> {
    let bytes = text.as_bytes();
    if start >= bytes.len() || bytes[start] != b'{' {
        return None;
    }
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut prev_backslash = false;
    for (i, &b) in bytes[start..].iter().enumerate() {
        if in_string {
            if b == b'"' && !prev_backslash {
                in_string = false;
            }
            prev_backslash = b == b'\\' && !prev_backslash;
        } else {
            match b {
                b'"' => {
                    in_string = true;
                    prev_backslash = false;
                }
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        let end = start + i + 1;
                        return Some((end, text[start..end].to_string()));
                    }
                }
                _ => {}
            }
        }
    }
    None
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
    //
    // NOTE: GLM models open tool calls with <tool_call> but close with <|end_of_box|>
    // (a vision bounding box marker they repurpose as tool call terminator).
    // We accept <|end_of_box|> as an alternative close ONLY when open is <tool_call>,
    // since using <|begin_of_box|> as an alternative *open* tag caused false positives
    // (GLM uses <|begin_of_box|> for thinking boxes → matched non-tool text).
    let close_alt = if tags.exec_open == "<tool_call>" {
        let ebox = regex::escape("<|end_of_box|>");
        format!("(?:{close}|{ebox})")
    } else {
        close.to_string()
    };
    let pattern = format!(r"(?s){open}(.+?){close_alt}");
    Regex::new(&pattern).ok()
}

// --- Format detectors: each returns the extracted command text or None ---

type FormatDetector = fn(&str, &ToolTags) -> Option<String>;

/// Detector priority order. First match wins.
const FORMAT_PRIORITY: &[(&str, FormatDetector)] = &[
    ("model_specific", detect_model_specific),
    ("exec", detect_exec),
    ("llama3", detect_llama3),
    ("harmony", detect_harmony),
    ("mistral_bracket", detect_mistral_bracket),
    ("mistral_json", detect_mistral_json),
];

fn detect_model_specific(text: &str, tags: &ToolTags) -> Option<String> {
    let re = build_model_exec_regex(tags)?;
    re.captures(text)?.get(1).map(|m| m.as_str().to_string())
}

fn detect_exec(text: &str, _tags: &ToolTags) -> Option<String> {
    EXEC_PATTERN.captures(text)?.get(1).map(|m| m.as_str().to_string())
}

fn detect_llama3(text: &str, _tags: &ToolTags) -> Option<String> {
    LLAMA3_FUNC_PATTERN.captures(text)?.get(1).map(|m| m.as_str().to_string())
}

fn detect_harmony(text: &str, _tags: &ToolTags) -> Option<String> {
    let caps = HARMONY_CALL_PATTERN.captures(text)?;
    let (tool_name, args_json) = (caps.get(1)?, caps.get(2)?);
    Some(format!(
        r#"{{"name":"{}","arguments":{}}}"#,
        tool_name.as_str(),
        args_json.as_str().trim()
    ))
}

fn detect_mistral_bracket(text: &str, _tags: &ToolTags) -> Option<String> {
    let caps = MISTRAL_BRACKET_PREFIX.captures(text)?;
    let tool_name = caps.get(1)?;
    let json_start = caps.get(0)?.end();
    let (_end, args_json) = extract_balanced_json(text, json_start)?;
    Some(format!(
        r#"{{"name":"{}","arguments":{}}}"#,
        tool_name.as_str(),
        args_json.trim()
    ))
}

fn detect_mistral_json(text: &str, _tags: &ToolTags) -> Option<String> {
    let m = MISTRAL_JSON_PREFIX.find(text)?;
    // The `{` is at the end of the match, so JSON starts at match.end() - 1
    let json_start = m.end() - 1;
    let (_end, json) = extract_balanced_json(text, json_start)?;
    // Validate it has the expected "name" and "arguments" fields
    let parsed: serde_json::Value = serde_json::from_str(&json).ok()?;
    if parsed.get("name").is_some() && parsed.get("arguments").is_some() {
        Some(json)
    } else {
        None
    }
}

/// Result of command execution
pub struct CommandExecutionResult {
    /// Display block for frontend/logging (just the output tags, no chat template wrapping)
    pub output_block: String,
    /// Tokens for model context injection (wrapped in chat template turn structure)
    pub model_tokens: Vec<i32>,
    /// The template-wrapped text used for model context injection.
    /// Needed by the vision path to re-tokenize with `<__media__>` markers via MtmdContext.
    #[allow(dead_code)]
    pub model_block: String,
    /// Raw image bytes from tool responses (e.g., screenshots) for vision pipeline injection.
    /// When non-empty AND the model has vision capability, these are fed as image embeddings
    /// instead of (or alongside) the text tokens.
    #[allow(dead_code)]
    pub response_images: Vec<Vec<u8>>,
}

/// Maximum number of times the same command can be repeated before blocking.
const MAX_COMMAND_REPEATS: usize = 3;

/// Timeout for native tool execution (web_search, web_fetch, etc.)
const NATIVE_TOOL_TIMEOUT_SECS: u64 = 30;

/// Run a native tool with a timeout to prevent blocking the generation thread indefinitely.
fn run_native_tool_with_timeout(
    command_text: &str,
    web_search_provider: Option<&str>,
    web_search_api_key: Option<&str>,
    conversation_id: &str,
    use_htmd: bool,
    browser_backend: crate::web::browser::BrowserBackend,
    mcp_manager: Option<Arc<crate::web::mcp::McpManager>>,
    db: crate::web::database::SharedDatabase,
) -> Option<native_tools::NativeToolResult> {
    let cmd = command_text.to_string();
    let provider = web_search_provider.map(|s| s.to_string());
    let api_key = web_search_api_key.map(|s| s.to_string());
    let mcp = mcp_manager.clone();
    let db = db.clone();

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = native_tools::dispatch_native_tool(
            &cmd,
            provider.as_deref(),
            api_key.as_deref(),
            use_htmd,
            &browser_backend,
            mcp.as_deref(),
            Some(&db),
        );
        let _ = tx.send(result);
    });

    match rx.recv_timeout(std::time::Duration::from_secs(NATIVE_TOOL_TIMEOUT_SECS)) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            log_info!(conversation_id, "⏱️ Native tool timed out after {}s", NATIVE_TOOL_TIMEOUT_SECS);
            Some(native_tools::NativeToolResult::text_only(format!("Error: Tool execution timed out after {} seconds. The network request may be slow or unresponsive. Please try again.", NATIVE_TOOL_TIMEOUT_SECS)))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            log_info!(conversation_id, "⚠️ Native tool thread panicked");
            Some(native_tools::NativeToolResult::text_only("Error: Tool execution failed unexpectedly.".to_string()))
        }
    }
}

/// Execute a single tool call given its parsed name and arguments.
/// Returns (text_output, image_bytes). Used by the batch execution path.
fn execute_single_tool(
    name: &str,
    args: &serde_json::Value,
    tool_json: &str,
    conversation_id: &str,
    web_search_provider: Option<&str>,
    web_search_api_key: Option<&str>,
    token_sender: &Option<mpsc::UnboundedSender<TokenData>>,
    token_pos: i32,
    context_size: u32,
    cancel: Option<Arc<AtomicBool>>,
    use_rtk: bool,
    use_htmd: bool,
    browser_backend: &crate::web::browser::BrowserBackend,
    mcp_manager: Option<Arc<crate::web::mcp::McpManager>>,
    db: crate::web::database::SharedDatabase,
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    chat_template_string: Option<&str>,
    tags: &ToolTags,
) -> (String, Vec<Vec<u8>>) {
    // spawn_agent: run a sub-agent with fresh context
    if name == "spawn_agent" {
        let task = args.get("task").and_then(|v| v.as_str()).unwrap_or("");
        if task.is_empty() {
            return ("Error: 'task' argument is required for spawn_agent".to_string(), Vec::new());
        }
        let extra_context = args.get("context").and_then(|v| v.as_str());
        match run_sub_agent(
            model, backend, task, extra_context, chat_template_string,
            conversation_id, tags, web_search_provider, web_search_api_key,
            use_rtk, use_htmd, browser_backend, mcp_manager.clone(), db.clone(),
            token_sender,
        ) {
            Ok(result) => return (result, Vec::new()),
            Err(e) => return (format!("Sub-agent error: {}", e), Vec::new()),
        }
    }

    // execute_command gets streaming or background treatment (no images)
    if name == "execute_command" {
        if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
            if !cmd.is_empty() {
                let is_background = args.get("background").map(|v| {
                    v.as_bool().unwrap_or_else(|| {
                        v.as_str().map(|s| matches!(s.trim().to_lowercase().as_str(), "true" | "1" | "yes")).unwrap_or(false)
                    })
                }).unwrap_or(false);
                let timeout_secs = args.get("timeout").and_then(|v| {
                    v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
                });
                let rtk_cmd = maybe_rtk_prefix(cmd, use_rtk);
                if is_background {
                    log_info!(conversation_id, "🐚 Batch: background execute_command: {}", rtk_cmd);
                    let sender_clone = token_sender.clone();
                    let text = execute_command_background(&rtk_cmd, |line| {
                        if let Some(ref sender) = sender_clone {
                            let _ = sender.send(TokenData {
                                token: format!("{}\n", strip_ansi_codes(line)),
                                tokens_used: token_pos,
                                max_tokens: context_size as i32, status: None,
                            });
                        }
                    });
                    return (text, Vec::new());
                } else {
                    log_info!(conversation_id, "🐚 Batch: streaming execute_command (timeout={}s): {}", timeout_secs.unwrap_or(300), rtk_cmd);
                    let sender_clone = token_sender.clone();
                    let text = execute_command_streaming_with_timeout(&rtk_cmd, cancel, timeout_secs, &mut |line| {
                        if let Some(ref sender) = sender_clone {
                            let _ = sender.send(TokenData {
                                token: format!("{}\n", strip_ansi_codes(line)),
                                tokens_used: token_pos,
                                max_tokens: context_size as i32, status: None,
                            });
                        }
                    });
                    return (text, Vec::new());
                }
            }
        }
    }

    // Try native tool dispatch (may return images for vision)
    if let Some(native_result) = run_native_tool_with_timeout(
        tool_json,
        web_search_provider,
        web_search_api_key,
        conversation_id,
        use_htmd,
        browser_backend.clone(),
        mcp_manager.clone(),
        db.clone(),
    ) {
        log_info!(conversation_id, "📦 Batch: native tool '{}' dispatched (images={})", name, native_result.images.len());
        if let Some(ref sender) = token_sender {
            let _ = sender.send(TokenData {
                token: native_result.text.trim().to_string(),
                tokens_used: token_pos,
                max_tokens: context_size as i32, status: None,
            });
        }
        return (native_result.text, native_result.images);
    }

    // Fallback: unknown tool
    let err = format!("Error: Unknown or unsupported tool '{}'", name);
    if let Some(ref sender) = token_sender {
        let _ = sender.send(TokenData {
            token: err.clone(),
            tokens_used: token_pos,
            max_tokens: context_size as i32, status: None,
        });
    }
    (err, Vec::new())
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
    web_search_api_key: Option<&str>,
    recent_commands: &mut Vec<String>,
    token_sender: &Option<mpsc::UnboundedSender<TokenData>>,
    token_pos: i32,
    context_size: u32,
    cancel: Option<Arc<AtomicBool>>,
    use_rtk: bool,
    use_htmd: bool,
    browser_backend: &crate::web::browser::BrowserBackend,
    mcp_manager: Option<Arc<crate::web::mcp::McpManager>>,
    db: crate::web::database::SharedDatabase,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    chat_template_string: Option<&str>,
) -> Result<Option<CommandExecutionResult>, String> {
    // Only scan new content since last command execution
    let response_to_scan = if last_scan_pos < response.len() {
        // Adjust to char boundary to avoid panicking on multi-byte UTF-8
        let mut pos = last_scan_pos;
        while pos < response.len() && !response.is_char_boundary(pos) {
            pos += 1;
        }
        &response[pos..]
    } else {
        return Ok(None);
    };

    // Fast path: skip expensive regex checks unless we see a closing tag character.
    // Command blocks always end with '>' (SYSTEM.EXEC, </tool_call>, </function>)
    // or ']' ([/TOOL_CALLS], [ARGS]{...}) or '}' (JSON tool calls).
    // This avoids running 6 regex patterns on every single token.
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
                // Log when we have tag characters but no format matches — throttled to
                // avoid flooding logs (previously caused 12K+ lines during repetition loops).
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

    // Loop detection: check if this command was recently executed
    // Exempt wait/sleep (intentional delays) and check_background_process (throttled by wait + no_output_checks)
    let normalized_cmd = command_text.trim().to_string();
    // Match tool names across all model formats:
    // JSON: "name": "wait"  |  Llama3 XML: <function=wait>  |  GLM: wait\n<arg_key>  |  Mistral: wait,{
    let cmd_lower = normalized_cmd.to_lowercase();
    let is_wait_or_poll = cmd_lower.contains("wait")
        || cmd_lower.contains("sleep")
        || cmd_lower.contains("check_background_process");
    let repeat_count = recent_commands.iter().filter(|c| *c == &normalized_cmd).count();
    // Fuzzy similarity: count commands that share 80%+ of their first 50 chars
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
            return Ok(Some(CommandExecutionResult {
                output_block,
                model_tokens: model_tokens.iter().map(|t| t.0).collect(),
                model_block,
                response_images: Vec::new(),
            }));
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
        return Ok(Some(CommandExecutionResult {
            output_block,
            model_tokens: model_tokens.iter().map(|t| t.0).collect(),
            model_block,
            response_images: Vec::new(),
        }));
    }
    // (normalized_cmd already pushed above for loop detection tracking)

    // Parse all tool calls from the command text (supports JSON arrays for batch calls)
    let all_calls = native_tools::try_parse_all_from_raw(&command_text);
    let is_batch = all_calls.len() > 1;

    if is_batch {
        log_info!(
            conversation_id,
            "📦 Batch tool call: {} tools detected",
            all_calls.len()
        );
    }

    // Stream the output_open tag to frontend immediately so the UI shows the block
    let output_open = format!("\n{}\n", tags.output_open);
    let output_close = format!("\n{}\n", tags.output_close);

    if let Some(ref sender) = token_sender {
        let _ = sender.send(TokenData {
            token: output_open.clone(),
            tokens_used: token_pos,
            max_tokens: context_size as i32, status: None,
        });
    }

    // Collect images from tool responses for vision pipeline
    let mut all_response_images: Vec<Vec<u8>> = Vec::new();

    let output = if is_batch {
        // === Batch execution path: parallel for native tools, sequential for execute_command ===
        let mut combined_output = String::new();

        // Partition tool calls into parallel (native) and sequential (execute_command)
        let mut parallel_indices: Vec<usize> = Vec::new();
        let mut sequential_indices: Vec<usize> = Vec::new();
        for (i, (name, _args)) in all_calls.iter().enumerate() {
            if name == "execute_command" || name == "spawn_agent" {
                sequential_indices.push(i);
            } else {
                parallel_indices.push(i);
            }
        }

        // Pre-allocate result slots: (text, images)
        let mut results: Vec<Option<(String, Vec<Vec<u8>>)>> = vec![None; all_calls.len()];

        // Execute parallel (native) tools concurrently via thread::scope
        if !parallel_indices.is_empty() {
            log_info!(
                conversation_id,
                "⚡ Executing {} native tools in parallel",
                parallel_indices.len()
            );

            // Prepare owned data for threads
            let thread_data: Vec<(usize, String, Option<String>, Option<String>, String)> = parallel_indices
                .iter()
                .map(|&i| {
                    let (name, args) = &all_calls[i];
                    let single_json = serde_json::json!({"name": name, "arguments": args}).to_string();
                    let provider = web_search_provider.map(|s| s.to_string());
                    let api_key = web_search_api_key.map(|s| s.to_string());
                    let conv_id = conversation_id.to_string();
                    (i, single_json, provider, api_key, conv_id)
                })
                .collect();

            std::thread::scope(|s| {
                let handles: Vec<_> = thread_data
                    .iter()
                    .map(|(idx, json, provider, api_key, conv_id)| {
                        let idx = *idx;
                        let tool_name = all_calls[idx].0.clone();
                        let mcp_clone = mcp_manager.clone();
                        let backend_clone = browser_backend.clone();
                        let db_clone = db.clone();
                        s.spawn(move || {
                            let result = run_native_tool_with_timeout(
                                json,
                                provider.as_deref(),
                                api_key.as_deref(),
                                conv_id,
                                use_htmd,
                                backend_clone,
                                mcp_clone,
                                db_clone,
                            );
                            let native_result = result.unwrap_or_else(|| {
                                native_tools::NativeToolResult::text_only(
                                    format!("Error: Tool '{}' returned no output", tool_name)
                                )
                            });
                            (idx, native_result.text, native_result.images)
                        })
                    })
                    .collect();

                for handle in handles {
                    if let Ok((idx, text, images)) = handle.join() {
                        results[idx] = Some((text, images));
                    }
                }
            });
        }

        // Execute sequential tools (execute_command) in order
        for &i in &sequential_indices {
            let (name, args) = &all_calls[i];
            let single_json = serde_json::json!({"name": name, "arguments": args}).to_string();
            let (tool_output, tool_images) = execute_single_tool(
                name, args, &single_json,
                conversation_id,
                web_search_provider,
                web_search_api_key,
                token_sender,
                token_pos,
                context_size,
                cancel.clone(),
                use_rtk,
                use_htmd,
                browser_backend,
                mcp_manager.clone(),
                db.clone(),
                model, backend, chat_template_string, tags,
            );
            results[i] = Some((tool_output, tool_images));
        }

        // Merge results in original order, streaming to frontend
        for (i, (name, _args)) in all_calls.iter().enumerate() {
            let header = format!("[Tool {}: {}]\n", i + 1, name);
            if let Some(ref sender) = token_sender {
                let _ = sender.send(TokenData {
                    token: header.clone(),
                    tokens_used: token_pos,
                    max_tokens: context_size as i32, status: None,
                });
            }
            combined_output.push_str(&header);

            let (tool_output, tool_images) = results[i].take().unwrap_or_default();
            all_response_images.extend(tool_images);
            log_info!(
                conversation_id,
                "📤 Tool {} ({}) output: {} chars",
                i + 1,
                name,
                tool_output.len()
            );

            if let Some(ref sender) = token_sender {
                let _ = sender.send(TokenData {
                    token: tool_output.trim().to_string(),
                    tokens_used: token_pos,
                    max_tokens: context_size as i32, status: None,
                });
            }
            combined_output.push_str(tool_output.trim());
            if i < all_calls.len() - 1 {
                combined_output.push_str("\n\n");
                if let Some(ref sender) = token_sender {
                    let _ = sender.send(TokenData {
                        token: "\n\n".to_string(),
                        tokens_used: token_pos,
                        max_tokens: context_size as i32, status: None,
                    });
                }
            }
        }

        combined_output
    } else {
        // === Single execution path (existing logic) ===
        // Check for spawn_agent first — needs model/backend access, can't go through native tool path
        if let Some(agent_result) = try_extract_spawn_agent(&command_text) {
            let (task, extra_context) = agent_result;
            if task.is_empty() {
                "Error: 'task' argument is required for spawn_agent".to_string()
            } else {
                match run_sub_agent(
                    model, backend, &task, extra_context.as_deref(), chat_template_string,
                    conversation_id, tags, web_search_provider, web_search_api_key,
                    use_rtk, use_htmd, browser_backend, mcp_manager.clone(), db.clone(),
                    token_sender,
                ) {
                    Ok(result) => result,
                    Err(e) => format!("Sub-agent error: {}", e),
                }
            }
        }
        // Check if this is an `execute_command` tool call — route through streaming or background path
        // so the UI shows line-by-line output for long-running commands (composer, npm, etc.)
        else if let Some((cmd, is_background)) = native_tools::extract_execute_command_with_opts(&command_text) {
            let rtk_cmd = maybe_rtk_prefix(&cmd, use_rtk);
            if is_background {
                log_info!(conversation_id, "🐚 Background execute_command: {}", rtk_cmd);
                let sender_clone = token_sender.clone();
                execute_command_background(&rtk_cmd, |line| {
                    if let Some(ref sender) = sender_clone {
                        let _ = sender.send(TokenData {
                            token: format!("{}\n", strip_ansi_codes(line)),
                            tokens_used: token_pos,
                            max_tokens: context_size as i32, status: None,
                        });
                    }
                })
            } else {
                log_info!(conversation_id, "🐚 Streaming execute_command: {}", rtk_cmd);
                let sender_clone = token_sender.clone();
                execute_command_streaming(&rtk_cmd, cancel.clone(), |line| {
                    if let Some(ref sender) = sender_clone {
                        let _ = sender.send(TokenData {
                            token: format!("{}\n", strip_ansi_codes(line)),
                            tokens_used: token_pos,
                            max_tokens: context_size as i32, status: None,
                        });
                    }
                })
            }
        } else if let Some(native_result) = run_native_tool_with_timeout(
            &command_text,
            web_search_provider,
            web_search_api_key,
            conversation_id,
            use_htmd,
            browser_backend.clone(),
            mcp_manager.clone(),
            db.clone(),
        ) {
            log_info!(conversation_id, "📦 Dispatched to native tool handler (images={})", native_result.images.len());
            // Non-execute tools complete quickly, stream their output at once
            if let Some(ref sender) = token_sender {
                let _ = sender.send(TokenData {
                    token: native_result.text.trim().to_string(),
                    tokens_used: token_pos,
                    max_tokens: context_size as i32, status: None,
                });
            }
            all_response_images.extend(native_result.images);
            native_result.text
        } else {
            let trimmed_cmd = command_text.trim();
            if trimmed_cmd.starts_with('{') || trimmed_cmd.starts_with('[') {
                // Looks like a JSON tool call that failed to parse — don't execute as shell.
                log_info!(conversation_id, "⚠️ JSON-like tool call failed to parse, returning error to model");
                let err_msg = "Error: Failed to parse tool call JSON. The JSON may be malformed (check for unescaped backslashes, missing braces, or literal newlines in strings). Please try the execute_command tool to write files instead.".to_string();
                if let Some(ref sender) = token_sender {
                    let _ = sender.send(TokenData {
                        token: err_msg.clone(),
                        tokens_used: token_pos,
                        max_tokens: context_size as i32, status: None,
                    });
                }
                err_msg
            } else {
                log_info!(conversation_id, "🐚 Falling back to streaming shell execution");
                // Use streaming execution — each line is sent to frontend as it arrives
                let rtk_cmd = maybe_rtk_prefix(&command_text, use_rtk);
                let sender_clone = token_sender.clone();
                execute_command_streaming(&rtk_cmd, cancel.clone(), |line| {
                    if let Some(ref sender) = sender_clone {
                        let _ = sender.send(TokenData {
                            token: format!("{}\n", strip_ansi_codes(line)),
                            tokens_used: token_pos,
                            max_tokens: context_size as i32, status: None,
                        });
                    }
                })
            }
        }
    };
    log_info!(
        conversation_id,
        "📤 Command output length: {} chars",
        output.len()
    );

    // Sanitize output: strip ANSI codes + truncate long output.
    let sanitized = sanitize_command_output(&output);

    // Summarize large outputs via LLM sub-agent to save context tokens.
    // The user sees the original output (persisted in output_block);
    // the model only receives the summary (injected via model_block/model_tokens).
    // Use original output length to decide summarization — the sanitized version may
    // be heavily truncated but the user still sees the full streamed output.
    let (display_text, model_text) = if output.len() > SUMMARIZE_THRESHOLD || sanitized.len() > SUMMARIZE_THRESHOLD {
        match summarize_tool_output(model, backend, &sanitized, chat_template_string, conversation_id) {
            Ok(summary) => {
                log_info!(conversation_id, "📝 Summarized tool output: {} → {} chars", sanitized.len(), summary.len());
                // Stream summary with actual content to frontend (before output_close)
                let summary_block = format!(
                    "\n\n📝 Summary for model ({} → {} chars):\n{}",
                    sanitized.len(), summary.len(), summary.trim()
                );
                if let Some(ref sender) = token_sender {
                    let _ = sender.send(TokenData {
                        token: summary_block.clone(),
                        tokens_used: token_pos,
                        max_tokens: context_size as i32, status: None,
                    });
                }
                // Display: original output + summary with content
                // Model: summary only
                let display = format!("{}{}", sanitized, summary_block);
                (display, summary)
            }
            Err(e) => {
                log_warn!(conversation_id, "Summarization failed ({}), using raw output", e);
                (sanitized.clone(), sanitized)
            }
        }
    } else {
        (sanitized.clone(), sanitized)
    };

    // Stream the output_close tag to frontend (after any summary note)
    if let Some(ref sender) = token_sender {
        let _ = sender.send(TokenData {
            token: output_close.clone(),
            tokens_used: token_pos,
            max_tokens: context_size as i32, status: None,
        });
    }

    // output_block: persisted in conversation — contains original output for user display
    let output_block = format!("{}{}{}", output_open, display_text.trim(), output_close);

    // Detect dead links / HTTP errors and hint the model to search online
    let model_trimmed = model_text.trim();
    let http_error_hint = {
        let lower = model_trimmed.to_lowercase();
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
    };

    // Build model text with warnings
    let mut model_text_with_warning = model_trimmed.to_string();
    if let Some(ref warning) = fuzzy_warning {
        model_text_with_warning = format!("{}\n\n{}", warning, model_text_with_warning);
    }
    if let Some(hint) = http_error_hint {
        model_text_with_warning = format!("{}\n\n{}", model_text_with_warning, hint);
    }

    // model_injection_block: contains only the summary — this is what the LLM sees
    let model_injection_block = format!("{}{}{}", output_open, model_text_with_warning, output_close);

    // Build model injection block with chat template turn wrapping.
    // The model needs proper turn structure to know the tool response is from
    // a different role and that it should continue as assistant.
    let model_block = wrap_output_for_model(&model_injection_block, template_type);
    log_info!(
        conversation_id,
        "🔄 Model injection block (template={:?}):\n{}",
        template_type,
        model_block
    );

    let model_tokens = model
        .str_to_token(&model_block, AddBos::Never)
        .map_err(|e| format!("Tokenization of model injection block failed: {e}"))?;

    if !all_response_images.is_empty() {
        eprintln!(
            "[TOOL_RESULT] {} image(s) for vision pipeline, sizes: {:?}",
            all_response_images.len(),
            all_response_images.iter().map(|img| img.len()).collect::<Vec<_>>()
        );
    }

    Ok(Some(CommandExecutionResult {
        output_block,
        model_tokens: model_tokens.iter().map(|t| t.0).collect(),
        model_block,
        response_images: all_response_images,
    }))
}

/// Inject command output tokens into the LLM context.
pub fn inject_output_tokens(
    tokens: &[i32],
    batch: &mut LlamaBatch<'_>,
    context: &mut llama_cpp_2::context::LlamaContext<'_>,
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

        if let Err(e) = context.decode(batch) {
            let err_str = format!("{e}");
            if err_str.contains("NoKvCacheSlot") || err_str.contains("no kv cache slot") {
                return Err("CONTEXT_EXHAUSTED".to_string());
            }
            return Err(format!("Decode failed for command output: {e}"));
        }

        *token_pos += chunk.len() as i32;
    }

    // Check if we've consumed too much context after injection
    // (catches recurrent/hybrid models where decode succeeds but context is full)
    let ctx_size = context.n_ctx();
    if *token_pos as u32 >= ctx_size.saturating_sub(ctx_size / 20) {
        eprintln!("[INJECT] Context 95% full after injection ({}/{})", token_pos, ctx_size);
        return Err("CONTEXT_EXHAUSTED".to_string());
    }

    Ok(())
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
            // GLM-4 family: inject tool result with <|observation|> role marker,
            // then re-open assistant turn so model continues generating.
            // Format: <|observation|>\n<tool_response>\nresult\n</tool_response>\n<|assistant|>\n
            format!("\n<|observation|>\n{}\n<|assistant|>\n", output_block.trim())
        }
        Some("Mistral") | _ => {
            // Mistral and default: output tags are sufficient, no extra turn wrapping needed.
            // Mistral's tool format is inline within the conversation flow.
            output_block.to_string()
        }
    }
}
