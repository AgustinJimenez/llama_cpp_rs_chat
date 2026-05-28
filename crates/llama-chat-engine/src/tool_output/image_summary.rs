use llama_cpp_2::{llama_batch::LlamaBatch, sampling::LlamaSampler};
use std::num::NonZeroU32;

use crate::generation::create_fresh_context;
use llama_chat_types::*;

/// Context window used for the image summary sub-pass (tokens).
const IMG_SUMMARY_CTX: u32 = 4096;
/// Maximum tokens generated for the image description.
const IMG_SUMMARY_MAX_TOKENS: usize = 400;
/// Batch size for prompt eval during image summary.
const IMG_SUMMARY_BATCH: i32 = 512;

/// Run a vision-only sub-pass to produce a text description of one or more images.
///
/// Creates a fresh KV context, feeds the images + prompt through the multimodal
/// pipeline, and returns the generated description. Callers should clear
/// `response_images` after a successful call and inject the description as tokens
/// instead.
#[cfg(feature = "vision")]
pub(crate) fn run_image_vision_summary(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    mtmd_ctx: &llama_cpp_2::mtmd::MtmdContext,
    images: &[Vec<u8>],
    prompt: &str,
    conversation_id: &str,
) -> Result<String, String> {
    use llama_cpp_2::mtmd::{MtmdBitmap, MtmdInputText};

    if images.is_empty() {
        return Err("no images provided".to_string());
    }

    // One <__media__> marker per image tells mtmd where to splice embeddings.
    let markers = "<__media__>\n".repeat(images.len());
    let text_with_markers = format!("{markers}{prompt}");

    let bitmaps: Vec<MtmdBitmap> = images.iter().enumerate().map(|(i, bytes)| {
        MtmdBitmap::from_buffer(mtmd_ctx, bytes)
            .map_err(|e| format!("Image summary bitmap {i}: {e}"))
    }).collect::<Result<Vec<_>, _>>()?;
    let bitmap_refs: Vec<&MtmdBitmap> = bitmaps.iter().collect();

    let text_input = MtmdInputText {
        text: text_with_markers,
        add_special: true,
        parse_special: true,
    };
    let chunks = mtmd_ctx.tokenize(text_input, &bitmap_refs)
        .map_err(|e| format!("Image summary tokenize: {e}"))?;

    let config = SamplerConfig::default();
    let n_ctx = NonZeroU32::new(IMG_SUMMARY_CTX).unwrap();
    let mut ctx = create_fresh_context(model, backend, n_ctx, true, &config)?;

    let n_past = chunks.eval_chunks(mtmd_ctx, &mut ctx, 0, 0, IMG_SUMMARY_BATCH, true)
        .map_err(|e| format!("Image summary eval_chunks: {e}"))?;

    let eos = model.token_eos();
    let mut sampler = LlamaSampler::chain_simple(vec![
        LlamaSampler::temp(0.2),
        LlamaSampler::dist(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u32)
                .unwrap_or(42),
        ),
    ]);

    let mut description = String::new();
    let mut batch = LlamaBatch::new(1, 1);
    let mut pos = n_past;

    for _ in 0..IMG_SUMMARY_MAX_TOKENS {
        let next_token = sampler.sample(&ctx, -1);
        if next_token == eos { break; }

        #[allow(deprecated)]
        let token_str = model
            .token_to_str(next_token, llama_cpp_2::model::Special::Tokenize)
            .unwrap_or_default();
        description.push_str(&token_str);

        batch.clear();
        batch.add(next_token, pos, &[0], true)
            .map_err(|e| format!("Image summary gen add: {e}"))?;
        ctx.decode(&mut batch)
            .map_err(|e| format!("Image summary gen decode: {e}"))?;
        pos += 1;
    }

    drop(ctx);
    let result = description.trim().to_string();
    log_info!(conversation_id, "🖼️ Image summary: {} image(s) → {} chars", images.len(), result.len());
    Ok(result)
}
