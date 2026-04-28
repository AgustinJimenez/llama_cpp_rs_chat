//! Sub-agent checks: tool result validation, task completion, title generation.

use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::{AddBos};
use llama_cpp_2::sampling::LlamaSampler;
use std::num::NonZeroU32;

use super::context_eval::create_fresh_context;
use llama_chat_types::{SamplerConfig, SharedLlamaState};
pub fn quick_tool_result_check(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    chat_template_string: Option<&str>,
    _conversation_id: &str,
    tool_name: &str,
    output: &str,
) -> bool {
    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::model::AddBos;
    use std::num::NonZeroU32;
    use llama_chat_types::SamplerConfig;

    let prompt_text = format!(
        "Tool: {tool_name}\nOutput: {output}\n\n\
         Did this tool call produce useful results? Answer YES if the output contains \
         ANY meaningful content (even if mixed with navigation or ads). \
         Answer NO ONLY if the output is entirely an error page, 404, empty, \
         paywall with no content, or login wall with no content."
    );

    let formatted = if let Some(template_str) = chat_template_string {
        use super::jinja_templates::{apply_native_chat_template, ChatMessage};
        #[allow(deprecated)]
        use llama_cpp_2::model::Special;
        #[allow(deprecated)]
        let bos = model.token_to_str(model.token_bos(), Special::Tokenize).unwrap_or_else(|_| "<s>".into());
        #[allow(deprecated)]
        let eos = model.token_to_str(model.token_eos(), Special::Tokenize).unwrap_or_else(|_| "</s>".into());
        let messages = vec![
            ChatMessage { role: "system".into(), content: "Answer YES or NO only.".into(), tool_calls: None },
            ChatMessage { role: "user".into(), content: prompt_text.clone().into(), tool_calls: None },
        ];
        apply_native_chat_template(template_str, messages, None, None, true, &bos, &eos)
            .unwrap_or_else(|_| format!("SYSTEM:\nAnswer YES or NO only.\n\nUSER:\n{prompt_text}\n\nASSISTANT:\n"))
    } else {
        format!("SYSTEM:\nAnswer YES or NO only.\n\nUSER:\n{prompt_text}\n\nASSISTANT:\n")
    };

    let tokens = match model.str_to_token(&formatted, AddBos::Never) {
        Ok(t) => t,
        Err(_) => return true,
    };

    let n_ctx = NonZeroU32::new(1024).unwrap();
    let config = SamplerConfig::default();
    let mut ctx = match create_fresh_context(model, backend, n_ctx, false, &config) {
        Ok(c) => c,
        Err(_) => return true,
    };

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
            if batch.add(token, pos, &[0], is_last).is_err() { return true; }
        }
        if ctx.decode(&mut batch).is_err() { return true; }
    }

    let mut sampler = LlamaSampler::chain_simple(vec![
        LlamaSampler::temp(0.1),
        LlamaSampler::dist(42),
    ]);

    let mut response = String::new();
    let mut token_pos = tokens.len() as i32;
    let eos_token = model.token_eos();
    for _ in 0..5 {
        let next_token = sampler.sample(&ctx, -1);
        if next_token == eos_token { break; }
        #[allow(deprecated)]
        let token_str = model
            .token_to_str(next_token, llama_cpp_2::model::Special::Tokenize)
            .unwrap_or_default();
        response.push_str(&token_str);
        batch.clear();
        if batch.add(next_token, token_pos, &[0], true).is_err() { break; }
        if ctx.decode(&mut batch).is_err() { break; }
        token_pos += 1;
    }
    drop(ctx);

    let answer = response.trim().to_uppercase();
    let is_success = answer.starts_with("YES") || answer.starts_with('Y');
    eprintln!("[TOOL_RESULT_CHECK] {tool_name}: '{answer}' → {}", if is_success { "OK" } else { "ERROR" });
    is_success
}

/// Quick Y/N check: ask the model if the current task is complete.
/// Uses a tiny context (1024 tokens) for fast inference (~50ms).
/// Returns true if complete, false if the model thinks more work is needed.
pub fn quick_task_completion_check(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    chat_template_string: Option<&str>,
    conversation_id: &str,
    response_tail: &str,
) -> bool {
    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::model::AddBos;
    use std::num::NonZeroU32;
    use llama_chat_types::SamplerConfig;

    let prompt_text = format!(
        "{response_tail}\n\n\
         ---\n\
         Is the ENTIRE user request fulfilled? Rules:\n\
         - If the user asked for multiple items (e.g., 'top 5', 'each article') and only some were done → NO\n\
         - If the response ends with tool output but no final summary/answer to the user → NO\n\
         - If the assistant said 'Now let me...' or 'Starting with...' → NO\n\
         - Only answer YES if the task is 100% complete with a final answer.\n\
         Answer YES or NO."
    );

    let formatted = if let Some(template_str) = chat_template_string {
        use super::jinja_templates::{apply_native_chat_template, ChatMessage};
        #[allow(deprecated)]
        use llama_cpp_2::model::Special;
        #[allow(deprecated)]
        let bos = model.token_to_str(model.token_bos(), Special::Tokenize).unwrap_or_else(|_| "<s>".into());
        #[allow(deprecated)]
        let eos = model.token_to_str(model.token_eos(), Special::Tokenize).unwrap_or_else(|_| "</s>".into());
        let messages = vec![
            ChatMessage { role: "system".into(), content: "Answer YES or NO only.".into(), tool_calls: None },
            ChatMessage { role: "user".into(), content: prompt_text.clone().into(), tool_calls: None },
        ];
        apply_native_chat_template(template_str, messages, None, None, true, &bos, &eos)
            .unwrap_or_else(|_| format!("SYSTEM:\nAnswer YES or NO only.\n\nUSER:\n{prompt_text}\n\nASSISTANT:\n"))
    } else {
        format!("SYSTEM:\nAnswer YES or NO only.\n\nUSER:\n{prompt_text}\n\nASSISTANT:\n")
    };

    let tokens = match model.str_to_token(&formatted, AddBos::Never) {
        Ok(t) => t,
        Err(_) => return true, // Assume complete on error
    };

    let n_ctx = NonZeroU32::new(1024).unwrap();
    let config = SamplerConfig::default();
    let mut ctx = match create_fresh_context(model, backend, n_ctx, false, &config) {
        Ok(c) => c,
        Err(_) => return true,
    };

    // Eval prompt
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
            if batch.add(token, pos, &[0], is_last).is_err() { return true; }
        }
        if ctx.decode(&mut batch).is_err() { return true; }
    }

    // Sample just 1-3 tokens
    let mut sampler = LlamaSampler::chain_simple(vec![
        LlamaSampler::temp(0.1),
        LlamaSampler::dist(42),
    ]);

    let mut response = String::new();
    let mut token_pos = tokens.len() as i32;
    let eos_token = model.token_eos();

    for _ in 0..5 {
        let next_token = sampler.sample(&ctx, -1);
        if next_token == eos_token { break; }

        #[allow(deprecated)]
        let token_str = model
            .token_to_str(next_token, llama_cpp_2::model::Special::Tokenize)
            .unwrap_or_default();
        response.push_str(&token_str);

        batch.clear();
        if batch.add(next_token, token_pos, &[0], true).is_err() { break; }
        if ctx.decode(&mut batch).is_err() { break; }
        token_pos += 1;
    }

    drop(ctx);

    let answer = response.trim().to_uppercase();
    let is_complete = answer.starts_with("YES") || answer.starts_with('Y');
    eprintln!("[TASK_CHECK] Tool calls made, completion check: '{}' → {}", answer, if is_complete { "complete" } else { "INCOMPLETE" });
    log_info!(conversation_id, "🔍 Task completion check: '{}' → {}", answer, if is_complete { "complete" } else { "incomplete, will auto-continue" });

    is_complete
}

/// Generate response from LLaMA model with streaming support.
///
/// Handles token generation, stop conditions, command execution, and conversation logging.
/// Supports multiple sampling strategies and automatic context size validation.

pub fn generate_title_text(
    llama_state: &SharedLlamaState,
    prompt: &str,
) -> Result<String, String> {
    let mut state_guard = llama_state
        .lock()
        .map_err(|_| "Failed to lock LLaMA state")?;
    let state = state_guard.as_mut().ok_or("No model loaded")?;
    let model = state.model.as_ref().ok_or("No model loaded")?;

    // Format a minimal [system, user] prompt using the model's chat template
    let system_msg = "Generate a concise title (3-6 words) for this conversation. Respond with ONLY the title, nothing else.";

    #[allow(deprecated)]
    use llama_cpp_2::model::Special;
    #[allow(deprecated)]
    let bos_text = model
        .token_to_str(model.token_bos(), Special::Tokenize)
        .unwrap_or_else(|_| "<s>".to_string());
    #[allow(deprecated)]
    let eos_text = model
        .token_to_str(model.token_eos(), Special::Tokenize)
        .unwrap_or_else(|_| "</s>".to_string());

    let formatted_prompt = if let Some(ref template_str) = state.chat_template_string {
        // Use Jinja template (no tools, no behavioral prompt)
        use super::jinja_templates::{apply_native_chat_template, ChatMessage};
        let messages = vec![
            ChatMessage { role: "system".into(), content: system_msg.into(), tool_calls: None },
            ChatMessage { role: "user".into(), content: prompt.into(), tool_calls: None },
        ];
        apply_native_chat_template(template_str, messages, None, None, true, &bos_text, &eos_text)
            .unwrap_or_else(|_| format!("SYSTEM:\n{system_msg}\n\nUSER:\n{prompt}\n\nASSISTANT:\n"))
    } else {
        // Simple fallback format
        format!("SYSTEM:\n{system_msg}\n\nUSER:\n{prompt}\n\nASSISTANT:\n")
    };

    // Tokenize
    let tokens = model
        .str_to_token(&formatted_prompt, AddBos::Never)
        .map_err(|e| format!("Title tokenization failed: {e}"))?;

    eprintln!("[WORKER] Title generation: {} prompt tokens", tokens.len());

    // Create a temporary context (small size, doesn't touch inference_cache)
    let title_ctx_size = 2048u32;
    let n_ctx = NonZeroU32::new(title_ctx_size).unwrap();
    let offload_kqv = state.gpu_layers.unwrap_or(0) > 0;
    let config = SamplerConfig::default();
    let mut ctx = create_fresh_context(model, &state.backend, n_ctx, offload_kqv, &config)?;

    // Evaluate prompt tokens in batches
    let batch_cap = 512usize;
    let n_chunks = tokens.len().div_ceil(batch_cap);
    let mut batch = LlamaBatch::new(batch_cap, 1);
    for chunk_idx in 0..n_chunks {
        let start = chunk_idx * batch_cap;
        let end = std::cmp::min(start + batch_cap, tokens.len());
        batch.clear();
        for (offset, &token) in tokens[start..end].iter().enumerate() {
            let pos = (start + offset) as i32;
            let is_last = start + offset == tokens.len() - 1;
            batch.add(token, pos, &[0], is_last)
                .map_err(|e| format!("Title batch add failed: {e}"))?;
        }
        ctx.decode(&mut batch)
            .map_err(|e| format!("Title prompt decode failed: {e}"))?;
    }

    // Create a simple sampler: temp(0.7) + dist
    let mut sampler = LlamaSampler::chain_simple(vec![
        LlamaSampler::temp(0.7),
        LlamaSampler::dist(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u32)
                .unwrap_or(42),
        ),
    ]);

    // Generate up to 30 tokens
    let mut title = String::new();
    let mut token_pos = tokens.len() as i32;
    let eos_token = model.token_eos();

    for _ in 0..30 {
        let next_token = sampler.sample(&ctx, -1);
        if next_token == eos_token {
            break;
        }

        #[allow(deprecated)]
        let token_str = model
            .token_to_str(next_token, Special::Tokenize)
            .unwrap_or_default();

        // Stop on newlines (title should be single line)
        if token_str.contains('\n') {
            break;
        }

        title.push_str(&token_str);

        batch.clear();
        batch.add(next_token, token_pos, &[0], true)
            .map_err(|e| format!("Title gen batch add failed: {e}"))?;
        ctx.decode(&mut batch)
            .map_err(|e| format!("Title gen decode failed: {e}"))?;
        token_pos += 1;
    }

    // Drop the temporary context (inference_cache untouched)
    drop(ctx);
    drop(state_guard);

    let result = title.trim().to_string();
    eprintln!("[WORKER] Title generated: {:?}", result);
    Ok(result)
}
