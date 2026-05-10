//! Position-dependent deadlock test.
//!
//! The deadlock occurs at context position ~6583 during single-token injection decode.
//! This test generates tokens to specific positions and then injects, to find the
//! exact position threshold that triggers the hang.
//!
//! Run: npm run cargo -- run --release --features cuda,vision -p llama-chat-tests --bin position-test
//! With specific position: ... --bin position-test -- [model_path] [target_position]

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use std::num::NonZeroU32;
use std::time::Instant;

const SYSTEM_PROMPT: &str = "You are a helpful AI assistant.";

fn main() {
    let model_path = std::env::args().nth(1).unwrap_or_else(|| {
        "E:/ai_models/Qwen3.5-9B-Q8_0.gguf".to_string()
    });

    // Target position to reach before injection, or 0 for sweep mode
    let target_pos: i32 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let context_size: u32 = 100000;

    println!("=== Position-Dependent Deadlock Test ===");
    println!("Model: {model_path}");
    println!("Context: {context_size}");
    if target_pos > 0 {
        println!("Target position: {target_pos}");
    } else {
        println!("Mode: sweep (test positions 4000, 5000, 5500, 6000, 6200, 6400, 6500, 6550, 6600, 6700, 7000, 8000)");
    }
    println!();

    let backend = LlamaBackend::init().expect("init failed");
    println!("Loading model...");
    let model_params = LlamaModelParams::default().with_n_gpu_layers(99);
    let model = LlamaModel::load_from_file(&backend, &model_path, &model_params)
        .expect("load failed");
    println!("Model loaded.\n");

    let positions: Vec<i32> = if target_pos > 0 {
        vec![target_pos]
    } else {
        vec![4000, 5000, 5500, 6000, 6200, 6400, 6500, 6550, 6600, 6700, 7000, 8000]
    };

    for &pos in &positions {
        let result = test_position(&backend, &model, context_size, pos);
        match result {
            TestResult::Passed(ms) => {
                println!("  ✅ Position {pos}: PASSED (inject decode took {ms}ms)\n");
            }
            TestResult::Hung(token_idx, at_pos) => {
                println!("  💀 Position {pos}: HUNG at injection token #{token_idx} (ctx pos {at_pos})\n");
                println!("=== DEADLOCK POSITION FOUND: ~{at_pos} ===");
                std::process::exit(1);
            }
            TestResult::Error(msg) => {
                println!("  ⚠️ Position {pos}: ERROR: {msg}\n");
            }
        }
    }

    println!("=== ALL POSITIONS PASSED ===");
}

enum TestResult {
    Passed(u128),      // total injection decode time in ms
    Hung(usize, i32),  // (token_index, context_position) where it hung
    Error(String),
}

fn test_position(backend: &LlamaBackend, model: &LlamaModel, context_size: u32, target_pos: i32) -> TestResult {
    println!("╔══════════════════════════════════════╗");
    println!("║  Testing position {:<19}║", target_pos);
    println!("╚══════════════════════════════════════╝");

    // Create context
    let n_ctx = NonZeroU32::new(context_size).unwrap();
    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(Some(n_ctx))
        .with_flash_attention_policy(1)
        .with_offload_kqv(true);
    let mut ctx = model.new_context(backend, ctx_params).expect("context failed");

    // Build prompt
    let prompt = format!(
        "<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n<|im_start|>user\nPlease write a very long detailed story about space exploration, including many characters and plot points. Make it at least 5000 words.<|im_end|>\n<|im_start|>assistant\n"
    );
    let tokens = model.str_to_token(&prompt, AddBos::Always).expect("tokenize failed");
    println!("  Prompt: {} tokens", tokens.len());

    // Eval prompt
    let mut batch = LlamaBatch::new(2048, 1);
    eval_tokens(&mut ctx, &mut batch, &tokens, 0);
    ctx.synchronize();
    let mut token_pos = tokens.len() as i32;

    // Sampler
    let mut sampler = LlamaSampler::chain(vec![
        LlamaSampler::penalties(64, 1.0, 0.0, 1.5),
        LlamaSampler::temp(0.7),
        LlamaSampler::top_k(20),
        LlamaSampler::top_p(0.8, 1),
        LlamaSampler::dist(42),
    ], true);

    // Generate tokens to reach target position
    let tokens_to_gen = (target_pos - token_pos).max(0) as usize;
    if tokens_to_gen > 0 {
        print!("  Generating {} tokens to reach pos {}...", tokens_to_gen, target_pos);
        let t = Instant::now();
        for i in 0..tokens_to_gen {
            let next = sampler.sample(&ctx, -1);
            if next == model.token_eos() {
                // Don't stop at EOS — we need to reach the target position
                // Just decode it and keep going (the model will generate garbage but that's fine)
            }
            batch.clear();
            batch.add(next, token_pos, &[0], true).unwrap();
            ctx.decode(&mut batch).expect("gen decode failed");
            token_pos += 1;
            if (i + 1) % 1000 == 0 {
                print!(" {}k", (i + 1) / 1000);
            }
        }
        println!(" done ({:.1}s, {:.0} tok/s)", t.elapsed().as_secs_f64(),
            tokens_to_gen as f64 / t.elapsed().as_secs_f64());
    }
    println!("  Current position: {}", token_pos);

    // Now inject a large tool output (like the app does after read_file)
    let fake_tool_output = build_injection_text(650); // ~650 tokens to match the crash scenario
    let inject_tokens = model.str_to_token(&fake_tool_output, AddBos::Never)
        .expect("tokenize failed");
    println!("  Injecting {} tokens starting at pos {}...", inject_tokens.len(), token_pos);

    // Single-token injection with per-token timing (matching app's inject_output_tokens)
    let total = inject_tokens.len();
    let inject_start = Instant::now();
    let mut hung_at: Option<(usize, i32)> = None;

    for (i, &tok) in inject_tokens.iter().enumerate() {
        batch.clear();
        let is_last = i == total - 1;
        batch.add(tok, token_pos, &[0], is_last).unwrap();

        let t = Instant::now();
        if let Err(e) = ctx.decode(&mut batch) {
            return TestResult::Error(format!("Decode failed at token {}: {e}", i));
        }
        let decode_ms = t.elapsed().as_millis();

        // If a single decode takes >8s, it's a hang
        if decode_ms > 8000 {
            hung_at = Some((i, token_pos));
            break;
        }

        // Log progress
        if i % 100 == 0 || is_last || decode_ms > 100 {
            print!("  [{}/{}] pos={} {:.0}ms", i + 1, total, token_pos,
                if decode_ms > 0 { decode_ms as f64 } else { 0.0 });
            if decode_ms > 100 {
                print!(" ⚠️SLOW");
            }
            println!();
        }

        token_pos += 1;
    }

    if let Some((idx, pos)) = hung_at {
        return TestResult::Hung(idx, pos);
    }

    let total_ms = inject_start.elapsed().as_millis();
    println!("  Injection complete: {} tokens in {}ms ({:.1} tok/s)",
        total, total_ms, total as f64 / (total_ms as f64 / 1000.0));

    // Also try sample() after injection
    print!("  Post-inject sample()...");
    let t = Instant::now();
    let next = sampler.sample(&ctx, -1);
    let ms = t.elapsed().as_millis();
    println!(" {}ms token={}", ms, next.0);
    if ms > 5000 {
        return TestResult::Hung(total, token_pos);
    }

    TestResult::Passed(total_ms)
}

fn build_injection_text(approx_tokens: usize) -> String {
    let mut text = String::from("<tool_response>\n");
    // Generate enough text to produce ~approx_tokens tokens
    // Average ~1.3 tokens per word
    let words_needed = approx_tokens * 3 / 4;
    let paragraphs = [
        "The file contains a comprehensive project setup guide with installation instructions and configuration details. ",
        "Prerequisites include a modern compiler toolchain, web framework dependencies, and database libraries. ",
        "The directory structure follows standard conventions with source code, templates, static assets, and test files. ",
        "Configuration is managed through environment variables for port, database URL, and logging level. ",
        "API endpoints provide full CRUD operations with proper HTTP methods and JSON response formats. ",
        "Error handling includes validation of request bodies, proper HTTP status codes, and descriptive error messages. ",
        "The build process compiles all source files and produces an optimized binary for deployment. ",
        "Testing covers unit tests for individual functions and integration tests for API endpoints. ",
        "Documentation explains each module's purpose and provides usage examples for common operations. ",
        "Security considerations include input sanitization, CORS configuration, and rate limiting. ",
    ];
    let mut word_count = 0;
    let mut para_idx = 0;
    while word_count < words_needed {
        text.push_str(paragraphs[para_idx % paragraphs.len()]);
        word_count += paragraphs[para_idx % paragraphs.len()].split_whitespace().count();
        para_idx += 1;
        if para_idx % 5 == 0 {
            text.push('\n');
        }
    }
    text.push_str("\n</tool_response>");
    text
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
