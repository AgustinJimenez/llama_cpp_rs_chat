
use llama_cpp_2::{
    context::params::{KvCacheType, LlamaContextParams},
    context::LlamaContext,
    llama_batch::LlamaBatch,
    model::LlamaModel,
    token::LlamaToken,
};
use std::num::NonZeroU32;

use super::super::models::*;
use crate::{log_debug, log_info};

// Constants for LLaMA configuration
pub(super) const CONTEXT_SIZE: u32 = 32768;

pub(super) const MODEL_PATH: &str =
    "/app/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf";

/// Parse a KV cache type string (from config) into the llama-cpp-2 enum.
pub(super) fn parse_kv_cache_type(s: &str) -> KvCacheType {
    match s.to_lowercase().as_str() {
        "f32" => KvCacheType::F32,
        "f16" => KvCacheType::F16,
        "q8_0" => KvCacheType::Q8_0,
        "q4_0" => KvCacheType::Q4_0,
        "q4_1" => KvCacheType::Q4_1,
        "q5_0" => KvCacheType::Q5_0,
        "q5_1" => KvCacheType::Q5_1,
        // TurboQuant KV cache types (requires turboquant llama.cpp fork)
        "turbo2" | "turbo2_0" => KvCacheType::Unknown(43), // GGML_TYPE_TURBO2_0
        "turbo3" | "turbo3_0" => KvCacheType::Unknown(41), // GGML_TYPE_TURBO3_0
        "turbo4" | "turbo4_0" => KvCacheType::Unknown(42), // GGML_TYPE_TURBO4_0
        _ => KvCacheType::F16, // default
    }
}

/// Build LlamaContextParams from config, applying all context-level settings.
pub(super) fn build_context_params(
    n_ctx: NonZeroU32,
    offload_kqv: bool,
    config: &SamplerConfig,
) -> LlamaContextParams {
    let mut params = LlamaContextParams::default()
        .with_n_ctx(Some(n_ctx))
        .with_offload_kqv(offload_kqv)
        .with_type_k(parse_kv_cache_type(&config.cache_type_k))
        .with_type_v(parse_kv_cache_type(&config.cache_type_v))
        .with_n_batch(config.n_batch)
        .with_n_ubatch(config.n_ubatch);

    // Enable perf timing (disabled by default in llama.cpp since no_perf defaults to true)
    // SAFETY: LlamaContextParams is a newtype wrapper around llama_context_params.
    unsafe {
        let raw = &mut *(&mut params as *mut LlamaContextParams as *mut llama_cpp_sys_2::llama_context_params);
        raw.no_perf = false;
    }

    if config.flash_attention {
        params = params.with_flash_attention_policy(1);
    }
    if config.n_threads > 0 {
        params = params.with_n_threads(config.n_threads);
    }
    if config.n_threads_batch > 0 {
        params = params.with_n_threads_batch(config.n_threads_batch);
    }
    if config.rope_freq_base > 0.0 {
        params = params.with_rope_freq_base(config.rope_freq_base);
    }
    if config.rope_freq_scale > 0.0 {
        params = params.with_rope_freq_scale(config.rope_freq_scale);
    }
    params
}

/// Evaluate tokenized prompt through the model, reusing KV cache when possible.
///
/// Returns `(context, skip_count)` where `skip_count` is how many tokens were
/// already in the cache and didn't need re-evaluation.
#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate_text_prompt(
    inference_cache: &mut Option<InferenceCache>,
    model: &LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    tokens: &[LlamaToken],
    conversation_id: &str,
    context_size: u32,
    offload_kqv: bool,
    flash_attention: bool,
    cache_type_k: &str,
    cache_type_v: &str,
    config: &SamplerConfig,
    batch_cap: usize,
    cancel: Option<&std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Result<(LlamaContext<'static>, usize), String> {
    let n_ctx = NonZeroU32::new(context_size).expect("Context size must be non-zero");

    let cached = inference_cache.take();
    let (mut ctx, skip_tokens) = match cached {
        Some(cache)
            if (cache.conversation_id == conversation_id
                || cache.conversation_id == super::prompt_builder::WARMUP_CONVERSATION_ID)
                && cache.context_size == context_size
                && cache.offload_kqv == offload_kqv
                && cache.flash_attention == flash_attention
                && cache.cache_type_k == cache_type_k
                && cache.cache_type_v == cache_type_v =>
        {
            let common_len = cache.evaluated_tokens.iter()
                .zip(tokens.iter()).take_while(|(a, b)| a == b).count();

            if common_len < cache.evaluated_tokens.len() {
                log_info!(conversation_id, "KV cache diverged at token {} (cached {}), starting fresh",
                    common_len, cache.evaluated_tokens.len());
                drop(cache.context);
                let ctx = create_fresh_context(model, backend, n_ctx, offload_kqv, config)?;
                (ctx, 0)
            } else {
                log_info!(conversation_id, "♻️ Reusing KV cache: {} of {} prompt tokens already evaluated",
                    common_len, tokens.len());
                (cache.context, common_len)
            }
        }
        _ => {
            drop(cached);
            log_debug!(conversation_id, "Creating fresh context (size={}K tokens)...", context_size / 1024);
            let ctx = create_fresh_context(model, backend, n_ctx, offload_kqv, config)?;
            (ctx, 0)
        }
    };

    // Reset perf counters so timings cover only this turn (not accumulated from cache)
    ctx.reset_timings();

    // Set abort callback so llama_decode can be interrupted by cancel flag during prompt eval
    extern "C" fn abort_cb(data: *mut std::ffi::c_void) -> bool {
        let flag = unsafe { &*(data as *const std::sync::atomic::AtomicBool) };
        flag.load(std::sync::atomic::Ordering::Relaxed)
    }
    if let Some(cancel_flag) = cancel {
        let cancel_ptr = std::sync::Arc::as_ptr(cancel_flag) as *mut std::ffi::c_void;
        unsafe { ctx.set_abort_callback(Some(abort_cb), cancel_ptr); }
    }

    // Evaluate only new tokens (skip those already in KV cache)
    let new_tokens = &tokens[skip_tokens..];
    if !new_tokens.is_empty() {
        let n_chunks = new_tokens.len().div_ceil(batch_cap);
        log_debug!(conversation_id, "Decoding {} new prompt tokens in {} chunks (skipped {})...",
            new_tokens.len(), n_chunks, skip_tokens);

        let mut batch = LlamaBatch::new(batch_cap, 1);
        for chunk_idx in 0..n_chunks {
            if let Some(cf) = cancel {
                if cf.load(std::sync::atomic::Ordering::Relaxed) {
                    unsafe { ctx.set_abort_callback(None, std::ptr::null_mut()); }
                    return Err("Cancelled".to_string());
                }
            }

            let start = chunk_idx * batch_cap;
            let end = std::cmp::min(start + batch_cap, new_tokens.len());

            batch.clear();
            for (offset, &token) in new_tokens[start..end].iter().enumerate() {
                let pos = skip_tokens + start + offset;
                let is_last = pos == tokens.len() - 1;
                batch.add(token, pos as i32, &[0], is_last)
                    .map_err(|e| format!("Batch add failed at prompt token {pos}: {e}"))?;
            }

            if let Err(e) = ctx.decode(&mut batch) {
                let err_str = format!("{e}");
                if err_str.contains("NoKvCacheSlot") {
                    unsafe { ctx.set_abort_callback(None, std::ptr::null_mut()); }
                    return Err("Context too small for conversation — try increasing context size or starting a new conversation".to_string());
                }
                // Abort callback triggered — treat as cancellation
                if err_str.contains("Unknown(2)") || cancel.map_or(false, |c| c.load(std::sync::atomic::Ordering::Relaxed)) {
                    unsafe { ctx.set_abort_callback(None, std::ptr::null_mut()); }
                    return Err("Cancelled".to_string());
                }
                unsafe { ctx.set_abort_callback(None, std::ptr::null_mut()); }
                return Err(format!("Prompt decode failed (chunk {}/{}): {e}", chunk_idx + 1, n_chunks));
            }
        }
    } else {
        log_info!(conversation_id, "All {} prompt tokens already in KV cache, skipping decode", tokens.len());
    }

    // Clear abort callback — generation loop will set its own
    unsafe { ctx.set_abort_callback(None, std::ptr::null_mut()); }

    Ok((ctx, skip_tokens))
}

/// Create a fresh LlamaContext with transmuted 'static lifetime for cache storage.
pub(super) fn create_fresh_context(
    model: &LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    n_ctx: NonZeroU32,
    offload_kqv: bool,
    config: &SamplerConfig,
) -> Result<LlamaContext<'static>, String> {
    let ctx_params = build_context_params(n_ctx, offload_kqv, config);
    unsafe {
        let real_ctx = model
            .new_context(backend, ctx_params)
            .map_err(|e| format!("Context creation failed: {e}"))?;
        Ok(std::mem::transmute::<LlamaContext<'_>, LlamaContext<'static>>(real_ctx))
    }
}
