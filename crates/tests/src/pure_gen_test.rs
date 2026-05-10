//! Pure generation deadlock test — NO injection, just continuous token generation.
//! Tests if the deadlock occurs from generating many tokens on hybrid Qwen3.5.
//!
//! Run: npm run cargo -- run --release --features cuda,vision -p llama-chat-tests --bin pure-gen-test

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use std::num::NonZeroU32;
use std::time::Instant;

fn main() {
    let model_path = std::env::args().nth(1).unwrap_or_else(|| {
        "E:/ai_models/Qwen3.5-9B-Q8_0.gguf".to_string()
    });

    let max_tokens: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(10000);

    println!("=== Pure Generation Deadlock Test ===");
    println!("Model: {model_path}");
    println!("Max tokens: {max_tokens}");
    println!("NO injection — just continuous sample() → decode() loop");
    println!();

    let backend = LlamaBackend::init().expect("init failed");
    println!("Loading model...");
    let model_params = LlamaModelParams::default().with_n_gpu_layers(99);
    let model = LlamaModel::load_from_file(&backend, &model_path, &model_params)
        .expect("load failed");
    println!("Model loaded.\n");

    let n_ctx = NonZeroU32::new(100000).unwrap();
    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(Some(n_ctx))
        .with_flash_attention_policy(1)
        .with_offload_kqv(true);
    let mut ctx = model.new_context(&backend, ctx_params).expect("context failed");

    let prompt = "<|im_start|>system\nYou are a helpful assistant.<|im_end|>\n<|im_start|>user\nWrite a very long detailed story about space exploration with many characters. Make it at least 10000 words.<|im_end|>\n<|im_start|>assistant\n";
    let tokens = model.str_to_token(prompt, AddBos::Always).expect("tokenize failed");
    println!("Prompt: {} tokens", tokens.len());

    let mut batch = LlamaBatch::new(2048, 1);
    eval_tokens(&mut ctx, &mut batch, &tokens, 0);
    ctx.synchronize();
    let mut token_pos = tokens.len() as i32;

    let mut sampler = LlamaSampler::chain(vec![
        LlamaSampler::penalties(64, 1.0, 0.0, 1.5),
        LlamaSampler::temp(0.7),
        LlamaSampler::top_k(20),
        LlamaSampler::top_p(0.8, 1),
        LlamaSampler::dist(42),
    ], true);

    println!("Generating {} tokens...", max_tokens);
    let t = Instant::now();
    let mut eos_count = 0;

    for i in 0..max_tokens {
        let next = sampler.sample(&ctx, -1);

        if next == model.token_eos() {
            eos_count += 1;
            // Don't stop — keep generating to test position limits
        }

        batch.clear();
        batch.add(next, token_pos, &[0], true).unwrap();
        ctx.decode(&mut batch).expect("decode failed");
        token_pos += 1;

        if (i + 1) % 500 == 0 {
            let elapsed = t.elapsed().as_secs_f64();
            let tok_s = (i + 1) as f64 / elapsed;
            eprintln!("  [{}/{}] pos={} {:.0} tok/s (EOS: {})",
                i + 1, max_tokens, token_pos, tok_s, eos_count);
        }
    }

    let elapsed = t.elapsed().as_secs_f64();
    println!("\n=== PASSED: {} tokens in {:.1}s ({:.0} tok/s) ===",
        max_tokens, elapsed, max_tokens as f64 / elapsed);
    println!("Final position: {}, EOS count: {}", token_pos, eos_count);
}

fn eval_tokens(ctx: &mut llama_cpp_2::context::LlamaContext, batch: &mut LlamaBatch, tokens: &[LlamaToken], start: i32) {
    let total = tokens.len();
    for (ci, chunk) in tokens.chunks(512).enumerate() {
        batch.clear();
        for (j, &tok) in chunk.iter().enumerate() {
            let pos = start + (ci * 512 + j) as i32;
            batch.add(tok, pos, &[0], ci * 512 + j == total - 1).unwrap();
        }
        ctx.decode(batch).expect("decode failed");
    }
    ctx.synchronize();
}
