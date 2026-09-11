//! Sub-agent checks: tool result validation, task completion, title generation.

use llama_chat_db::event_log::log_event;

/// Result from `check_eos_continuation`.
pub struct EosContinuationResult {
    /// True → task is complete, accept EOS. False → inject continuation tokens.
    pub is_complete: bool,
    /// String form of continuation tokens (populated when is_complete = false).
    pub continuation_text: String,
    /// Token IDs to inject into the main KV cache (populated when is_complete = false).
    pub continuation_tokens: Vec<llama_cpp_2::token::LlamaToken>,
}

/// Ask the model whether the response is complete.
/// - If complete: the model replies "Y" → returns `is_complete = true`.
/// - If incomplete: the model continues writing from where it left off →
///   those tokens are returned as `continuation_tokens` for seamless injection.
///
/// Runs on a disposable 1 K-token context (~50 ms). The main KV cache is untouched.
///
/// **Currently has no callers.** Before wiring it up, note that its `i == 0` first-token
/// verdict below carries the same defect that `inline_eos_probe` had: a thinking model
/// replies with its thinking-close tag first, so the literal first token is never the
/// bare verdict word. Use [`probe_verdict_word`] on the accumulated reply instead.
/// See AGENT_TASKS/002.
pub fn check_eos_continuation(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    chat_template_string: Option<&str>,
    conversation_id: &str,
    user_message: &str,
    response_tail: &str,
    max_continuation_tokens: usize,
) -> EosContinuationResult {
    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::model::AddBos;
    use std::num::NonZeroU32;
    use llama_chat_types::SamplerConfig;

    let complete_result = EosContinuationResult { is_complete: true, continuation_text: String::new(), continuation_tokens: vec![] };

    let system_msg = "You are a completion judge for an agentic AI that uses tools.\n\
        Rules — reply Y ONLY if ALL of these are true:\n\
        - The response fully answers the USER REQUEST with verified results.\n\
        - The response does NOT end with a future-tense intention like \
          'Let me...', 'Now let me...', 'I\\'ll...', 'I will...', 'Starting with...', \
          'First, let me...', 'Next, I\\'ll...' without a completed action following it.\n\
        - The response does NOT end mid-sentence, mid-code-block, or mid-list.\n\
        Otherwise, continue writing the ASSISTANT RESPONSE from exactly where it ended. \
        Output ONLY the continuation — no preamble, no explanation, no 'Y'.";

    let user_content = format!(
        "USER REQUEST: {user_message}\n\nASSISTANT RESPONSE:\n{response_tail}"
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
            ChatMessage { role: "system".into(), content: system_msg.into(), tool_calls: None },
            ChatMessage { role: "user".into(), content: user_content.clone(), tool_calls: None },
        ];
        apply_native_chat_template(template_str, messages, None, None, true, &bos, &eos, false)
            .unwrap_or_else(|_| format!("SYSTEM:\n{system_msg}\n\nUSER:\n{user_content}\n\nASSISTANT:\n"))
    } else {
        format!("SYSTEM:\n{system_msg}\n\nUSER:\n{user_content}\n\nASSISTANT:\n")
    };

    let tokens = match model.str_to_token(&formatted, AddBos::Never) {
        Ok(t) => t,
        Err(_) => return complete_result,
    };

    let n_ctx = NonZeroU32::new(1024).unwrap();
    let config = SamplerConfig::default();
    let mut ctx = match create_fresh_context(model, backend, n_ctx, false, &config) {
        Ok(c) => c,
        Err(_) => return complete_result,
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
            if batch.add(token, pos, &[0], is_last).is_err() { return complete_result; }
        }
        if ctx.decode(&mut batch).is_err() { return complete_result; }
    }

    let mut sampler = LlamaSampler::chain_simple(vec![
        LlamaSampler::temp(0.1),
        LlamaSampler::dist(42),
    ]);

    let eos_token = model.token_eos();
    let mut continuation_text = String::new();
    let mut continuation_tokens: Vec<llama_cpp_2::token::LlamaToken> = Vec::new();
    let prompt_len = tokens.len() as i32;

    for i in 0..=max_continuation_tokens {
        let next_token = sampler.sample(&ctx, -1);
        if next_token == eos_token { break; }

        #[allow(deprecated)]
        let token_str = model
            .token_to_str(next_token, llama_cpp_2::model::Special::Tokenize)
            .unwrap_or_default();

        // First token decides: "Y" (or "YES") → complete; anything else → continuation.
        if i == 0 {
            let first = token_str.trim().to_uppercase();
            if first == "Y" || first.starts_with("YES") {
                drop(ctx);
                eprintln!("[EOS_CHECK] '{first}' → task complete");
                log_info!(conversation_id, "✅ EOS check: task complete (model said '{first}')");
                return complete_result;
            }
        }

        continuation_text.push_str(&token_str);
        continuation_tokens.push(next_token);

        let token_pos = prompt_len + i as i32;
        batch.clear();
        if batch.add(next_token, token_pos, &[0], true).is_err() { break; }
        if ctx.decode(&mut batch).is_err() { break; }
    }

    drop(ctx);

    eprintln!("[EOS_CHECK] incomplete → continuation: {:?}", &continuation_text[..continuation_text.len().min(80)]);
    log_info!(conversation_id, "🔄 EOS check: incomplete — injecting {} continuation tokens", continuation_tokens.len());
    log_event(conversation_id, "eos_intercept", &format!(
        "incomplete, continuation: {:?}",
        &continuation_text[..continuation_text.len().min(60)]
    ));

    EosContinuationResult {
        is_complete: false,
        continuation_text,
        continuation_tokens,
    }
}

use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::{AddBos};
use llama_cpp_2::sampling::LlamaSampler;
use std::num::NonZeroU32;

use super::context_eval::create_fresh_context;
use llama_chat_types::{SamplerConfig, SharedLlamaState};

/// EOS probe text injected into the main context to ask the model if it's done.
const EOS_PROBE_TEXT: &str =
    "\n\n[SELF-CHECK] Are you completely done with the task? Type DONE if yes, or write your next action if not.\n";

/// The first bare word of a probe reply, ignoring any markup around it.
///
/// The verdict must NOT be read from the literal first token. A thinking-capable model
/// answers this probe with its thinking-close tag and whitespace first — observed as
/// `"</think>\n\nDONE"` — so a first-token test reads `</think>`, concludes "not done",
/// and injects the model's private answer into the user-visible response. Every thinking
/// model was affected; see AGENT_TASKS/002.
///
/// Tag spans are dropped generically (anything between `<` and `>`) rather than via the
/// model's configured tag pairs: a verdict is always a bare word, so this needs no
/// per-model knowledge and cannot drift out of sync with `tool_tags.rs`.
///
/// With `require_terminator`, returns `None` unless the word is followed by a delimiter,
/// so a partially-sampled `"DON"` is never mistaken for a complete answer.
fn probe_verdict_word(text: &str, require_terminator: bool) -> Option<String> {
    let mut cleaned = String::new();
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => cleaned.push(c),
            _ => {}
        }
    }

    // Skip leading punctuation as well as whitespace. A reply that opens with a bracket
    // — observed when the model answers by echoing `[SELF-CHECK] …` back at us — otherwise
    // yields an empty word and no verdict at all (AGENT_TASKS/012).
    let trimmed = cleaned.trim_start_matches(|c: char| !c.is_alphanumeric());
    let word: String = trimmed
        .chars()
        .take_while(|c| c.is_alphanumeric())
        .collect();
    if word.is_empty() {
        return None;
    }
    if require_terminator && trimmed.len() == word.len() {
        return None; // nothing after the word yet — it may still be growing
    }
    Some(word.to_uppercase())
}

/// Does this probe reply mean "yes, I'm finished"?
fn verdict_is_complete(word: &str) -> bool {
    word == "DONE" || word == "Y" || word.starts_with("YES")
}

/// Probe the model for task completion using the **main** inference context.
///
/// Unlike [`check_eos_continuation`] (which spins up a disposable 1 K context),
/// this function injects a short probe question directly into the live KV cache
/// at `rollback_pos`, samples the model's reply, then **always** rolls the KV
/// cache back to `rollback_pos` before returning — leaving the main context
/// byte-identical to its pre-call state.
///
/// * If the model's first token is "DONE" / "Y" / "YES" → `is_complete = true`.
/// * Otherwise the sampled continuation tokens are returned so the caller can
///   re-inject them as real response tokens.
///
/// # Errors
/// Any decode / tokenisation failure returns `is_complete = true` after rolling
/// back whatever was already injected.
pub fn inline_eos_probe(
    model: &llama_cpp_2::model::LlamaModel,
    context: &mut llama_cpp_2::context::LlamaContext<'_>,
    gen_token_pos: i32,
    conversation_id: &str,
) -> EosContinuationResult {
    #[allow(deprecated)]
    use llama_cpp_2::model::Special;

    let rollback_pos = gen_token_pos;

    let complete_result = EosContinuationResult {
        is_complete: true,
        continuation_text: String::new(),
        continuation_tokens: vec![],
    };

    // Tokenise the probe text (no BOS — we're mid-sequence).
    let probe_tokens = match model.str_to_token(EOS_PROBE_TEXT, AddBos::Never) {
        Ok(t) => t,
        Err(_) => return complete_result,
    };

    // Use a dedicated batch so we don't disturb the caller's batch state.
    let mut probe_batch = LlamaBatch::new(128, 1);

    // Inject probe tokens into the live KV cache.
    let mut probe_end_pos = rollback_pos;
    let mut injection_ok = true;
    for (i, &tok) in probe_tokens.iter().enumerate() {
        let pos = rollback_pos + i as i32;
        let is_last = i == probe_tokens.len() - 1;
        probe_batch.clear();
        if probe_batch.add(tok, pos, &[0], is_last).is_err() {
            injection_ok = false;
            break;
        }
        if context.decode(&mut probe_batch).is_err() {
            injection_ok = false;
            break;
        }
        probe_end_pos = pos + 1;
    }

    if !injection_ok {
        // Roll back whatever we managed to inject before failing.
        let _ = context.clear_kv_cache_seq(Some(0), Some(rollback_pos as u32), None);
        return complete_result;
    }

    // Sample the model's reply with a low-temperature probe sampler.
    let mut probe_sampler = LlamaSampler::chain_simple(vec![
        LlamaSampler::temp(0.1),
        LlamaSampler::dist(42),
    ]);

    let eos_token = model.token_eos();
    let max_probe_tokens: usize = 20;
    let mut continuation_text = String::new();
    let mut continuation_tokens: Vec<llama_cpp_2::token::LlamaToken> = Vec::new();
    let mut is_done = false;

    for i in 0..=max_probe_tokens {
        let next_token = probe_sampler.sample(context, -1);
        if next_token == eos_token {
            break;
        }

        #[allow(deprecated)]
        let token_str = model
            .token_to_str(next_token, Special::Tokenize)
            .unwrap_or_default();

        continuation_text.push_str(&token_str);
        continuation_tokens.push(next_token);

        // Stop as soon as a complete bare word has emerged — that word is the verdict,
        // whatever markup preceded it. Sampling further would only lengthen a reply we
        // are about to discard (on "done") or re-inject (on "not done").
        if probe_verdict_word(&continuation_text, true).is_some() {
            break;
        }

        // Feed the sampled token back so the model can autoregress.
        let sample_pos = probe_end_pos + i as i32;
        probe_batch.clear();
        if probe_batch.add(next_token, sample_pos, &[0], true).is_err() {
            break;
        }
        if context.decode(&mut probe_batch).is_err() {
            break;
        }
    }

    // Always roll back — remove probe + any sampled tokens from the KV cache.
    let _ = context.clear_kv_cache_seq(Some(0), Some(rollback_pos as u32), None);

    // Read the verdict from the reply as a whole. `require_terminator: false` because the
    // loop may have ended on EOS or the token budget with the word still unterminated.
    if let Some(word) = probe_verdict_word(&continuation_text, false) {
        if verdict_is_complete(&word) {
            is_done = true;
            eprintln!("[EOS_PROBE] '{word}' → task complete (inline probe)");
            log_info!(
                conversation_id,
                "✅ Inline EOS probe: task complete (model said '{word}')"
            );
        }
    }

    if is_done || continuation_tokens.is_empty() {
        return complete_result;
    }

    // The model's reply is a signal, NOT content.
    //
    // It was sampled in a context that contains our hidden probe question, so it is an
    // answer to *that* question — not a continuation of the user-visible response.
    // Returning it verbatim is what leaked `[SELF-CHECK] …` and `</think>\n\nI'm` into the
    // chat (AGENT_TASKS/012): to "hi" the model replied "I'm here and ready to help", which
    // got pasted straight after "Hello! How can I help you today?".
    //
    // So we discard the reply and return a neutral nudge instead. The KV cache has already
    // been rolled back above, so the probe text is gone from the model's context and the
    // main token loop regenerates a real continuation of its own answer. This mirrors what
    // the `force_continue` path in `token_loop.rs` already does.
    let nudge = "\n\n";
    let nudge_tokens: Vec<llama_cpp_2::token::LlamaToken> = match model
        .str_to_token(nudge, AddBos::Never)
    {
        Ok(t) => t,
        // Without a nudge we cannot resume past EOS; accepting EOS is the safe failure.
        Err(_) => return complete_result,
    };

    eprintln!(
        "[EOS_PROBE] incomplete → nudging; discarded reply: {:?}",
        &continuation_text[..continuation_text.len().min(80)]
    );
    log_info!(
        conversation_id,
        "🔄 Inline EOS probe: incomplete — nudging continuation (probe reply discarded)"
    );
    llama_chat_db::event_log::log_event(
        conversation_id,
        "eos_intercept",
        &format!(
            "inline probe incomplete; discarded reply: {:?}",
            &continuation_text[..continuation_text.len().min(60)]
        ),
    );

    EosContinuationResult {
        is_complete: false,
        continuation_text: nudge.to_string(),
        continuation_tokens: nudge_tokens,
    }
}
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
            ChatMessage { role: "user".into(), content: prompt_text.clone(), tool_calls: None },
        ];
        apply_native_chat_template(template_str, messages, None, None, true, &bos, &eos, false)
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
    let prompt_len = tokens.len() as i32;
    let eos_token = model.token_eos();
    for i in 0..5 {
        let next_token = sampler.sample(&ctx, -1);
        if next_token == eos_token { break; }
        #[allow(deprecated)]
        let token_str = model
            .token_to_str(next_token, llama_cpp_2::model::Special::Tokenize)
            .unwrap_or_default();
        response.push_str(&token_str);
        let token_pos = prompt_len + i;
        batch.clear();
        if batch.add(next_token, token_pos, &[0], true).is_err() { break; }
        if ctx.decode(&mut batch).is_err() { break; }
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
            ChatMessage { role: "user".into(), content: prompt_text.clone(), tool_calls: None },
        ];
        apply_native_chat_template(template_str, messages, None, None, true, &bos, &eos, false)
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
    let prompt_len = tokens.len() as i32;
    let eos_token = model.token_eos();

    for i in 0..5 {
        let next_token = sampler.sample(&ctx, -1);
        if next_token == eos_token { break; }

        #[allow(deprecated)]
        let token_str = model
            .token_to_str(next_token, llama_cpp_2::model::Special::Tokenize)
            .unwrap_or_default();
        response.push_str(&token_str);

        let token_pos = prompt_len + i;
        batch.clear();
        if batch.add(next_token, token_pos, &[0], true).is_err() { break; }
        if ctx.decode(&mut batch).is_err() { break; }
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
    let system_msg = "You are a title generator. You output ONLY a thread title. Nothing else.\n\
        \n\
        Generate a brief title that would help the user find this conversation later.\n\
        Your output must be:\n\
        - A single line\n\
        - ≤50 characters\n\
        - No explanations\n\
        \n\
        Rules:\n\
        - Use the same language as the user message you are summarizing\n\
        - Title must be grammatically correct and read naturally — no word salad\n\
        - Never include tool names in the title (e.g. read_file, execute_command, write_file)\n\
        - Focus on the main topic or question the user needs to retrieve\n\
        - Vary your phrasing — avoid repetitive patterns like always starting with \"Analyzing\"\n\
        - When a file is mentioned, focus on WHAT the user wants to do WITH the file, not just that they shared it\n\
        - Keep exact: technical terms, numbers, filenames, HTTP codes\n\
        - Remove filler words: the, this, my, a, an\n\
        - Never assume tech stack\n\
        - NEVER respond to questions, just generate a title\n\
        - NEVER include \"summarizing\" or \"generating\" in the title\n\
        - DO NOT SAY YOU CANNOT GENERATE A TITLE\n\
        - Always output something meaningful, even if the input is minimal\n\
        - If the user message is short or conversational (e.g. \"hello\", \"what's up\"):\n\
          create a title that reflects the tone or intent (e.g. Greeting, Quick check-in, Light chat)\n\
        \n\
        Examples:\n\
        \"debug 500 errors in production\" → Debugging production 500 errors\n\
        \"refactor user service\" → Refactoring user service\n\
        \"why is app.js failing\" → app.js failure investigation\n\
        \"implement rate limiting\" → Rate limiting implementation\n\
        \"how do I connect postgres to my API\" → Postgres API connection";

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
        apply_native_chat_template(template_str, messages, None, None, true, &bos_text, &eos_text, false)
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
    let prompt_len = tokens.len() as i32;
    let eos_token = model.token_eos();

    for i in 0..30 {
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

        let token_pos = prompt_len + i;
        batch.clear();
        batch.add(next_token, token_pos, &[0], true)
            .map_err(|e| format!("Title gen batch add failed: {e}"))?;
        ctx.decode(&mut batch)
            .map_err(|e| format!("Title gen decode failed: {e}"))?;
    }

    // Drop the temporary context (inference_cache untouched)
    drop(ctx);
    drop(state_guard);

    let result = title.trim().to_string();
    eprintln!("[WORKER] Title generated: {result:?}");
    Ok(result)
}

#[cfg(test)]
mod probe_verdict_tests {
    use super::{probe_verdict_word, verdict_is_complete};

    /// The exact reply that broke every thinking model (AGENT_TASKS/002). Server stderr
    /// recorded it verbatim as `[EOS_PROBE] incomplete → 3 continuation tokens:
    /// "</think>\n\nDONE"`. The old first-token check read `</think>` and concluded the
    /// model was not finished, leaking its private answer into the visible response.
    #[test]
    fn thinking_model_reply_is_recognised_as_done() {
        let word = probe_verdict_word("</think>\n\nDONE", false).expect("a verdict word");
        assert_eq!(word, "DONE");
        assert!(verdict_is_complete(&word));
    }

    #[test]
    fn plain_reply_still_recognised() {
        for reply in ["DONE", "done", " Yes", "Y"] {
            let word = probe_verdict_word(reply, false).expect("a verdict word");
            assert!(verdict_is_complete(&word), "should be complete: {reply:?}");
        }
    }

    #[test]
    fn real_continuation_is_not_mistaken_for_done() {
        let word = probe_verdict_word("</think>\n\nNext I will read the file.", false)
            .expect("a verdict word");
        assert_eq!(word, "NEXT");
        assert!(!verdict_is_complete(&word));
    }

    /// A tool call opening the reply must not be read as a verdict of "done".
    #[test]
    fn tool_call_continuation_is_not_done() {
        let word = probe_verdict_word("<tool_call>[{\"name\": \"read_file\"", false);
        // Either no bare word yet, or a word that is plainly not a completion verdict.
        assert!(word.as_deref().is_none_or(|w| !verdict_is_complete(w)));
    }

    /// Markup alone carries no verdict — keep sampling rather than guessing.
    #[test]
    fn markup_only_reply_has_no_verdict() {
        assert_eq!(probe_verdict_word("</think>", false), None);
        assert_eq!(probe_verdict_word("\n\n", false), None);
    }

    /// A half-sampled word must not be accepted while it may still be growing.
    #[test]
    fn unterminated_word_requires_terminator() {
        assert_eq!(probe_verdict_word("</think>\n\nDON", true), None);
        assert_eq!(
            probe_verdict_word("</think>\n\nDONE\n", true).as_deref(),
            Some("DONE")
        );
    }
}

#[cfg(test)]
mod probe_leak_tests {
    use super::{probe_verdict_word, verdict_is_complete, EOS_PROBE_TEXT};

    /// Observed on the 27B in the desktop app: the model answered the probe by echoing it.
    /// The reply opens with '[', which used to yield an empty word and no verdict at all.
    #[test]
    fn punctuation_led_reply_still_yields_a_verdict() {
        let echoed = "[SELF-CHECK] Are you completely done with the task? Type DONE if yes,";
        let w = probe_verdict_word(echoed, false).expect("a verdict word");
        assert_eq!(w, "SELF");
        assert!(!verdict_is_complete(&w));
    }

    /// The case that proved the parser was never the real problem (AGENT_TASKS/012):
    /// to "hi" the model replied "</think>\n\nI'm", which parses correctly to "I" — a
    /// perfectly good word that simply isn't a completion verdict. The leak happened
    /// *after* this point, when the reply was returned as response content.
    #[test]
    fn greeting_continuation_parses_but_is_not_a_verdict() {
        let w = probe_verdict_word("</think>\n\nI'm", false).expect("a verdict word");
        assert_eq!(w, "I");
        assert!(!verdict_is_complete(&w));
    }

    /// The probe question must never be mistaken for a completion verdict.
    #[test]
    fn probe_text_itself_is_not_a_completion_verdict() {
        let w = probe_verdict_word(EOS_PROBE_TEXT, false).expect("a verdict word");
        assert!(!verdict_is_complete(&w));
    }

    /// Guard the 002 behaviour: a thinking model's "DONE" is still recognised.
    #[test]
    fn thinking_model_done_still_recognised() {
        let w = probe_verdict_word("</think>\n\nDONE", false).expect("a verdict word");
        assert!(verdict_is_complete(&w));
    }
}
