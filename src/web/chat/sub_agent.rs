use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::AddBos;
use llama_cpp_2::sampling::LlamaSampler;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::Arc;
use tokio::sync::mpsc;

use super::command_executor::{check_and_execute_command_with_tags, inject_output_tokens};
use super::generation::create_fresh_context;
use super::tool_tags::ToolTags;
use super::super::models::*;
use super::super::native_tools;
use crate::{log_info, log_warn};

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
pub(crate) fn try_extract_spawn_agent(text: &str) -> Option<(String, Option<String>)> {
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
