use llama_cpp_2::{llama_batch::LlamaBatch, model::AddBos};
use llama_cpp_2::sampling::LlamaSampler;
use std::num::NonZeroU32;

use crate::generation::create_fresh_context;
use llama_chat_types::*;

/// Minimum output size (chars) to trigger LLM sub-agent summarization (GPU).
pub(crate) const SUMMARIZE_THRESHOLD: usize = 4000;
/// Context size for each tool-output summarization pass (tokens).
pub(crate) const SUMMARY_CTX_SIZE: u32 = 8192;
/// Maximum tokens to generate per tool-output summary.
pub(crate) const SUMMARY_MAX_TOKENS: usize = 512;
/// Maximum chars per chunk for map-reduce summarization.
const SUMMARY_CHUNK_CHARS: usize = 5000;
pub(crate) const COMPACT_SUMMARY_CTX_SIZE: u32 = 8192;
pub(crate) const COMPACT_SUMMARY_MAX_TOKENS: usize = 1500;

pub(crate) fn run_summary_pass(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    text: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
) -> Result<String, String> {
    let system_msg = "Summarize this tool output concisely. Keep: file names, errors, key values, success/failure status. Remove: verbose logs, repeated patterns, boilerplate.";

    let formatted_prompt = if let Some(template_str) = chat_template_string {
        use crate::jinja_templates::{apply_native_chat_template, ChatMessage};
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
        apply_native_chat_template(template_str, messages, None, None, true, &bos, &eos, false)
            .unwrap_or_else(|_| format!("SYSTEM:\n{system_msg}\n\nUSER:\n{text}\n\nASSISTANT:\n"))
    } else {
        format!("SYSTEM:\n{system_msg}\n\nUSER:\n{text}\n\nASSISTANT:\n")
    };

    let tokens = model
        .str_to_token(&formatted_prompt, AddBos::Never)
        .map_err(|e| format!("Summary tokenization failed: {e}"))?;

    if tokens.len() + SUMMARY_MAX_TOKENS > SUMMARY_CTX_SIZE as usize {
        return Err(format!("Summary prompt too large: {} tokens", tokens.len()));
    }

    let n_ctx = NonZeroU32::new(SUMMARY_CTX_SIZE).unwrap();
    let config = SamplerConfig::default();
    let mut ctx = create_fresh_context(model, backend, n_ctx, true, &config)?;

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
    let prompt_len = tokens.len() as i32;
    let eos_token = model.token_eos();

    for i in 0..SUMMARY_MAX_TOKENS {
        let next_token = sampler.sample(&ctx, -1);
        if next_token == eos_token { break; }

        #[allow(deprecated)]
        let token_str = model
            .token_to_str(next_token, llama_cpp_2::model::Special::Tokenize)
            .unwrap_or_default();

        summary.push_str(&token_str);

        let token_pos = prompt_len + i as i32;
        batch.clear();
        batch.add(next_token, token_pos, &[0], true)
            .map_err(|e| format!("Summary gen batch add failed: {e}"))?;
        ctx.decode(&mut batch)
            .map_err(|e| format!("Summary gen decode failed: {e}"))?;
    }

    drop(ctx);
    let result = summary.trim().to_string();
    log_info!(conversation_id, "📝 Summary pass: {} input chars → {} output chars", text.len(), result.len());
    Ok(result)
}

pub(crate) const COMPACT_SYSTEM_PROMPT: &str = "\
Summarize this conversation so work can resume seamlessly. \
This summary is the ONLY context available — be complete and precise.

Rules:
- Plain text only. No markdown: no headers (#), no bold (**), no italics, no code fences.
- Start directly with the content. No preamble like 'Here is a summary...'.
- No raw code snippets or file contents — describe what things do and why.
- CRITICAL: This summary represents REAL actions already taken. Any task listed as complete must NOT be repeated.

Cover these areas in order:
1. Primary Request and Intent: what the user asked for, all goals and sub-goals.
2. Task Completion Status: EXPLICITLY state 'COMPLETE', 'IN PROGRESS', or 'NOT STARTED' for every goal. List every tool call made (function name, result summary) to prove real execution happened.
3. Key Technical Concepts: technologies, frameworks, architecture decisions and why.
4. Files and Code Sections: every file created or edited, what changed and why.
5. Errors and Fixes: every error, its root cause, and how it was fixed.
6. Problem Solving: approaches tried, decisions made, trade-offs, open issues.
7. Pending Tasks: tasks requested but not yet completed.
8. Current Work: exactly what was in progress at compaction time.
9. Next Step: the immediate next action to continue. If the primary task is COMPLETE, say so explicitly.";

/// Public entry point for conversation compaction summarization.
pub fn run_summary_pass_public(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    text: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
) -> Result<String, String> {
    run_summary_pass_with_system(
        model, backend, text, chat_template_string, conversation_id,
        COMPACT_SYSTEM_PROMPT,
        COMPACT_SUMMARY_CTX_SIZE,
        COMPACT_SUMMARY_MAX_TOKENS,
    )
}

/// Run a summary pass with a custom system message.
pub fn run_summary_pass_with_system(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    text: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
    system_msg: &str,
    ctx_size: u32,
    max_tokens: usize,
) -> Result<String, String> {
    let formatted_prompt = if let Some(template_str) = chat_template_string {
        use crate::jinja_templates::apply_native_chat_template;
        #[allow(deprecated)]
        use llama_cpp_2::model::Special;
        #[allow(deprecated)]
        let bos = model.token_to_str(model.token_bos(), Special::Tokenize).unwrap_or_else(|_| "<s>".into());
        #[allow(deprecated)]
        let eos = model.token_to_str(model.token_eos(), Special::Tokenize).unwrap_or_else(|_| "</s>".into());
        let messages = vec![
            crate::jinja_templates::ChatMessage { role: "system".into(), content: system_msg.into(), tool_calls: None },
            crate::jinja_templates::ChatMessage { role: "user".into(), content: text.into(), tool_calls: None },
        ];
        apply_native_chat_template(template_str, messages, None, None, true, &bos, &eos, false)
            .unwrap_or_else(|_| format!("SYSTEM:\n{system_msg}\n\nUSER:\n{text}\n\nASSISTANT:\n"))
    } else {
        format!("SYSTEM:\n{system_msg}\n\nUSER:\n{text}\n\nASSISTANT:\n")
    };

    let tokens = model
        .str_to_token(&formatted_prompt, AddBos::Never)
        .map_err(|e| format!("Summary tokenization failed: {e}"))?;

    if tokens.len() + max_tokens > ctx_size as usize {
        return Err(format!("Summary prompt too large: {} tokens", tokens.len()));
    }

    let n_ctx = NonZeroU32::new(ctx_size).unwrap();
    let config = SamplerConfig::default();
    let mut ctx = create_fresh_context(model, backend, n_ctx, true, &config)?;

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
    let prompt_len = tokens.len() as i32;
    let eos_token = model.token_eos();

    for i in 0..max_tokens {
        let next_token = sampler.sample(&ctx, -1);
        if next_token == eos_token { break; }

        #[allow(deprecated)]
        let token_str = model
            .token_to_str(next_token, llama_cpp_2::model::Special::Tokenize)
            .unwrap_or_default();

        summary.push_str(&token_str);

        let token_pos = prompt_len + i as i32;
        batch.clear();
        batch.add(next_token, token_pos, &[0], true)
            .map_err(|e| format!("Summary gen batch add failed: {e}"))?;
        ctx.decode(&mut batch)
            .map_err(|e| format!("Summary gen decode failed: {e}"))?;
    }

    drop(ctx);
    let result = summary.trim().to_string();
    log_info!(conversation_id, "📦 Conversation summary: {} input chars → {} output chars", text.len(), result.len());
    Ok(result)
}

/// Run a summary pass reusing an existing context (clears memory between uses).
pub fn run_summary_reusing_ctx(
    model: &llama_cpp_2::model::LlamaModel,
    ctx: &mut llama_cpp_2::context::LlamaContext<'_>,
    text: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
    ctx_size: usize,
    max_tokens: usize,
) -> Result<String, String> {
    run_summary_reusing_ctx_with_system(
        model, ctx, text, chat_template_string, conversation_id,
        COMPACT_SYSTEM_PROMPT,
        ctx_size,
        max_tokens,
    )
}

/// Run a summary pass reusing an existing context with a CUSTOM system prompt.
pub fn run_summary_reusing_ctx_with_system(
    model: &llama_cpp_2::model::LlamaModel,
    ctx: &mut llama_cpp_2::context::LlamaContext<'_>,
    text: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
    system_msg: &str,
    ctx_size: usize,
    max_tokens: usize,
) -> Result<String, String> {
    let formatted_prompt = if let Some(template_str) = chat_template_string {
        use crate::jinja_templates::apply_native_chat_template;
        #[allow(deprecated)]
        use llama_cpp_2::model::Special;
        #[allow(deprecated)]
        let bos = model.token_to_str(model.token_bos(), Special::Tokenize).unwrap_or_else(|_| "<s>".into());
        #[allow(deprecated)]
        let eos = model.token_to_str(model.token_eos(), Special::Tokenize).unwrap_or_else(|_| "</s>".into());
        let messages = vec![
            crate::jinja_templates::ChatMessage { role: "system".into(), content: system_msg.into(), tool_calls: None },
            crate::jinja_templates::ChatMessage { role: "user".into(), content: text.into(), tool_calls: None },
        ];
        apply_native_chat_template(template_str, messages, None, None, true, &bos, &eos, false)
            .unwrap_or_else(|_| format!("SYSTEM:\n{system_msg}\n\nUSER:\n{text}\n\nASSISTANT:\n"))
    } else {
        format!("SYSTEM:\n{system_msg}\n\nUSER:\n{text}\n\nASSISTANT:\n")
    };

    let tokens = model
        .str_to_token(&formatted_prompt, AddBos::Never)
        .map_err(|e| format!("Summary tokenization failed: {e}"))?;

    if tokens.len() + max_tokens > ctx_size {
        return Err(format!("Summary prompt too large: {} tokens", tokens.len()));
    }

    ctx.clear_kv_cache();

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
    let prompt_len = tokens.len() as i32;
    let eos_token = model.token_eos();

    for i in 0..max_tokens {
        let next_token = sampler.sample(ctx, -1);
        if next_token == eos_token { break; }

        #[allow(deprecated)]
        let token_str = model
            .token_to_str(next_token, llama_cpp_2::model::Special::Tokenize)
            .unwrap_or_default();

        summary.push_str(&token_str);

        let token_pos = prompt_len + i as i32;
        batch.clear();
        batch.add(next_token, token_pos, &[0], true)
            .map_err(|e| format!("Summary gen batch add failed: {e}"))?;
        ctx.decode(&mut batch)
            .map_err(|e| format!("Summary gen decode failed: {e}"))?;
    }

    let result = summary.trim().to_string();
    log_info!(conversation_id, "📦 Summary pass (reused ctx): {} input → {} output chars", text.len(), result.len());
    Ok(result)
}

/// Summarize tool output using chunked map-reduce if needed.
pub(crate) fn summarize_tool_output(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    output: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
) -> Result<String, String> {
    crate::tool_output::summarize_tool_output_with_prompt(model, backend, output, chat_template_string, conversation_id, None)
}

pub fn summarize_tool_output_with_prompt(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    output: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
    custom_prompt: Option<&str>,
) -> Result<String, String> {
    let original_len = output.len();

    let estimated_tokens = model
        .str_to_token(output, llama_cpp_2::model::AddBos::Never)
        .map(|t| t.len())
        .unwrap_or(output.len() / 4);
    let single_pass_limit = (SUMMARY_CTX_SIZE as usize) - SUMMARY_MAX_TOKENS - 200;

    let summary = if estimated_tokens < single_pass_limit {
        if let Some(prompt) = custom_prompt {
            run_summary_pass_with_system(model, backend, output, chat_template_string, conversation_id, prompt, SUMMARY_CTX_SIZE, SUMMARY_MAX_TOKENS)?
        } else {
            run_summary_pass(model, backend, output, chat_template_string, conversation_id)?
        }
    } else {
        let mut chunk_texts = Vec::new();
        let mut pos = 0;
        while pos < output.len() {
            let mut end = std::cmp::min(pos + SUMMARY_CHUNK_CHARS, output.len());
            while end < output.len() && !output.is_char_boundary(end) {
                end += 1;
            }
            chunk_texts.push(&output[pos..end]);
            pos = end;
        }

        log_info!(conversation_id, "📝 Chunked summarization: {} chars → {} chunks", original_len, chunk_texts.len());

        let mut chunk_summaries = Vec::new();
        for (i, chunk) in chunk_texts.iter().enumerate() {
            match if let Some(prompt) = custom_prompt {
                run_summary_pass_with_system(model, backend, chunk, chat_template_string, conversation_id, prompt, SUMMARY_CTX_SIZE, SUMMARY_MAX_TOKENS)
            } else {
                run_summary_pass(model, backend, chunk, chat_template_string, conversation_id)
            } {
                Ok(s) => {
                    log_info!(conversation_id, "📝 Chunk {}/{}: {} → {} chars", i + 1, chunk_texts.len(), chunk.len(), s.len());
                    chunk_summaries.push(s);
                }
                Err(e) => {
                    log_warn!(conversation_id, "📝 Chunk {}/{} failed: {}", i + 1, chunk_texts.len(), e);
                    chunk_summaries.push(chunk.chars().take(200).collect::<String>() + "...");
                }
            }
        }

        let combined = chunk_summaries.join("\n");

        if combined.len() > SUMMARIZE_THRESHOLD {
            log_info!(conversation_id, "📝 Final reduction pass: {} chars", combined.len());
            run_summary_pass(model, backend, &combined, chat_template_string, conversation_id)
                .unwrap_or(combined)
        } else {
            combined
        }
    };

    Ok(summary)
}
