use llama_cpp_2::{llama_batch::LlamaBatch, model::AddBos};
use llama_cpp_2::sampling::LlamaSampler;
use std::num::NonZeroU32;

use super::generation::create_fresh_context;
use llama_chat_types::*;
// --- Tool output summarization via LLM sub-agent ---

/// Minimum output size (chars) to trigger LLM summarization.
/// Set to 0 to always summarize (useful for testing).
pub(crate) const SUMMARIZE_THRESHOLD: usize = 1500;
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
    // offload_kqv=true: summary context uses GPU alongside the main context.
    // The 4K KV cache uses ~50MB VRAM — trivial next to the main context.
    // Both contexts share the same model weights (read-only).
    let mut ctx = create_fresh_context(model, backend, n_ctx, true, &config)?;

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
    use crate::generation::create_fresh_context;
    use llama_chat_types::SamplerConfig;

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
    run_summary_reusing_ctx_with_system(model, ctx, text, chat_template_string, conversation_id, system_msg)
}

/// Run a summary pass reusing an existing context with a CUSTOM system prompt.
/// Used for both conversation compaction and tool output summarization.
pub fn run_summary_reusing_ctx_with_system(
    model: &llama_cpp_2::model::LlamaModel,
    ctx: &mut llama_cpp_2::context::LlamaContext<'_>,
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

    let tokens = model
        .str_to_token(&formatted_prompt, AddBos::Never)
        .map_err(|e| format!("Summary tokenization failed: {e}"))?;

    if tokens.len() + SUMMARY_MAX_TOKENS > SUMMARY_CTX_SIZE as usize {
        return Err(format!("Summary prompt too large: {} tokens", tokens.len()));
    }

    // Clear memory to reuse context for a fresh prompt
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
pub(crate) fn summarize_tool_output(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    output: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
) -> Result<String, String> {
    summarize_tool_output_with_prompt(model, backend, output, chat_template_string, conversation_id, None)
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

    // Estimate if output fits in a single pass (use tokenizer if available, else ~4 chars per token)
    let estimated_tokens = model
        .str_to_token(output, llama_cpp_2::model::AddBos::Never)
        .map(|t| t.len())
        .unwrap_or(output.len() / 4);
    let single_pass_limit = (SUMMARY_CTX_SIZE as usize) - SUMMARY_MAX_TOKENS - 200; // 200 tokens for prompt overhead

    let summary = if estimated_tokens < single_pass_limit {
        // Single pass — output fits in one context
        if let Some(prompt) = custom_prompt {
            run_summary_pass_with_system(model, backend, output, chat_template_string, conversation_id, prompt)?
        } else {
            run_summary_pass(model, backend, output, chat_template_string, conversation_id)?
        }
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
            match if let Some(prompt) = custom_prompt {
                run_summary_pass_with_system(model, backend, chunk, chat_template_string, conversation_id, prompt)
            } else {
                run_summary_pass(model, backend, chunk, chat_template_string, conversation_id)
            } {
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

    Ok(summary)
}

/// Generate a compact one-line summary of a tool execution result.
/// Used for logging and for prepending to truncated tool output so the model
/// always sees a quick status line even when output is large.
pub fn tool_use_one_liner(tool_name: &str, args_hint: &str, output: &str, duration_ms: u64) -> String {
    let status = if output.contains("Error") || output.contains("error:") || output.contains("FAILED") {
        "FAILED"
    } else {
        "OK"
    };

    // Extract key info based on tool type
    let detail = match tool_name {
        "execute_command" => {
            if let Some(line) = output.lines().rev().find(|l| l.contains("exit code")) {
                line.trim().to_string()
            } else {
                format!("{} chars output", output.len())
            }
        }
        "write_file" => {
            if let Some(bytes) = output.split("wrote ").nth(1).and_then(|s| s.split(' ').next()) {
                format!("wrote {}", bytes)
            } else {
                output.lines().next().unwrap_or("done").to_string()
            }
        }
        "read_file" => {
            let lines = output.lines().count();
            format!("{} lines", lines)
        }
        "web_search" => {
            let results = output.matches("URL:").count().max(output.matches("http").count().min(10));
            format!("{} results", results)
        }
        "web_fetch" => {
            format!("{} chars fetched", output.len())
        }
        _ => {
            let first_line = output.lines().next().unwrap_or("done");
            if first_line.chars().count() > 80 {
                let truncated: String = first_line.chars().take(77).collect();
                format!("{truncated}...")
            } else {
                first_line.to_string()
            }
        }
    };

    let args_part = if args_hint.is_empty() {
        String::new()
    } else {
        let hint = if args_hint.chars().count() > 60 {
            let truncated: String = args_hint.chars().take(57).collect();
            format!("{truncated}...")
        } else {
            args_hint.to_string()
        };
        format!(" {}", hint)
    };

    if duration_ms > 0 {
        format!("[{}{} -> {} {} ({}ms)]", tool_name, args_part, status, detail, duration_ms)
    } else {
        format!("[{}{} -> {} {}]", tool_name, args_part, status, detail)
    }
}

/// Threshold above which tool output gets smart-truncated before context injection.
/// Defined in token units — chars threshold derived as tokens * 4.
const TOOL_OUTPUT_TOKEN_THRESHOLD: usize = 2000;
const TOOL_OUTPUT_TRUNCATION_THRESHOLD: usize = TOOL_OUTPUT_TOKEN_THRESHOLD * 4; // ~8000 chars

/// Smart-truncate large tool output, preserving start and end.
/// Tools like write_file/edit_file produce small output and should NOT be truncated.
/// Returns the original output if it's small enough.
pub fn maybe_truncate_tool_output(output: &str, tool_name: &str, conversation_id: &str) -> String {
    // Quick char-based check (tokens <= chars, so if chars fit, tokens definitely fit)
    if output.len() <= TOOL_OUTPUT_TRUNCATION_THRESHOLD {
        return output.to_string();
    }

    // Skip truncation for tools that produce small, important output
    match tool_name {
        "write_file" | "edit_file" | "read_file" => return output.to_string(),
        _ => {}
    }

    llama_chat_db::event_log::log_event(
        conversation_id, "tool_truncate",
        &format!("{}: truncated {} -> {} chars", tool_name, output.len(), TOOL_OUTPUT_TRUNCATION_THRESHOLD),
    );
    log_info!(
        conversation_id,
        "✂️ Truncating {} output: {} -> {} chars",
        tool_name, output.len(), TOOL_OUTPUT_TRUNCATION_THRESHOLD
    );

    let one_liner = tool_use_one_liner(tool_name, "", output, 0);
    let mut head = (TOOL_OUTPUT_TRUNCATION_THRESHOLD * 3 / 4).min(output.len()); // ~6000 chars from start
    let mut tail_start = output.len().saturating_sub(TOOL_OUTPUT_TRUNCATION_THRESHOLD / 4); // ~2000 chars from end
    // Ensure we slice on char boundaries
    while head > 0 && !output.is_char_boundary(head) { head -= 1; }
    while tail_start < output.len() && !output.is_char_boundary(tail_start) { tail_start += 1; }
    let truncated = output.len().saturating_sub(head).saturating_sub(output.len() - tail_start);
    format!(
        "{}\n{}\n\n[...{} chars truncated — {} total. Key info may be at the end.]\n\n{}",
        one_liner,
        &output[..head],
        truncated,
        output.len(),
        &output[tail_start..]
    )
}

/// Summarize large tool output using recursive map-reduce (sub-agent approach).
/// Handles arbitrarily large outputs by chunking, summarizing each chunk,
/// combining summaries, and recursing if needed — same pattern as conversation compaction.
/// Falls back to truncation if summarization fails.
pub fn maybe_summarize_tool_output(
    output: &str,
    tool_name: &str,
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    chat_template_string: Option<&str>,
    conversation_id: &str,
) -> String {
    const PASS_THROUGH_THRESHOLD: usize = 8000;
    const SINGLE_PASS_LIMIT: usize = 10000;
    const MAP_REDUCE_CTX: u32 = 4096;

    if output.len() <= PASS_THROUGH_THRESHOLD {
        return output.to_string();
    }

    // Skip summarization for tools where raw output is important
    let lower_name = tool_name.to_lowercase();
    if lower_name.contains("read_file") || lower_name.contains("write_file") || lower_name.contains("edit_file")
        || lower_name.contains("browser_get_html") || lower_name.contains("browser_eval")
    {
        return maybe_truncate_tool_output(output, tool_name, conversation_id);
    }

    // Tool-specific summarization instructions
    let extra_instructions = if lower_name.contains("browser_get_links") || lower_name.contains("browser_get_html") {
        "\nCRITICAL: Preserve ALL URLs/href values and their associated text. \
         The user needs the actual links to navigate. Never omit or paraphrase URLs."
    } else if lower_name.contains("browser_get_text") || lower_name.contains("web_fetch") {
        "\nPreserve key facts, names, dates, and quotes from the page content. \
         Keep article structure (headings, main points)."
    } else if lower_name.contains("browser_eval") {
        "\nPreserve the complete data structure (JSON arrays, objects). \
         Do not paraphrase structured data — keep it verbatim if possible."
    } else {
        ""
    };

    let system_prompt = format!(
        "Summarize this {} tool output concisely. Extract ONLY:\n\
         - Key results and status\n\
         - Error messages with file paths and line numbers\n\
         - Important warnings\n\
         - Actionable information\n\n\
         Remove verbose logs, progress bars, repeated output, boilerplate.\n\
         Keep under 500 words.{extra_instructions}",
        tool_name
    );

    log_info!(conversation_id, "📝 [TOOL_SUMMARY] Summarizing {} output: {} chars", tool_name, output.len());
    llama_chat_db::event_log::log_event(conversation_id, "tool_summary",
        &format!("{}: {} chars -> summarizing", tool_name, output.len()));

    // Small enough for a single summary pass (no map-reduce needed)
    if output.len() <= SINGLE_PASS_LIMIT {
        match run_summary_pass_with_system(
            model, backend, output, chat_template_string, conversation_id, &system_prompt,
        ) {
            Ok(summary) => {
                log_info!(conversation_id, "📝 [TOOL_SUMMARY] Single pass: {} -> {} chars", output.len(), summary.len());
                llama_chat_db::event_log::log_event(conversation_id, "tool_summary",
                    &format!("{}: {} -> {} chars (single)", tool_name, output.len(), summary.len()));
                return format!("[Summarized {} output: {} -> {} chars]\n{}", tool_name, output.len(), summary.len(), summary);
            }
            Err(e) => {
                log_warn!(conversation_id, "[TOOL_SUMMARY] Single pass failed: {}, falling back to truncation", e);
                return maybe_truncate_tool_output(output, tool_name, conversation_id);
            }
        }
    }

    // Large output: map-reduce with a reusable context (avoids CUDA memory fragmentation)
    eprintln!("[TOOL_SUMMARY] Map-reduce summarizing {} output: {} chars", tool_name, output.len());

    let n_ctx = NonZeroU32::new(MAP_REDUCE_CTX).unwrap();
    let config = SamplerConfig::default();
    let mut ctx = match create_fresh_context(model, backend, n_ctx, true, &config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[TOOL_SUMMARY] Failed to create summary context: {}", e);
            return maybe_truncate_tool_output(output, tool_name, conversation_id);
        }
    };

    match map_reduce_summarize_tool_output(model, &mut ctx, output, chat_template_string, conversation_id, &system_prompt) {
        Ok(summary) => {
            log_info!(conversation_id, "📝 [TOOL_SUMMARY] Map-reduce: {} -> {} chars", output.len(), summary.len());
            llama_chat_db::event_log::log_event(conversation_id, "tool_summary",
                &format!("{}: {} -> {} chars (map-reduce)", tool_name, output.len(), summary.len()));
            format!("[Summarized {} output: {} -> {} chars]\n{}", tool_name, output.len(), summary.len(), summary)
        }
        Err(e) => {
            log_warn!(conversation_id, "[TOOL_SUMMARY] Map-reduce failed: {}, falling back to truncation", e);
            maybe_truncate_tool_output(output, tool_name, conversation_id)
        }
    }
}

/// Recursive map-reduce summarization for tool output.
/// Splits text into chunks, summarizes each with a reusable context,
/// combines the summaries, and recurses if the combined result is still too large.
fn map_reduce_summarize_tool_output(
    model: &llama_cpp_2::model::LlamaModel,
    ctx: &mut llama_cpp_2::context::LlamaContext<'_>,
    text: &str,
    chat_template_string: Option<&str>,
    conversation_id: &str,
    system_prompt: &str,
) -> Result<String, String> {
    const CHUNK_SIZE: usize = 10000;

    // === MAP PHASE: split into chunks and summarize each ===
    let mut summaries = Vec::new();
    let mut pos = 0;
    let total_chunks = (text.len() + CHUNK_SIZE - 1) / CHUNK_SIZE;
    let mut chunk_num = 0;

    while pos < text.len() {
        let end = (pos + CHUNK_SIZE).min(text.len());
        // Ensure we land on a UTF-8 char boundary
        let end = (pos..=end).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(end);
        let chunk = &text[pos..end];
        chunk_num += 1;

        eprintln!("[TOOL_SUMMARY] Map chunk {}/{} ({} chars)", chunk_num, total_chunks, chunk.len());

        match run_summary_reusing_ctx_with_system(model, ctx, chunk, chat_template_string, conversation_id, system_prompt) {
            Ok(summary) => {
                eprintln!("[TOOL_SUMMARY] Chunk {} -> {} chars", chunk_num, summary.len());
                summaries.push(summary);
            }
            Err(e) => {
                eprintln!("[TOOL_SUMMARY] Chunk {} failed: {}, using truncated fallback", chunk_num, e);
                summaries.push(chunk.chars().take(200).collect::<String>() + "...");
            }
        }

        pos = end;
    }

    // === REDUCE PHASE: combine summaries ===
    let combined = summaries.join("\n\n");
    eprintln!("[TOOL_SUMMARY] Reduce: {} summaries ({} chars)", summaries.len(), combined.len());

    if combined.len() <= CHUNK_SIZE {
        // Final summary pass on the combined chunk summaries
        run_summary_reusing_ctx_with_system(model, ctx, &combined, chat_template_string, conversation_id, system_prompt)
    } else {
        // Still too large — recurse
        map_reduce_summarize_tool_output(model, ctx, &combined, chat_template_string, conversation_id, system_prompt)
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
pub(crate) fn wrap_output_for_model(output_block: &str, template_type: Option<&str>) -> String {
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
