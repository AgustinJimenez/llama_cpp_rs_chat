//! Test binary to reproduce the sample() deadlock after tool injection
//! with KV cache reuse between turns.
//!
//! Run: cargo run --release --features cuda,vision --bin test_sample_deadlock
//!
//! Simulates the exact app flow:
//! Turn 1: eval full prompt → generate → tool call → inject → generate → EOS
//! Turn 2: reuse KV cache, eval only delta → generate → tool call → inject → HANG?

#[allow(unused_imports)]
use llama_cpp_2::token::LlamaToken;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use std::num::NonZeroU32;
use std::time::Instant;

#[allow(unused_variables, unused_assignments, unused_mut)]
fn main() {
    let model_path = std::env::args().nth(1).unwrap_or_else(|| {
        "E:/ai_models/Qwen3.6-35B-A3B-UD-IQ4_XS.gguf".to_string()
    });

    println!("=== Sample Deadlock Test (KV Cache Reuse) ===");
    println!("Model: {model_path}");

    let backend = LlamaBackend::init().expect("Failed to init backend");

    println!("Loading model...");
    let model_params = LlamaModelParams::default().with_n_gpu_layers(40);
    let model = LlamaModel::load_from_file(&backend, &model_path, &model_params)
        .expect("Failed to load model");
    println!("Model loaded.");

    let n_ctx = NonZeroU32::new(32768).unwrap();
    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(Some(n_ctx))
        .with_flash_attention_policy(1)
        .with_offload_kqv(true);
    let mut ctx = model
        .new_context(&backend, ctx_params)
        .expect("Failed to create context");
    println!("Context created (32K, flash_attn, offload_kqv).");

    let batch_size = 512;
    let mut batch = LlamaBatch::new(batch_size, 1);

    // Build a large-ish system prompt to simulate real conversation
    let system_prompt = "You are a helpful AI assistant with tool access.\n\
        You can use browser_search to find information online.\n\
        To call a tool: <tool_call>{\"name\": \"tool\", \"arguments\": {}}</tool_call>\n\
        After the tool runs, you'll get a <tool_response>...</tool_response>.\n".repeat(20);

    let turn1_prompt = format!(
        "<|im_start|>system\n{system_prompt}<|im_end|>\n\
         <|im_start|>user\nSearch for latest news about Iran and USA war in the Strait of Hormuz<|im_end|>\n\
         <|im_start|>assistant\n"
    );

    let turn1_tokens = model.str_to_token(&turn1_prompt, AddBos::Always).expect("Tokenize failed");
    println!("\n=== TURN 1 ===");
    println!("Prompt: {} tokens", turn1_tokens.len());

    // Eval turn 1 prompt
    eval_tokens(&mut ctx, &mut batch, &turn1_tokens, 0);
    let mut token_pos = turn1_tokens.len() as i32;
    println!("Prompt evaluated. token_pos={token_pos}");

    let mut sampler = LlamaSampler::chain_simple(vec![
        LlamaSampler::temp(0.7),
        LlamaSampler::top_p(0.8, 1),
        LlamaSampler::dist(42),
    ]);

    // Generate some tokens (simulating model output before tool call)
    println!("\n--- Turn 1: Generate 30 tokens ---");
    let mut response1 = String::new();
    token_pos = generate_tokens(&model, &mut ctx, &mut sampler, &mut batch, token_pos, 30, &mut response1);
    println!("Generated {} chars", response1.len());

    // Inject tool response 1
    println!("\n--- Turn 1: Inject tool response ---");
    let tool_resp = "<|im_end|>\n<|im_start|>user\n<tool_response>\nSearch completed. Found 5 results about Iran USA tensions.\n</tool_response>\n<|im_end|>\n<|im_start|>assistant\n";
    token_pos = inject_tokens(&model, &mut ctx, &mut batch, token_pos, tool_resp);

    // Generate more (model continues after tool response)
    println!("\n--- Turn 1: Generate 30 more tokens after tool response ---");
    let mut response1b = String::new();
    token_pos = generate_tokens(&model, &mut ctx, &mut sampler, &mut batch, token_pos, 30, &mut response1b);
    println!("Generated {} chars", response1b.len());

    // Inject tool response 2
    println!("\n--- Turn 1: Inject 2nd tool response ---");
    let tool_resp2 = "<|im_end|>\n<|im_start|>user\n<tool_response>\nReuters: Iran closes Strait of Hormuz amid tensions with US. NATO holds emergency meeting.\n</tool_response>\n<|im_end|>\n<|im_start|>assistant\n";
    token_pos = inject_tokens(&model, &mut ctx, &mut batch, token_pos, tool_resp2);

    // Generate final response for turn 1
    println!("\n--- Turn 1: Generate final 40 tokens ---");
    let mut response1c = String::new();
    token_pos = generate_tokens(&model, &mut ctx, &mut sampler, &mut batch, token_pos, 40, &mut response1c);

    // Store what we've evaluated for KV cache reuse
    let turn1_all_tokens: Vec<LlamaToken> = model.str_to_token(
        &format!("{turn1_prompt}{response1}{tool_resp}{response1b}{tool_resp2}{response1c}"),
        AddBos::Always,
    ).unwrap_or_default();
    println!("\nTurn 1 complete. Total tokens in KV cache: {token_pos}");

    // === TURN 2: Simulate KV cache reuse ===
    println!("\n\n=== TURN 2 (KV cache reuse) ===");

    // Build turn 2 prompt (includes turn 1 content + new user message)
    let turn2_addition = "<|im_end|>\n<|im_start|>user\nWhat about the ceasefire negotiations?\n<|im_end|>\n<|im_start|>assistant\n";
    let turn2_new_tokens = model.str_to_token(turn2_addition, AddBos::Never).expect("Tokenize failed");

    // Find common prefix (simulating KV cache reuse)
    // In the app, the KV cache from turn 1 is reused — only new tokens are evaluated
    println!("New tokens to eval (delta): {}", turn2_new_tokens.len());

    // Eval only the new tokens (KV cache has everything up to token_pos)
    eval_tokens(&mut ctx, &mut batch, &turn2_new_tokens, token_pos);
    token_pos += turn2_new_tokens.len() as i32;
    println!("Delta evaluated. token_pos={token_pos}");

    // Generate tokens (turn 2, 1st generation cycle)
    println!("\n--- Turn 2: Generate 20 tokens ---");
    let mut response2 = String::new();
    token_pos = generate_tokens(&model, &mut ctx, &mut sampler, &mut batch, token_pos, 20, &mut response2);
    println!("Generated: {:?}", &response2[..response2.len().min(200)]);

    // Inject turn 2 tool response 1
    println!("\n--- Turn 2: Inject 1st tool response ---");
    let turn2_tool = "<|im_end|>\n<|im_start|>user\n<tool_response>\nSearch for 'ceasefire Iran USA 2026' completed.\n</tool_response>\n<|im_end|>\n<|im_start|>assistant\n";
    token_pos = inject_tokens(&model, &mut ctx, &mut batch, token_pos, turn2_tool);

    // Generate after 1st tool response in turn 2
    println!("\n--- Turn 2: Generate after 1st tool ---");
    let mut response2b = String::new();
    token_pos = generate_tokens(&model, &mut ctx, &mut sampler, &mut batch, token_pos, 20, &mut response2b);
    println!("OK — 1st tool in turn 2 works. Generated {} chars", response2b.len());

    // Inject turn 2 tool response 2 — THIS IS WHERE THE APP DEADLOCKS
    println!("\n--- Turn 2: Inject 2nd tool response (DEADLOCK POINT) ---");
    let turn2_tool2 = "<|im_end|>\n<|im_start|>user\n<tool_response>\nSearch for 'Strait of Hormuz ceasefire details' completed.\n</tool_response>\n<|im_end|>\n<|im_start|>assistant\n";
    token_pos = inject_tokens(&model, &mut ctx, &mut batch, token_pos, turn2_tool2);

    println!("\n--- Turn 2: sample() after 2nd tool injection (EXPECTED DEADLOCK) ---");
    println!("Calling sample()...");
    let t = Instant::now();
    let next = sampler.sample(&ctx, -1);
    println!("  sample() returned in {}ms: token={:?}", t.elapsed().as_millis(), next);

    #[allow(deprecated)]
    let s = model.token_to_str(next, llama_cpp_2::model::Special::Tokenize).unwrap_or_default();
    println!("  Token: {:?}", s);

    // If we get here, try a 3rd injection
    println!("\n--- Turn 2: 3rd tool injection ---");
    let turn2_tool3 = "<|im_end|>\n<|im_start|>user\n<tool_response>\nBBC News: Ceasefire talks ongoing.\n</tool_response>\n<|im_end|>\n<|im_start|>assistant\n";
    token_pos = inject_tokens(&model, &mut ctx, &mut batch, token_pos, turn2_tool3);

    let t = Instant::now();
    let next = sampler.sample(&ctx, -1);
    println!("  3rd: sample() returned in {}ms", t.elapsed().as_millis());

    println!("\n=== TEST PASSED — no deadlock ===");
}

fn eval_tokens(ctx: &mut llama_cpp_2::context::LlamaContext, batch: &mut LlamaBatch, tokens: &[LlamaToken], start_pos: i32) {
    let chunk_size = 512;
    for (chunk_idx, chunk) in tokens.chunks(chunk_size).enumerate() {
        batch.clear();
        for (offset, &token) in chunk.iter().enumerate() {
            let pos = start_pos + (chunk_idx * chunk_size + offset) as i32;
            let is_last = chunk_idx * chunk_size + offset == tokens.len() - 1;
            batch.add(token, pos, &[0], is_last).unwrap();
        }
        ctx.decode(batch).expect("Decode failed");
    }
    ctx.synchronize();
}

fn inject_tokens(model: &LlamaModel, ctx: &mut llama_cpp_2::context::LlamaContext, batch: &mut LlamaBatch, mut token_pos: i32, text: &str) -> i32 {
    let tokens = model.str_to_token(text, AddBos::Never).expect("Tokenize failed");
    println!("  Injecting {} tokens at pos {}", tokens.len(), token_pos);
    eval_tokens(ctx, batch, &tokens, token_pos);
    token_pos += tokens.len() as i32;
    println!("  Done. token_pos={token_pos}");
    token_pos
}

fn generate_tokens(
    model: &LlamaModel,
    ctx: &mut llama_cpp_2::context::LlamaContext,
    sampler: &mut LlamaSampler,
    batch: &mut LlamaBatch,
    mut token_pos: i32,
    count: usize,
    response: &mut String,
) -> i32 {
    for i in 0..count {
        let t = Instant::now();
        let next = sampler.sample(ctx, -1);
        let ms = t.elapsed().as_millis();
        if ms > 1000 {
            println!("  [SLOW] sample() took {}ms at token {}", ms, i);
        }
        if ms > 5000 {
            println!("  [HANG DETECTED] sample() took {}ms — aborting", ms);
            break;
        }

        #[allow(deprecated)]
        let s = model.token_to_str(next, llama_cpp_2::model::Special::Tokenize).unwrap_or_default();
        response.push_str(&s);

        if next == model.token_eos() {
            println!("  EOS at token {i}");
            // Don't decode EOS, just advance position
            token_pos += 1;
            break;
        }

        batch.clear();
        batch.add(next, token_pos, &[0], true).unwrap();
        ctx.decode(batch).expect("Decode failed");
        token_pos += 1;
    }
    token_pos
}
