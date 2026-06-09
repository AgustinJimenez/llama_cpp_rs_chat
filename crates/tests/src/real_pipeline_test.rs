//! Real pipeline deadlock reproduction test.
//!
//! Unlike `threaded_injection` which calls decode/sample directly, this test
//! uses the actual `generate_llama_response()` function — the same one the
//! worker process calls. This means:
//! - Real tool parsing and execution (read_file, execute_command, etc.)
//! - Real token injection via command_executor::inject_output_tokens()
//! - Real conversation logging to SQLite
//! - Real sampler chain creation
//! - Real Jinja template rendering
//! - Real prompt building with system prompt + tool definitions
//! - Watchdog thread, first-injection workaround, etc.
//!
//! The only thing NOT present is the worker's IPC pipe architecture (stdin/stdout
//! JSON lines to parent process). If this test deadlocks, the bug is in the
//! generation pipeline itself. If it doesn't, the bug is in the IPC/process layer.
//!
//! Run: npm run cargo -- run --release --features cuda,vision -p llama-chat-tests --bin real-pipeline-test
//! Or with model: npm run cargo -- run --release --features cuda,vision -p llama-chat-tests --bin real-pipeline-test -- E:/ai_models/Model.gguf

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use llama_chat_db::Database;
use llama_chat_engine::{generate_llama_response, load_model, GenerationOutput};
use llama_chat_types::models::{SharedLlamaState, TokenData};
use llama_chat_types::SamplerConfig;
use llama_chat_config::sampler_config_to_db;
use tokio::sync::mpsc;

/// Messages that trigger tool calls for injection testing.
const TOOL_TRIGGER_MESSAGES: &[&str] = &[
    // Simple read_file — triggers token injection
    "Read the file at E:/repo/llama_cpp_rs_chat/README.md and summarize what this project does.",
    // execute_command — triggers subprocess + token injection
    "Run the command `dir E:\\repo\\llama_cpp_rs_chat\\crates` and tell me what crates exist.",
    // Multiple tools — triggers multiple injections per turn
    "First read E:/repo/llama_cpp_rs_chat/Cargo.toml, then run `cargo --version`, and summarize both.",
    // Large file read — triggers large token injection
    "Read the file E:/repo/llama_cpp_rs_chat/CURRENT_TASK.md and give me the key findings.",
    // Search — triggers search tool + injection
    "Search for files containing 'deadlock' in E:/repo/llama_cpp_rs_chat/crates/llama-chat-engine/src/",
    // Another round of read_file
    "Read E:/repo/llama_cpp_rs_chat/crates/llama-chat-engine/src/token_loop.rs and explain the watchdog.",
    // Command that produces substantial output
    "Run `git log --oneline -20` in E:/repo/llama_cpp_rs_chat and summarize the recent changes.",
    // list_directory
    "List all files in E:/repo/llama_cpp_rs_chat/src/hooks/ and tell me what each hook does.",
];

fn main() {
    let model_path = std::env::args().nth(1).unwrap_or_else(|| {
        "E:/ai_models/Qwen3.5-9B-Q8_0.gguf".to_string()
    });

    let num_rounds: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(TOOL_TRIGGER_MESSAGES.len());

    eprintln!("=== Real Pipeline Deadlock Test ===");
    eprintln!("Model: {model_path}");
    eprintln!("Rounds: {num_rounds}");
    eprintln!();

    // Build a multi-thread tokio runtime (like the worker's generation thread)
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("Failed to build tokio runtime");

    rt.block_on(async move {
        run_test(&model_path, num_rounds).await;
    });
}

async fn run_test(model_path: &str, num_rounds: usize) {
    // 1. Create in-memory database
    let db = Arc::new(
        Database::new(":memory:").expect("Failed to create in-memory database"),
    );
    eprintln!("[TEST] Database created (in-memory)");

    // 2. Create LlamaState and load model
    let llama_state: SharedLlamaState = Arc::new(Mutex::new(None));
    let progress = Arc::new(AtomicU8::new(0));

    eprintln!("[TEST] Loading model: {model_path}");
    let t_load = Instant::now();
    load_model(
        llama_state.clone(),
        model_path,
        Some(99), // all GPU layers
        None,     // default model params
        None,     // no mmproj
        Some(progress),
    )
    .await
    .expect("Failed to load model");
    eprintln!("[TEST] Model loaded in {:.1}s", t_load.elapsed().as_secs_f64());

    // 2b. Save config with the correct model path so generate_llama_response
    // doesn't try to load the default granite model from SamplerConfig::default()
    {
        let mut config = SamplerConfig::default();
        config.model_path = Some(model_path.to_string());
        config.context_size = Some(32768);
        config.flash_attention = true;
        let db_config = sampler_config_to_db(&config);
        db.save_config(&db_config).expect("Failed to save config");
        eprintln!("[TEST] Config saved (model_path={model_path}, context=32768)");
    }

    // 3. Run generation rounds
    let cancel = Arc::new(AtomicBool::new(false));
    let mut total_injections = 0u32;
    let mut total_tokens = 0i32;

    for round in 0..num_rounds {
        let msg = TOOL_TRIGGER_MESSAGES[round % TOOL_TRIGGER_MESSAGES.len()];
        eprintln!();
        eprintln!("╔══════════════════════════════════════════════════════════╗");
        eprintln!("║  Round {}/{}: {:<46}║", round + 1, num_rounds, &msg[..msg.len().min(46)]);
        eprintln!("╚══════════════════════════════════════════════════════════╝");

        // Create a fresh conversation for each round (avoids context accumulation)
        let logger = llama_chat_db::conversation::ConversationLogger::new(
            db.clone(),
            Some("You are a helpful AI assistant with access to tools. Use tools when asked to read files, execute commands, or search. Be concise in your responses."),
        )
        .expect("Failed to create conversation logger");
        let shared_logger = Arc::new(Mutex::new(logger));

        // Auto-continue loop: keep generating until model finishes (stop/length/error)
        // This mimics the real app's auto-continue behavior for tool_continue/tool_calls
        let mut continuation = 0u32;
        let mut current_msg = msg.to_string();
        let mut skip_user_log = false;
        let t_round = Instant::now();

        loop {
            // Token channel — consume tokens and print progress
            let (token_tx, mut token_rx) = mpsc::unbounded_channel::<TokenData>();
            let token_count = Arc::new(std::sync::atomic::AtomicI32::new(0));
            let tc = token_count.clone();
            let recv_task = tokio::spawn(async move {
                let mut last_print = Instant::now();
                while let Some(td) = token_rx.recv().await {
                    tc.fetch_add(1, Ordering::Relaxed);
                    if last_print.elapsed().as_secs() >= 2 {
                        let count = tc.load(Ordering::Relaxed);
                        eprint!("[{count} tok] ");
                        last_print = Instant::now();
                    }
                    if let Some(ref status) = td.status {
                        if !status.is_empty() {
                            eprintln!("  [STATUS] {status}");
                        }
                    }
                }
            });

            let gen_result = tokio::time::timeout(
                std::time::Duration::from_secs(600),
                generate_llama_response(
                    &current_msg,
                    llama_state.clone(),
                    shared_logger.clone(),
                    Some(token_tx),
                    skip_user_log,
                    db.clone(),
                    cancel.clone(),
                    None,
                    None,
                    None,
                ),
            )
            .await;

            let gen_tokens = token_count.load(Ordering::Relaxed);
            let _ = recv_task.await;

            match gen_result {
                Ok(Ok(output)) => {
                    total_tokens += gen_tokens;
                    let reason = &output.finish_reason;

                    if reason == "tool_continue" || reason == "tool_calls" {
                        total_injections += 1;
                        continuation += 1;
                        eprintln!("  [continuation #{continuation}] reason={reason}, tokens={gen_tokens}, ctx={}/{}",
                            output.tokens_used, output.max_tokens);

                        if continuation > 20 {
                            eprintln!("  ⚠️ Too many continuations (>20) — stopping round");
                            break;
                        }

                        // Continue the conversation (like app's auto-continue)
                        current_msg = "Continue".to_string();
                        skip_user_log = true;
                        continue;
                    }

                    // Generation finished normally
                    let elapsed = t_round.elapsed().as_secs_f64();
                    eprintln!();
                    eprintln!("  ✅ Round {} completed in {:.1}s ({} continuations)", round + 1, elapsed, continuation);
                    eprintln!("     Reason: {reason}");
                    eprintln!("     Tokens: {} gen, {}/{} ctx", gen_tokens, output.tokens_used, output.max_tokens);
                    if let Some(tps) = output.gen_tok_per_sec {
                        eprintln!("     Speed: {:.1} tok/s", tps);
                    }
                    let resp_preview = &output.response[..output.response.len().min(120)].replace('\n', " ");
                    eprintln!("     Response: {resp_preview}...");
                    break;
                }
                Ok(Err(e)) => {
                    let elapsed = t_round.elapsed().as_secs_f64();
                    eprintln!();
                    eprintln!("  ❌ Round {} FAILED after {:.1}s (continuation #{}): {}",
                        round + 1, elapsed, continuation, e);
                    if e.contains("deadlock") || e.contains("watchdog") || e.contains("crashed") {
                        eprintln!();
                        eprintln!("💀 DEADLOCK REPRODUCED IN REAL PIPELINE!");
                        eprintln!("   Round: {}, Continuation: {}", round + 1, continuation);
                        eprintln!("   Message: {msg}");
                        eprintln!("   After {total_injections} successful tool injections");
                        std::process::exit(1);
                    }
                    break;
                }
                Err(_timeout) => {
                    eprintln!();
                    eprintln!("  ⏰ Round {} TIMEOUT after 600s (continuation #{})!", round + 1, continuation);
                    eprintln!();
                    eprintln!("💀 LIKELY DEADLOCK — generation hung for 10 minutes!");
                    eprintln!("   Round: {}, Message: {msg}", round + 1);
                    std::process::exit(1);
                }
            }
        }
    }

    eprintln!();
    eprintln!("=== ALL {num_rounds} ROUNDS COMPLETED ===");
    eprintln!("Total tokens generated: {total_tokens}");
    eprintln!("Total tool injections: {total_injections}");
    eprintln!("No deadlocks detected.");
}
