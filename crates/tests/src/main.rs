//! Reproduce sample() crash after tool injection.
//! Uses the exact prompt and injection tokens from a real crash.
//!
//! Run: npm run cargo -- run --release --features cuda,vision -p llama-chat-tests

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
        "E:/ai_models/Qwen3.6-35B-A3B-UD-IQ4_XS.gguf".to_string()
    });

    // Load test data
    let test_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let prompt = std::fs::read_to_string(test_dir.join("test_data_prompt.txt"))
        .expect("Missing test_data_prompt.txt");
    let inject_raw = std::fs::read_to_string(test_dir.join("test_data_inject.txt"))
        .expect("Missing test_data_inject.txt");

    // Parse injection entries: [INJECT pos=N count=M] [token_ids...]
    let injections: Vec<(i32, Vec<i32>)> = inject_raw
        .lines()
        .filter(|l| l.starts_with("[INJECT"))
        .map(|line| {
            let pos_start = line.find("pos=").unwrap() + 4;
            let pos_end = line[pos_start..].find(' ').unwrap() + pos_start;
            let pos: i32 = line[pos_start..pos_end].parse().unwrap();
            let tokens_start = line.find("] [").unwrap() + 2;
            let tokens_str = &line[tokens_start..line.len()];
            let tokens: Vec<i32> = tokens_str
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split(", ")
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().parse().unwrap())
                .collect();
            (pos, tokens)
        })
        .collect();

    println!("=== Crash Reproduction Test ===");
    println!("Model: {model_path}");
    println!("Prompt: {} chars", prompt.len());
    println!("Injections: {}", injections.len());
    for (i, (pos, toks)) in injections.iter().enumerate() {
        println!("  #{}: pos={}, {} tokens", i + 1, pos, toks.len());
    }

    let backend = LlamaBackend::init().expect("init failed");
    println!("\nLoading model...");
    let model_params = LlamaModelParams::default().with_n_gpu_layers(40);
    let model = LlamaModel::load_from_file(&backend, &model_path, &model_params)
        .expect("load failed");
    println!("Model loaded.");

    let n_ctx = NonZeroU32::new(119040).unwrap();
    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(Some(n_ctx))
        .with_flash_attention_policy(1)
        .with_offload_kqv(true)
        .with_type_k(llama_cpp_2::context::params::KvCacheType::Unknown(43))
        .with_type_v(llama_cpp_2::context::params::KvCacheType::Unknown(41));
    let mut ctx = model.new_context(&backend, ctx_params).expect("context failed");
    println!("Context: 119K, flash_attn, TURBO2/TURBO3");

    let tokens = model.str_to_token(&prompt, AddBos::Always).expect("tokenize failed");
    println!("\nPrompt: {} tokens", tokens.len());

    // Use same batch size as app (2048 for prompt, 512 for injection)
    let mut batch = LlamaBatch::new(2048, 1);
    let t = Instant::now();
    eval_tokens(&mut ctx, &mut batch, &tokens, 0);
    println!("Prompt eval: {:.1}s ({:.0} tok/s)", t.elapsed().as_secs_f64(),
        tokens.len() as f64 / t.elapsed().as_secs_f64());

    let mut token_pos = tokens.len() as i32;

    // Build sampler chain identical to app's Temperature mode:
    // penalties → DRY → top_n_sigma → temp → top_k → top_p → dist
    // Config: temp=0.7, top_p=0.8, top_k=20, presence_penalty=1.5, repeat_penalty=1.0
    let mut sampler = LlamaSampler::chain(vec![
        // Penalties: presence_penalty=1.5, repeat_penalty=1.0, freq_penalty=0.0, last_n=64
        LlamaSampler::penalties(64, 1.0, 0.0, 1.5),
        LlamaSampler::temp(0.7),
        LlamaSampler::top_k(20),
        LlamaSampler::top_p(0.8, 1),
        LlamaSampler::dist(42),
    ], true);

    for (i, (expected_pos, inject_toks)) in injections.iter().enumerate() {
        // Generate tokens to reach injection position
        let gap = (*expected_pos - token_pos).max(0) as usize;
        if gap > 0 {
            println!("\n  Generating {} tokens to pos {}...", gap.min(200), expected_pos);
            token_pos = gen_tokens(&model, &mut ctx, &mut sampler, &mut batch, token_pos, gap.min(200));
        }

        println!("\n--- Inject #{} at pos {} ({} tokens) ---", i + 1, token_pos, inject_toks.len());
        let lt: Vec<LlamaToken> = inject_toks.iter().map(|&id| LlamaToken(id)).collect();
        let t = Instant::now();
        eval_tokens(&mut ctx, &mut batch, &lt, token_pos);
        ctx.synchronize();
        token_pos += inject_toks.len() as i32;
        println!("  Injected {:.0}ms, pos={}", t.elapsed().as_millis(), token_pos);

        print!("  sample()... ");
        let t = Instant::now();
        let next = sampler.sample(&ctx, -1);
        let ms = t.elapsed().as_millis();
        println!("{}ms token={:?}", ms, next);

        if ms > 5000 {
            println!("  [HANG] Aborting.");
            std::process::exit(1);
        }

        batch.clear();
        batch.add(next, token_pos, &[0], true).unwrap();
        if let Err(e) = ctx.decode(&mut batch) {
            println!("  [DECODE FAILED] {e}");
            std::process::exit(1);
        }
        token_pos += 1;
    }

    println!("\n=== PASSED — no crash after {} injections ===", injections.len());
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

fn gen_tokens(model: &LlamaModel, ctx: &mut llama_cpp_2::context::LlamaContext, sampler: &mut LlamaSampler, batch: &mut LlamaBatch, mut pos: i32, count: usize) -> i32 {
    for _ in 0..count {
        let next = sampler.sample(ctx, -1);
        if next == model.token_eos() { pos += 1; break; }
        batch.clear();
        batch.add(next, pos, &[0], true).unwrap();
        ctx.decode(batch).expect("gen decode failed");
        pos += 1;
    }
    pos
}
