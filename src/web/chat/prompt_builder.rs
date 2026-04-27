
use llama_cpp_2::{
    context::LlamaContext,
    llama_batch::LlamaBatch,
    model::AddBos,
};
use std::num::NonZeroU32;
use std::time::Instant;

use super::super::models::*;
use super::context_eval::{build_context_params, CONTEXT_SIZE};
use super::templates::apply_system_prompt_by_type_with_tags;
use super::tool_tags::{default_tags, derive_tool_tags_from_pairs, get_tool_tags_for_model, try_get_tool_tags_for_model, ToolTags};
use crate::{log_warn, sys_debug};

/// Special conversation ID for warmup cache (system prompt pre-evaluation).
pub const WARMUP_CONVERSATION_ID: &str = "__warmup__";

/// Pre-evaluate the system prompt into the KV cache after model load.
///
/// Creates a context, tokenizes just the system prompt portion, evaluates it,
/// and stores the result in `inference_cache` so the first real generation
/// can skip re-evaluating those tokens.
pub fn warmup_system_prompt(
    llama_state: SharedLlamaState,
    db: &super::super::database::Database,
) -> Result<(), String> {
    use super::super::config::{load_config, get_resolved_system_prompt};

    let config = load_config(db);
    let system_prompt = get_resolved_system_prompt(db, &Some(llama_state.clone()));

    let system_prompt = match system_prompt {
        Some(p) if !p.is_empty() => p,
        _ => {
            sys_debug!("[WARMUP] No system prompt configured, skipping warmup");
            return Ok(());
        }
    };

    let mut state_guard = llama_state
        .lock()
        .map_err(|_| "Failed to lock LLaMA state")?;
    let state = state_guard.as_mut().ok_or("LLaMA state not initialized")?;
    let model = state.model.as_ref().ok_or("No model loaded")?;

    let context_size = config.context_size.unwrap_or_else(|| {
        state
            .model_context_length
            .map(|ctx| ctx.min(CONTEXT_SIZE))
            .unwrap_or(CONTEXT_SIZE)
    });

    // Build a minimal conversation with just the system prompt
    let conversation_content = format!("SYSTEM:\n{}\n\n", system_prompt);

    let template_type = state.chat_template_type.clone();
    let chat_template_string = state.chat_template_string.clone();
    let general_name = state.general_name.clone();

    let tags = get_tool_tags_for_model(general_name.as_deref()).with_overrides(
        config.tool_tag_exec_open.as_deref(),
        config.tool_tag_exec_close.as_deref(),
        config.tool_tag_output_open.as_deref(),
        config.tool_tag_output_close.as_deref(),
    );

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

    // Format using the same template as generation (no MCP tools for warmup)
    let prompt = apply_system_prompt_by_type_with_tags(
        &conversation_content,
        template_type.as_deref(),
        chat_template_string.as_deref(),
        &tags,
        &bos_text,
        &eos_text,
        None,
    )?;

    // Tokenize
    let tokens = model
        .str_to_token(&prompt, AddBos::Never)
        .map_err(|e| format!("Warmup tokenization failed: {e}"))?;

    if tokens.is_empty() {
        sys_debug!("[WARMUP] Empty token list, skipping warmup");
        return Ok(());
    }

    // Create context with the same parameters generation would use
    let n_ctx = NonZeroU32::new(context_size).expect("Context size must be non-zero");
    let offload_kqv = state.gpu_layers.unwrap_or(0) > 0;
    let flash_attention = config.flash_attention;
    let cache_type_k = config.cache_type_k.clone();
    let cache_type_v = config.cache_type_v.clone();

    let ctx_params = build_context_params(n_ctx, offload_kqv, &config);

    let start = Instant::now();
    let mut context = unsafe {
        let real_ctx = model
            .new_context(&state.backend, ctx_params)
            .map_err(|e| format!("Warmup context creation failed: {e}"))?;
        std::mem::transmute::<LlamaContext<'_>, LlamaContext<'static>>(real_ctx)
    };

    // Evaluate system prompt tokens in batches
    const BATCH_CAP: usize = 2048;
    let n_chunks = tokens.len().div_ceil(BATCH_CAP);
    let mut batch = LlamaBatch::new(BATCH_CAP, 1);

    for chunk_idx in 0..n_chunks {
        let start_tok = chunk_idx * BATCH_CAP;
        let end_tok = std::cmp::min(start_tok + BATCH_CAP, tokens.len());

        batch.clear();
        for (offset, &token) in tokens[start_tok..end_tok].iter().enumerate() {
            let pos = start_tok + offset;
            let is_last = pos == tokens.len() - 1;
            batch
                .add(token, pos as i32, &[0], is_last)
                .map_err(|e| format!("Warmup batch add failed: {e}"))?;
        }

        context.decode(&mut batch).map_err(|e| {
            format!("Warmup decode failed (chunk {}/{}): {e}", chunk_idx + 1, n_chunks)
        })?;
    }

    let elapsed = start.elapsed();
    let tok_per_sec = tokens.len() as f64 / elapsed.as_secs_f64();
    eprintln!(
        "[WORKER] System prompt warmup: {} tokens evaluated in {:.2}s ({:.1} tok/s)",
        tokens.len(),
        elapsed.as_secs_f64(),
        tok_per_sec
    );

    // Store in inference cache for reuse by first generation
    state.inference_cache = Some(InferenceCache {
        context,
        conversation_id: WARMUP_CONVERSATION_ID.to_string(),
        evaluated_tokens: tokens,
        context_size,
        offload_kqv,
        flash_attention,
        cache_type_k,
        cache_type_v,
    });

    Ok(())
}

/// Resolve ToolTags from config: model name lookup → saved tag_pairs → old override fields.
///
/// Known models always use their native tags (model name lookup).
/// Saved tag_pairs are only used for unknown models (custom user configuration).
pub(super) fn resolve_tool_tags(config: &SamplerConfig, general_name: Option<&str>) -> ToolTags {
    // Priority 1: Saved tag_pairs from config (user chose these in Load Model modal)
    if let Some(pairs) = &config.tag_pairs {
        if let Some(tags) = derive_tool_tags_from_pairs(pairs) {
            return tags;
        }
    }
    // Priority 2: Auto-detect from model name (fallback for models loaded without tag pairs)
    if let Some(tags) = try_get_tool_tags_for_model(general_name) {
        return tags;
    }
    // Priority 3: Old override fields + default tags (backward compat)
    default_tags().with_overrides(
        config.tool_tag_exec_open.as_deref(),
        config.tool_tag_exec_close.as_deref(),
        config.tool_tag_output_open.as_deref(),
        config.tool_tag_output_close.as_deref(),
    )
}

/// Compute and cache system prompt + tool definition token counts in conversation_context.
/// Uses a content hash to skip re-tokenization when nothing changed.
pub(super) fn snapshot_context_overhead(
    db: &super::super::database::SharedDatabase,
    conversation_id: &str,
    model: &llama_cpp_2::model::LlamaModel,
    system_prompt_text: &str,
    tools_json: &str,
    log_id: &str,
) -> (i32, i32) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Compute hash of current content
    let mut hasher = DefaultHasher::new();
    system_prompt_text.hash(&mut hasher);
    tools_json.hash(&mut hasher);
    let current_hash = format!("{:x}", hasher.finish());

    // Check if cached hash matches
    if let Some(existing_hash) = db.get_context_hash(conversation_id) {
        if existing_hash == current_hash {
            let overhead = db.get_context_overhead_tokens(conversation_id);
            if overhead > 0 {
                if let Some(ctx) = db.get_conversation_context(conversation_id) {
                    return (ctx.system_prompt_tokens, ctx.tool_definitions_tokens);
                }
            }
        }
    }

    // Hash mismatch or no cache — tokenize and store
    let sys_tokens = model
        .str_to_token(system_prompt_text, llama_cpp_2::model::AddBos::Never)
        .map(|t| t.len() as i32)
        .unwrap_or(0);
    let tool_tokens = model
        .str_to_token(tools_json, llama_cpp_2::model::AddBos::Never)
        .map(|t| t.len() as i32)
        .unwrap_or(0);

    if let Err(e) = db.upsert_conversation_context(
        conversation_id,
        system_prompt_text,
        sys_tokens,
        tools_json,
        tool_tokens,
        &current_hash,
    ) {
        log_warn!(log_id, "Failed to cache conversation context: {}", e);
    }

    (sys_tokens, tool_tokens)
}

/// Inject a tool response with images into the model context via the vision pipeline.
///
/// Instead of plain text token injection, this tokenizes the model_block text with
/// `<__media__>` markers using MtmdContext, creates bitmaps from the raw image bytes,
/// and evaluates the combined text+image chunks into the existing context.
#[cfg(feature = "vision")]
pub(super) fn inject_tool_response_with_vision(
    exec_result: &super::command_executor::CommandExecutionResult,
    mtmd_ctx: &llama_cpp_2::mtmd::MtmdContext,
    context: &mut LlamaContext<'static>,
    token_pos: &mut i32,
    n_batch: u32,
    conversation_id: &str,
) -> Result<(), String> {
    use llama_cpp_2::mtmd::{MtmdBitmap, MtmdInputText};
    use crate::log_info;
    use crate::log_debug;

    let n_images = exec_result.response_images.len();
    log_info!(
        conversation_id,
        "🖼️ Vision injection: {} image(s) with model_block ({} chars)",
        n_images, exec_result.model_block.len()
    );

    // Prepend <__media__> markers (one per image) to the model block text.
    // The markers tell mtmd where to insert image embeddings in the token stream.
    let markers = "<__media__>\n".repeat(n_images);
    let text_with_markers = format!("{markers}{}", exec_result.model_block);

    // Create bitmaps from raw image bytes
    let bitmaps: Vec<MtmdBitmap> = exec_result.response_images.iter().enumerate().map(|(i, bytes)| {
        log_debug!(conversation_id, "Creating vision bitmap {} from {} bytes", i, bytes.len());
        MtmdBitmap::from_buffer(mtmd_ctx, bytes)
            .map_err(|e| format!("Failed to create image bitmap {i}: {e}"))
    }).collect::<Result<Vec<_>, String>>()?;
    let bitmap_refs: Vec<&MtmdBitmap> = bitmaps.iter().collect();

    // Tokenize text + images into chunks via MtmdContext
    let text_input = MtmdInputText {
        text: text_with_markers,
        add_special: false, // no BOS — we're mid-generation, not starting a new prompt
        parse_special: true, // parse special tokens like <|im_end|> in template wrapping
    };
    let chunks = mtmd_ctx.tokenize(text_input, &bitmap_refs)
        .map_err(|e| format!("Vision tokenization of tool response failed: {e}"))?;
    let n_chunk_tokens = chunks.total_tokens();
    log_info!(
        conversation_id,
        "Vision tokenized tool response: {} chunks, {} total tokens",
        chunks.len(), n_chunk_tokens
    );

    // Evaluate all chunks (text tokens + image embeddings) into the existing context
    let n_past = chunks.eval_chunks(mtmd_ctx, context, *token_pos, 0, n_batch as i32, true)
        .map_err(|e| format!("Vision eval_chunks for tool response failed: {e}"))?;
    log_info!(
        conversation_id,
        "Vision eval_chunks complete: token_pos {} → {}",
        *token_pos, n_past
    );
    *token_pos = n_past;

    Ok(())
}

/// Insert `<__media__>` markers into a formatted prompt, just before the
/// last occurrence of the user's message text. One marker per image tells
/// the mtmd tokenizer where each image's embeddings go in the token stream.
#[cfg(feature = "vision")]
pub(super) fn inject_media_markers(prompt: &str, user_message: &str, count: usize) -> String {
    let markers = "<__media__>\n".repeat(count);
    // Find the last occurrence of the user message in the prompt
    if let Some(pos) = prompt.rfind(user_message) {
        let mut result = String::with_capacity(prompt.len() + markers.len());
        result.push_str(&prompt[..pos]);
        result.push_str(&markers);
        result.push_str(&prompt[pos..]);
        result
    } else {
        // Fallback: prepend markers to the entire prompt
        format!("{markers}{prompt}")
    }
}
