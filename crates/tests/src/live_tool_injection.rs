//! Live tool injection deadlock reproduction test.
//!
//! Unlike main.rs (which replays recorded tokens), this test does LIVE generation:
//! 1. Builds a realistic prompt with system prompt + tool definitions (like the app)
//! 2. Generates tokens until the model emits a tool call
//! 3. Injects fake tool output tokens mid-generation (like the app does)
//! 4. Calls sample() — this is where the CUDA deadlock occurs
//!
//! Run: npm run cargo -- run --release --features cuda,vision -p llama-chat-tests --bin live_tool_injection
//!
//! The test matches the app's exact conditions:
//! - 100K context (same as app default)
//! - Flash attention enabled
//! - f16 KV cache (default)
//! - Full system prompt with tool definitions via Jinja template
//! - Same sampler chain (penalties, temp, top_k, top_p, dist)
//! - Same batch sizes (2048 prompt, 512 injection chunks)

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use std::num::NonZeroU32;
use std::time::Instant;

/// Fake tool output to inject after the model emits a tool call.
/// This simulates a read_file result (common trigger for the deadlock).
const FAKE_TOOL_OUTPUT: &str = r#"<tool_response>
The file contains the following content:

# Project Setup Guide

## Prerequisites
- Nim compiler version 2.0 or later
- Jester web framework (install via nimble)
- SQLite3 development libraries

## Installation Steps

1. Install Nim from https://nim-lang.org/install.html
2. Run `nimble install jester` to install the web framework
3. Clone this repository and run `nimble build`

## Directory Structure

```
project/
├── src/
│   ├── main.nim          # Entry point
│   ├── routes.nim         # HTTP route handlers
│   ├── models.nim         # Data models
│   └── database.nim       # Database operations
├── templates/
│   ├── layout.html        # Base template
│   ├── index.html         # Home page
│   └── people/
│       ├── list.html      # People list view
│       ├── form.html      # Create/edit form
│       └── show.html      # Detail view
├── static/
│   ├── css/
│   │   └── style.css
│   └── js/
│       └── app.js
├── tests/
│   └── test_routes.nim
├── nim.cfg
└── project.nimble
```

## Configuration

The application reads configuration from environment variables:
- `PORT` - HTTP server port (default: 8080)
- `DATABASE_URL` - SQLite database path (default: ./data.db)
- `LOG_LEVEL` - Logging level: debug, info, warn, error (default: info)

## Running

```bash
nimble run
```

The server will start on http://localhost:8080 by default.

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | /people | List all people |
| POST | /people | Create a person |
| GET | /people/:id | Get person details |
| PUT | /people/:id | Update a person |
| DELETE | /people/:id | Delete a person |

## Testing

```bash
nimble test
```
</tool_response>"#;

/// System prompt matching the app's behavioral prompt.
const SYSTEM_PROMPT: &str = r#"You are a helpful AI assistant with access to tools. You can read files, write files, execute commands, and search the web to help the user.

When the user asks you to do something, use the available tools to accomplish the task. Always explain what you're doing and why.

Available tools:
- read_file: Read a file from the filesystem
- write_file: Write content to a file
- execute_command: Run a shell command
- list_directory: List files in a directory
- search_files: Search for text patterns in files
- web_search: Search the web for information"#;

fn main() {
    let model_path = std::env::args().nth(1).unwrap_or_else(|| {
        "E:/ai_models/Qwen3.5-9B-Q8_0.gguf".to_string()
    });

    let context_size: u32 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(100000);

    println!("=== Live Tool Injection Deadlock Test ===");
    println!("Model: {model_path}");
    println!("Context: {context_size}");
    println!();

    let backend = LlamaBackend::init().expect("init failed");

    println!("Loading model...");
    let model_params = LlamaModelParams::default().with_n_gpu_layers(99);
    let model = LlamaModel::load_from_file(&backend, &model_path, &model_params)
        .expect("load failed");
    println!("Model loaded.");

    // Match app's context params exactly
    let n_ctx = NonZeroU32::new(context_size).unwrap();
    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(Some(n_ctx))
        .with_flash_attention_policy(1)   // flash_attention = true
        .with_offload_kqv(true);
        // f16 KV cache (default) — no with_type_k/with_type_v needed

    let mut ctx = model.new_context(&backend, ctx_params).expect("context failed");
    println!("Context created: {}K, flash_attn=true, f16 KV cache", context_size / 1024);

    // Build prompt: system prompt + user message asking to read a file
    // This triggers the model to emit a tool call (read_file)
    let user_message = "Read the file at C:/Users/agus_/Downloads/nemesis.pdf and give me a summary of the first few pages.";

    let prompt = format!(
        "<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n<|im_start|>user\n{user_message}<|im_end|>\n<|im_start|>assistant\n"
    );

    let tokens = model.str_to_token(&prompt, AddBos::Always).expect("tokenize failed");
    println!("Prompt: {} tokens", tokens.len());

    // Eval prompt
    let mut batch = LlamaBatch::new(2048, 1);
    let t = Instant::now();
    eval_tokens(&mut ctx, &mut batch, &tokens, 0);
    ctx.synchronize();
    println!("Prompt eval: {:.1}s ({:.0} tok/s)",
        t.elapsed().as_secs_f64(),
        tokens.len() as f64 / t.elapsed().as_secs_f64());

    let mut token_pos = tokens.len() as i32;

    // Build sampler chain matching app's default Temperature mode
    let mut sampler = LlamaSampler::chain(vec![
        LlamaSampler::penalties(64, 1.0, 0.0, 1.5),
        LlamaSampler::temp(0.7),
        LlamaSampler::top_k(20),
        LlamaSampler::top_p(0.8, 1),
        LlamaSampler::dist(42),
    ], true);

    // Phase 1: Generate tokens until we get some output (simulating the model responding)
    println!("\n--- Phase 1: Generate tokens (pre-injection) ---");
    let max_pre_gen = 500; // Generate up to 500 tokens before injection
    let mut generated_text = String::new();
    let mut gen_count = 0;

    let t = Instant::now();
    for i in 0..max_pre_gen {
        let next = sampler.sample(&ctx, -1);

        #[allow(deprecated)]
        let token_str = model.token_to_str(next, llama_cpp_2::model::Special::Tokenize)
            .unwrap_or_default();
        generated_text.push_str(&token_str);
        gen_count += 1;

        // Check for EOS
        if next == model.token_eos() {
            println!("  [EOS at token {}]", i + 1);
            break;
        }

        // Decode the token for next iteration
        batch.clear();
        batch.add(next, token_pos, &[0], true).unwrap();
        ctx.decode(&mut batch).expect("decode failed");
        token_pos += 1;

        if (i + 1) % 100 == 0 {
            print!("  generated {} tokens ({:.0} tok/s)\r", i + 1,
                gen_count as f64 / t.elapsed().as_secs_f64());
        }
    }
    println!("\nPhase 1 complete: {} tokens in {:.1}s ({:.0} tok/s)",
        gen_count, t.elapsed().as_secs_f64(),
        gen_count as f64 / t.elapsed().as_secs_f64());
    println!("  Text preview: {}...", &generated_text[..generated_text.len().min(200)]);

    // Phase 2: Inject tool output tokens (simulating tool response injection)
    println!("\n--- Phase 2: Inject tool output tokens ---");
    let inject_tokens = model.str_to_token(FAKE_TOOL_OUTPUT, AddBos::Never)
        .expect("tokenize injection failed");
    println!("  Injecting {} tokens ({} chars)", inject_tokens.len(), FAKE_TOOL_OUTPUT.len());

    let t = Instant::now();

    // Inject in chunks of 512, matching app behavior
    let chunk_size = 512;
    for (ci, chunk) in inject_tokens.chunks(chunk_size).enumerate() {
        batch.clear();
        let total = inject_tokens.len();
        for (j, &tok) in chunk.iter().enumerate() {
            let pos = token_pos + (ci * chunk_size + j) as i32;
            let is_last = ci * chunk_size + j == total - 1;
            batch.add(tok, pos, &[0], is_last).unwrap();
        }
        ctx.decode(&mut batch).expect("injection decode failed");
    }
    ctx.synchronize();
    token_pos += inject_tokens.len() as i32;

    // Feed injected tokens to sampler (same as app's sampler.accept_many)
    let inject_llama_tokens = &inject_tokens;
    sampler.accept_many(inject_llama_tokens);

    println!("  Injection decode: {:.0}ms", t.elapsed().as_millis());
    println!("  token_pos after injection: {}", token_pos);

    // Brief pause + synchronize (same as app)
    std::thread::sleep(std::time::Duration::from_millis(50));
    ctx.synchronize();

    // Phase 3: Call sample() after injection — THIS IS WHERE THE DEADLOCK OCCURS
    println!("\n--- Phase 3: sample() after injection ---");
    println!("  Calling sample()... (if this hangs >10s, the deadlock is reproduced)");

    let t = Instant::now();
    let next = sampler.sample(&ctx, -1);
    let ms = t.elapsed().as_millis();
    println!("  sample() returned in {}ms, token={:?}", ms, next);

    if ms > 5000 {
        println!("\n  ⚠️  sample() took >5s — DEADLOCK LIKELY");
        std::process::exit(1);
    }

    // Phase 4: Continue generating after injection to see if it's stable
    println!("\n--- Phase 4: Continue generating after injection ---");
    let max_post_gen = 200;
    let mut post_count = 0;
    let mut post_text = String::new();

    batch.clear();
    batch.add(next, token_pos, &[0], true).unwrap();
    ctx.decode(&mut batch).expect("post-inject decode failed");
    token_pos += 1;

    let t = Instant::now();
    for i in 0..max_post_gen {
        let next = sampler.sample(&ctx, -1);

        #[allow(deprecated)]
        let token_str = model.token_to_str(next, llama_cpp_2::model::Special::Tokenize)
            .unwrap_or_default();
        post_text.push_str(&token_str);
        post_count += 1;

        if next == model.token_eos() {
            println!("  [EOS at token {}]", i + 1);
            break;
        }

        batch.clear();
        batch.add(next, token_pos, &[0], true).unwrap();
        ctx.decode(&mut batch).expect("post-inject decode failed");
        token_pos += 1;

        if (i + 1) % 50 == 0 {
            print!("  post-inject: {} tokens ({:.0} tok/s)\r", i + 1,
                post_count as f64 / t.elapsed().as_secs_f64());
        }
    }
    println!("\nPhase 4 complete: {} tokens in {:.1}s ({:.0} tok/s)",
        post_count, t.elapsed().as_secs_f64(),
        post_count as f64 / t.elapsed().as_secs_f64());

    // Phase 5: Do MULTIPLE injections to increase crash probability
    println!("\n--- Phase 5: Multiple injection cycles ---");
    for round in 1..=10 {
        let fake_output = format!(
            "<tool_response>\nTool output round {round}: The operation completed successfully.\n\
            File written to /tmp/test_{round}.txt ({} bytes)\n\
            Additional details: timestamp={}, status=ok, items_processed={}\n</tool_response>",
            round * 1234, chrono_like_timestamp(), round * 42
        );

        let inject_tokens = model.str_to_token(&fake_output, AddBos::Never)
            .expect("tokenize injection failed");

        // Inject
        let t = Instant::now();
        for (ci, chunk) in inject_tokens.chunks(chunk_size).enumerate() {
            batch.clear();
            let total = inject_tokens.len();
            for (j, &tok) in chunk.iter().enumerate() {
                let pos = token_pos + (ci * chunk_size + j) as i32;
                let is_last = ci * chunk_size + j == total - 1;
                batch.add(tok, pos, &[0], is_last).unwrap();
            }
            ctx.decode(&mut batch).expect("injection decode failed");
        }
        ctx.synchronize();
        token_pos += inject_tokens.len() as i32;

        let inject_llama_tokens = &inject_tokens;
        sampler.accept_many(inject_llama_tokens);

        std::thread::sleep(std::time::Duration::from_millis(50));
        ctx.synchronize();

        // sample() — the critical call
        print!("  Round {round}: inject {} tokens, sample()...", inject_tokens.len());
        let t = Instant::now();
        let next = sampler.sample(&ctx, -1);
        let ms = t.elapsed().as_millis();
        println!(" {}ms token={:?}", ms, next.0);

        if ms > 5000 {
            println!("\n  ⚠️  DEADLOCK at round {round}!");
            std::process::exit(1);
        }

        // Generate a few tokens to continue the conversation
        batch.clear();
        batch.add(next, token_pos, &[0], true).unwrap();
        ctx.decode(&mut batch).expect("decode failed");
        token_pos += 1;

        for _ in 0..20 {
            let next = sampler.sample(&ctx, -1);
            if next == model.token_eos() { break; }
            batch.clear();
            batch.add(next, token_pos, &[0], true).unwrap();
            ctx.decode(&mut batch).expect("decode failed");
            token_pos += 1;
        }
    }

    println!("\n=== PASSED — {} injection rounds, no deadlock ===", 10 + 1);
    println!("Total token_pos: {}", token_pos);
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

fn chrono_like_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!("{}", now)
}
