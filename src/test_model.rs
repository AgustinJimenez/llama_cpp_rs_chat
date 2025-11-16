use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::sampling::LlamaSampler;
use std::num::NonZeroU32;
use std::time::Instant;

fn main() {
    println!("=== LLaMA Model Test ===\n");

    // Configuration - matches your exact config
    let model_path = r"E:\.lmstudio\models\lmstudio-community\Devstral-Small-2507-GGUF\Devstral-Small-2507-Q4_K_M.gguf";
    let context_size: u32 = 131072; // 128K tokens - your exact config
    let gpu_layers: u32 = 40; // All layers on GPU

    println!("Configuration:");
    println!("  Model: {}", model_path);
    println!("  Context Size: {} tokens ({}K)", context_size, context_size / 1024);
    println!("  GPU Layers: {}", gpu_layers);
    println!();

    // Initialize backend
    println!("[1/5] Initializing LLaMA backend...");
    let backend = LlamaBackend::init().expect("Failed to initialize backend");
    println!("✓ Backend initialized\n");

    // Load model
    println!("[2/5] Loading model...");
    let load_start = Instant::now();

    let model_params = LlamaModelParams::default()
        .with_n_gpu_layers(gpu_layers);

    let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
        .expect("Failed to load model");

    let load_time = load_start.elapsed();
    println!("✓ Model loaded in {:.2}s\n", load_time.as_secs_f32());

    // Create context
    println!("[3/5] Creating context with {}K tokens...", context_size / 1024);
    let ctx_start = Instant::now();

    let n_ctx = NonZeroU32::new(context_size).expect("Context size must be non-zero");
    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(Some(n_ctx))
        .with_n_batch(512)
        .with_offload_kqv(true);  // Offload KV cache to GPU

    let mut ctx = model.new_context(&backend, ctx_params)
        .expect("Failed to create context");

    let ctx_time = ctx_start.elapsed();
    println!("✓ Context created in {:.2}s\n", ctx_time.as_secs_f32());

    // Prepare test prompt - asking for code generation like your real case
    println!("[4/5] Tokenizing test prompt...");
    let prompt = "[INST] Write me a login page in Svelte with form validation, password hashing, and session management. Include full code with comments. [/INST]";

    let token_start = Instant::now();

    let tokens = model.str_to_token(prompt, llama_cpp_2::model::AddBos::Always)
        .expect("Failed to tokenize prompt");

    let token_time = token_start.elapsed();
    println!("✓ Tokenized {} tokens in {:.2}s\n", tokens.len(), token_time.as_secs_f32());
    println!("  Prompt: {}", prompt);
    println!("  Token count: {}", tokens.len());
    println!();

    // Create batch and decode
    println!("[5/5] Decoding tokens (testing the crash point)...");
    let decode_start = Instant::now();

    let mut batch = LlamaBatch::new(512, 1);

    // Add tokens to batch
    for (i, token) in tokens.iter().enumerate() {
        batch.add(*token, i as i32, &[0], i == tokens.len() - 1)
            .expect("Failed to add token to batch");
    }

    println!("  Batch size: {} tokens", batch.n_tokens());
    println!("  Starting decode...");

    // This is where the crash happens
    ctx.decode(&mut batch)
        .expect("Failed to decode batch");

    let decode_time = decode_start.elapsed();
    println!("✓ Decode successful in {:.2}s\n", decode_time.as_secs_f32());

    // Try generating tokens
    println!("[6/5] Generating response tokens...");
    let gen_start = Instant::now();

    let mut n_generated = 0;
    let max_tokens = 500; // Generate 500 tokens (enough to trigger crash if it exists)

    println!("  Generating up to {} tokens...", max_tokens);

    // Create greedy sampler
    let mut sampler = LlamaSampler::greedy();

    for i in 0..max_tokens {
        // Sample next token
        let new_token = sampler.sample(&mut ctx, batch.n_tokens() - 1);

        // Check for EOS
        if model.is_eog_token(new_token) {
            println!("  ✓ Hit end-of-generation token at {} tokens", i);
            break;
        }

        // Print progress every 50 tokens
        if i % 50 == 0 {
            println!("    ... generated {} tokens so far", i);
        }

        // Clear batch and add new token
        batch.clear();
        batch.add(new_token, (tokens.len() + i) as i32, &[0], true)
            .expect("Failed to add token");

        // Decode
        ctx.decode(&mut batch)
            .expect("Failed to decode during generation");

        n_generated += 1;
    }

    let gen_time = gen_start.elapsed();
    println!("✓ Generated {} tokens in {:.2}s\n", n_generated, gen_time.as_secs_f32());
    println!("  Tokens/sec: {:.2}", n_generated as f32 / gen_time.as_secs_f32());

    println!("\n=== TEST PASSED ===");
    println!("No crash occurred with 128K context!");
    println!("Total time: {:.2}s", (load_time + ctx_time + token_time + decode_time + gen_time).as_secs_f32());
}
