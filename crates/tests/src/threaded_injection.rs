//! Threaded tool injection deadlock reproduction test.
//!
//! The single-threaded test (live_tool_injection) PASSES — no deadlock.
//! This test adds concurrency layers to find which one triggers the deadlock:
//!
//!   Level 0: Single-threaded (baseline, always passes)
//!   Level 1: Tokio runtime (multi-thread) wrapping the generation
//!   Level 2: + Background tokio tasks (timers, channels)
//!   Level 3: + Background std::threads doing CPU work
//!   Level 4: + Background thread doing periodic CUDA-adjacent work (model metadata reads)
//!   Level 5: + Simulated IPC (stdin/stdout pipe readers on background threads)
//!   Level 6: Large prompt (~5K tokens with full tool JSON definitions, like the real app)
//!   Level 7: Large prompt + real subprocess spawning during generation (cmd.exe)
//!   Level 8: Large prompt + real subprocess + real pipe I/O (child process stdin/stdout)
//!   Level 9: All of the above + SQLite DB writes during generation
//!   Level 10: High token count — generate 2000+ tokens before injection (match app's ~9K pos)
//!   Level 11: Large injection — inject 1000+ token tool output (match real read_file output)
//!   Level 12: Real tool execution pattern — generate, inject large output, generate more, inject again (loop)
//!   Level 13: Everything + real blocking tool execution (cmd.exe) between inject cycles
//!   Level 14: Real OS pipe blocking reads (BufReader on ChildStdout, like worker stdin reader)
//!   Level 15: Run as child process — CUDA runs in subprocess context with real inherited pipes
//!   Level 16: Multiple child processes running simultaneously (like worker + tool subprocesses)
//!   Level 17: Native Jinja chat template from GGUF + OpenAI tool defs (exact app prompt)
//!   Level 18: Warmup context create/destroy before main context (like app's system prompt warmup)
//!   Level 19: Multiple context create/destroy cycles (simulating sub-agent summarization)
//!   Level 20: Full app sequence: warmup→destroy→main context + sub-agent context mid-generation
//!   Level 21: Stdout pipe writes after every sample() (JSON token data, like worker IPC)
//!   Level 22: Actual worker IPC pattern — stdin command reader + stdout JSON writer + CUDA
//!   Level 23: VRAM pressure — allocate large tensors to simulate high memory usage near limit
//!   Level 24: Everything combined — the kitchen sink
//!   Level 25: Abort callback registered (like app's cancel check during CUDA compute)
//!   Level 26: Long delays (5-10s) between generation and injection (real tool execution time)
//!
//! Run: npm run cargo -- run --release --features cuda,vision -p llama-chat-tests --bin threaded-injection
//! Or with specific level: npm run cargo -- run --release --features cuda,vision -p llama-chat-tests --bin threaded-injection -- [model_path] [level]

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[path = "threaded_injection/background.rs"]
mod background;
#[path = "threaded_injection/helpers.rs"]
mod helpers;
#[path = "threaded_injection/levels.rs"]
mod levels;

use background::setup_background_noise;
use helpers::{build_jinja_prompt, build_large_prompt, build_large_tool_output, eval_tokens};
use levels::{
    level_description, run_level_15_as_child, run_mid_generation_sub_agent, run_warmup_cycles,
    FAKE_TOOL_OUTPUT, SYSTEM_PROMPT,
};

fn main() {
    let model_path = std::env::args().nth(1).unwrap_or_else(|| {
        "E:/ai_models/Qwen3.5-9B-Q8_0.gguf".to_string()
    });

    let level: u32 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(99); // default: run all levels

    let context_size: u32 = 100000;

    println!("=== Threaded Tool Injection Deadlock Test ===");
    println!("Model: {model_path}");
    println!("Context: {context_size}");
    println!("Level: {}", if level == 99 { "ALL".to_string() } else { level.to_string() });
    println!();

    let backend = LlamaBackend::init().expect("init failed");
    println!("Loading model...");
    let model_params = LlamaModelParams::default().with_n_gpu_layers(99);
    let model = LlamaModel::load_from_file(&backend, &model_path, &model_params)
        .expect("load failed");
    println!("Model loaded.\n");

    // Level 15 runs as child process — handle that first
    if std::env::var("LLAMA_TEST_CHILD").is_ok() {
        // We ARE the child process — run the injection test with level 12 logic
        println!("  [CHILD PROCESS] Running CUDA injection test inside child context...");
        let result = run_level(12, &backend, &model, context_size);
        std::process::exit(if result { 0 } else { 1 });
    }

    let levels_to_run: Vec<u32> = if level == 99 { (0..=27).collect() } else { vec![level] };

    for lvl in levels_to_run {
        let result = if lvl == 15 {
            println!("╔══════════════════════════════════════════════════════╗");
            println!("║  Level 15: {:<42}║", level_description(15));
            println!("╚══════════════════════════════════════════════════════╝");
            let passed = run_level_15_as_child(&model_path);
            if passed {
                println!("  ✅ Level 15 PASSED — child process completed successfully");
            }
            passed
        } else {
            run_level(lvl, &backend, &model, context_size)
        };
        if !result {
            println!("\n💀 DEADLOCK REPRODUCED at level {lvl}!");
            println!("This means the deadlock is caused by: {}", level_description(lvl));
            std::process::exit(1);
        }
        println!();
    }

    println!("=== ALL LEVELS PASSED — deadlock not reproduced ===");
}

fn run_level(level: u32, backend: &LlamaBackend, model: &LlamaModel, context_size: u32) -> bool {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║  Level {level}: {:<46}║", level_description(level));
    println!("╚══════════════════════════════════════════════════════╝");

    let n_ctx = NonZeroU32::new(context_size).unwrap();

    run_warmup_cycles(level, backend, model);

    // Create the MAIN context for the injection test
    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(Some(n_ctx))
        .with_flash_attention_policy(1)
        .with_offload_kqv(true);
    let mut ctx = model.new_context(backend, ctx_params).expect("context failed");

    // Level 25: Register abort callback (like the app does for cancel support)
    // This C callback is invoked by llama.cpp during decode() to check if
    // computation should be aborted. It reads an AtomicBool from inside CUDA kernels.
    let cancel_flag = Arc::new(AtomicBool::new(false));
    if level >= 25 {
        extern "C" fn abort_cb(data: *mut std::ffi::c_void) -> bool {
            let flag = unsafe { &*(data as *const AtomicBool) };
            flag.load(Ordering::Relaxed)
        }
        let cancel_ptr = Arc::as_ptr(&cancel_flag) as *mut std::ffi::c_void;
        unsafe { ctx.set_abort_callback(Some(abort_cb), cancel_ptr); }
        println!("  Abort callback registered (cancel check during CUDA compute)");
    }

    let prompt = if level == 17 {
        match build_jinja_prompt(model) {
            Ok(p) => {
                println!("  Using native Jinja chat template from GGUF");
                p
            }
            Err(e) => {
                println!("  ⚠️ Jinja template failed: {e} — falling back to manual prompt");
                build_large_prompt()
            }
        }
    } else if level >= 6 {
        build_large_prompt()
    } else {
        format!(
            "<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n<|im_start|>user\nRead the file at C:/Users/agus_/Downloads/nemesis.pdf and summarize it.<|im_end|>\n<|im_start|>assistant\n"
        )
    };
    let tokens = model.str_to_token(&prompt, AddBos::Always).expect("tokenize failed");
    println!("  Prompt: {} tokens", tokens.len());

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

    let running = Arc::new(AtomicBool::new(true));
    let heartbeat = Arc::new(AtomicU64::new(0));
    let (rt_handle, mut handles) = setup_background_noise(level, &running, &heartbeat);

    // --- Configure test parameters based on level ---
    // Levels 0-9: small pre-gen (200 tokens), small inject (~60 tokens), 15 rounds, 30 gen between
    // Level 10: large pre-gen (2000 tokens), small inject, 15 rounds
    // Level 11: small pre-gen, large inject (1000+ tokens), 10 rounds
    // Level 12: large pre-gen, large inject, 200 gen tokens between rounds (real app pattern)
    // Level 13: same as 12 + real blocking cmd.exe between inject cycles

    // Level 27: Hijack stdout fd (like worker's steal_stdout_for_ipc)
    // This redirects C-level stdout (fd 1) to stderr via _dup2, then uses the original
    // fd for IPC writes. This changes the process's file descriptor table at the OS level,
    // potentially affecting CUDA's internal I/O.
    let _ipc_file: Option<std::fs::File> = if level >= 27 {
        println!("  Hijacking stdout fd (_dup2(2, 1)) — like worker's steal_stdout_for_ipc...");
        #[cfg(windows)]
        {
            use std::os::windows::io::FromRawHandle;
            extern "C" {
                fn _dup(fd: i32) -> i32;
                fn _dup2(src: i32, dst: i32) -> i32;
            }
            unsafe {
                let ipc_fd = _dup(1);
                assert!(ipc_fd >= 0, "Failed to _dup stdout");
                _dup2(2, 1); // redirect C stdout → stderr
                extern "C" { fn _get_osfhandle(fd: i32) -> isize; }
                let handle = _get_osfhandle(ipc_fd) as usize;
                let file = std::fs::File::from_raw_handle(handle as *mut _);
                eprintln!("  stdout fd hijacked — C printf now goes to stderr, IPC fd={ipc_fd}");
                Some(file)
            }
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::io::FromRawFd;
            unsafe {
                let ipc_fd = libc::dup(1);
                assert!(ipc_fd >= 0, "Failed to dup stdout");
                libc::dup2(2, 1);
                let file = std::fs::File::from_raw_fd(ipc_fd);
                eprintln!("  stdout fd hijacked — C printf now goes to stderr, IPC fd={ipc_fd}");
                Some(file)
            }
        }
    } else {
        None
    };

    let pre_gen_count: usize = if level >= 10 { 2000 } else { 200 };
    let gen_between_rounds: usize = if level >= 12 { 200 } else { 30 };
    let num_rounds: usize = if level >= 11 { 10 } else { 15 };
    let use_large_inject = level >= 11;
    let use_blocking_tool = level >= 13;
    let use_pipe_writes = level >= 21;
    let use_vram_pressure = level >= 23;

    // Level 21+: Set up a real pipe for writing JSON after each sample()
    // This simulates the worker's stdout writes (token data sent to parent via pipe)
    let pipe_writer: Option<std::sync::Mutex<Box<dyn std::io::Write + Send>>> = if use_pipe_writes {
        // Spawn a process that reads from stdin (our pipe endpoint)
        let child = std::process::Command::new("cmd.exe")
            .args(["/C", "findstr /r .* >nul 2>nul"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        match child {
            Ok(mut c) => {
                let stdin = c.stdin.take().unwrap();
                // Keep child handle alive in a background thread
                let r = running.clone();
                handles.push(std::thread::spawn(move || {
                    while r.load(Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    let _ = c.kill();
                    let _ = c.wait();
                }));
                Some(std::sync::Mutex::new(Box::new(stdin) as Box<dyn std::io::Write + Send>))
            }
            Err(_) => None,
        }
    } else {
        None
    };

    // Level 22+: Set up stdin reader thread (simulating worker command reader)
    if level >= 22 {
        let r = running.clone();
        // Create a pipe pair: we write commands, the reader thread reads them
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<String>();

        // Reader thread (like worker's stdin reader)
        handles.push(std::thread::spawn(move || {
            while r.load(Ordering::Relaxed) {
                match cmd_rx.recv_timeout(std::time::Duration::from_millis(50)) {
                    Ok(cmd) => { std::hint::black_box(cmd); }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => break,
                }
            }
        }));

        // Writer thread (simulates server sending status commands periodically)
        let r = running.clone();
        handles.push(std::thread::spawn(move || {
            let mut seq = 0u64;
            while r.load(Ordering::Relaxed) {
                seq += 1;
                let cmd = format!("{{\"id\":{seq},\"command\":\"GetGlobalStatus\"}}");
                if cmd_tx.send(cmd).is_err() { break; }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }));
    }

    // Level 23+: VRAM pressure — allocate large GPU-adjacent memory
    let _vram_pressure: Vec<Vec<u8>> = if use_vram_pressure {
        println!("  Allocating VRAM pressure (2GB system RAM to simulate memory pressure)...");
        // Allocate ~2GB of pinned/system memory to create memory pressure
        // This simulates the app having multiple large allocations
        let mut buffers = Vec::new();
        for _ in 0..8 {
            let mut buf = vec![0u8; 256 * 1024 * 1024]; // 256MB each
            // Touch the memory to ensure it's allocated
            for chunk in buf.chunks_mut(4096) {
                chunk[0] = 0xFF;
            }
            buffers.push(buf);
        }
        println!("  VRAM pressure: {}GB allocated", buffers.len() * 256 / 1024);
        buffers
    } else {
        Vec::new()
    };

    // --- Pre-generate tokens ---
    let t_pre = Instant::now();
    print!("  Pre-generate {pre_gen_count} tokens: ");
    let mut eos_hit = false;
    for i in 0..pre_gen_count {
        let next = sampler.sample(&ctx, -1);
        if next == model.token_eos() {
            println!("[EOS at {}]", i + 1);
            eos_hit = true;
            break;
        }
        batch.clear();
        batch.add(next, token_pos, &[0], true).unwrap();
        ctx.decode(&mut batch).expect("decode failed");
        token_pos += 1;
        if (i + 1) % 500 == 0 { print!("{} ", i + 1); }
    }
    if !eos_hit {
        println!("done");
    }
    println!("  Pre-gen: {} tokens in {:.1}s, pos={}", pre_gen_count, t_pre.elapsed().as_secs_f64(), token_pos);

    // --- Build large tool output for injection ---
    let large_tool_output = build_large_tool_output();

    // --- Injection rounds ---
    let mut all_passed = true;

    for round in 1..=num_rounds {
        // Build injection content
        let fake_output = if use_large_inject {
            if round == 1 {
                large_tool_output.clone()
            } else {
                // Vary the content each round to get different token patterns
                format!(
                    "<tool_response>\n{}\n\nExecution round {round}:\n\
                    Status: completed successfully\n\
                    Files modified: {}\n\
                    Lines changed: +{} -{}\n\
                    Build output: Compiling project... Done in {:.1}s\n\
                    Test results: {} passed, 0 failed\n\
                    </tool_response>",
                    &large_tool_output[16..large_tool_output.len().min(3000)],
                    round * 3, round * 45, round * 12, round as f64 * 1.7, round * 8
                )
            }
        } else if round == 1 {
            FAKE_TOOL_OUTPUT.to_string()
        } else {
            format!(
                "<tool_response>\nRound {round} result: Operation completed.\n\
                Created file /tmp/test_{round}.nim with {} lines of code.\n\
                Compilation output: Hint: used 14580 KiB (16384 KiB available)\n\
                Build time: {:.2}s\n</tool_response>",
                round * 50, round as f64 * 0.3
            )
        };

        let round_tokens = model.str_to_token(&fake_output, AddBos::Never)
            .expect("tokenize failed");

        // Level 20: Create a sub-agent context MID-GENERATION (like GPU summarizer)
        // This creates a second CUDA context while the main context is still alive
        if level >= 20 && round % 3 == 1 {
            run_mid_generation_sub_agent(backend, model);
        }

        // Simulate blocking tool execution (like real execute_command)
        if use_blocking_tool {
            let child = std::process::Command::new("cmd.exe")
                .args(["/C", "echo tool_result & ping -n 2 127.0.0.1 >nul"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn();
            if let Ok(mut c) = child {
                let _ = c.wait(); // Block ~1s like real tool execution
            }
        }

        // Level 26: Long delay before injection (simulating real tool execution time)
        // In the app, tool execution takes 1-30 seconds while CUDA context sits idle
        if level >= 26 {
            let delay_secs = 3 + (round % 5) as u64; // 3-7 seconds
            print!("    [delay {delay_secs}s: CUDA idle while tool executes] ");
            // Also run a real command during the delay (like the app does)
            let child = std::process::Command::new("cmd.exe")
                .args(["/C", &format!("ping -n {delay_secs} 127.0.0.1 >nul")])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            if let Ok(mut c) = child {
                let _ = c.wait();
            }
            println!("done");
        }

        // Inject tokens — single-token decode to match real app (command_executor.rs)
        // The real app decodes each token individually, NOT in batches.
        // Synchronize CUDA before injection (GPU may have been idle during tool exec).
        ctx.synchronize();
        let total = round_tokens.len();
        for (i, &tok) in round_tokens.iter().enumerate() {
            batch.clear();
            let is_last = i == total - 1;
            batch.add(tok, token_pos + i as i32, &[0], is_last).unwrap();
            // yield_now() between each decode — matches real app
            std::thread::yield_now();
            ctx.decode(&mut batch).expect("decode failed");
        }
        token_pos += round_tokens.len() as i32;
        sampler.accept_many(&round_tokens);

        // Same delay as app (50ms + synchronize)
        std::thread::sleep(std::time::Duration::from_millis(50));
        ctx.synchronize();

        // sample() — the critical call
        let t = Instant::now();
        let next = sampler.sample(&ctx, -1);
        let ms = t.elapsed().as_millis();

        if ms > 8000 {
            println!("  Round {round}: inject {} tokens → sample() {ms}ms ⚠️ HANG (pos={})", round_tokens.len(), token_pos);
            all_passed = false;
            break;
        }

        print!("  Round {round}: inject {} tokens → sample() {ms}ms ✓", round_tokens.len());

        // Write token data to pipe (like worker's stdout JSON writes)
        if let Some(ref pw) = pipe_writer {
            use std::io::Write;
            let json_line = format!(
                "{{\"id\":{round},\"type\":\"token\",\"data\":{{\"token_id\":{},\"pos\":{},\"text\":\"tok\"}}}}\n",
                next.0, token_pos
            );
            if let Ok(mut w) = pw.lock() {
                let _ = w.write_all(json_line.as_bytes());
                let _ = w.flush();
            }
        }

        // Generate more tokens between rounds (like the model continuing after tool output)
        if next != model.token_eos() {
            batch.clear();
            batch.add(next, token_pos, &[0], true).unwrap();
            ctx.decode(&mut batch).expect("decode failed");
            token_pos += 1;

            for _ in 0..gen_between_rounds {
                let next = sampler.sample(&ctx, -1);
                if next == model.token_eos() { break; }

                // Pipe write for every generated token (like the real worker)
                if let Some(ref pw) = pipe_writer {
                    use std::io::Write;
                    let json_line = format!(
                        "{{\"type\":\"token\",\"data\":{{\"token_id\":{},\"pos\":{}}}}}\n",
                        next.0, token_pos
                    );
                    if let Ok(mut w) = pw.lock() {
                        let _ = w.write_all(json_line.as_bytes());
                        // Don't flush every token — buffer like real app
                    }
                }

                batch.clear();
                batch.add(next, token_pos, &[0], true).unwrap();
                ctx.decode(&mut batch).expect("decode failed");
                token_pos += 1;
            }

            // Flush pipe after batch of tokens
            if let Some(ref pw) = pipe_writer {
                use std::io::Write;
                if let Ok(mut w) = pw.lock() {
                    let _ = w.flush();
                }
            }
        }
        println!(" (pos={token_pos})");
    }

    // Cleanup background tasks
    running.store(false, Ordering::Relaxed);
    for h in handles {
        let _ = h.join();
    }
    drop(rt_handle);

    let hb_count = heartbeat.load(Ordering::Relaxed);
    if hb_count > 0 {
        println!("  Background heartbeats: {hb_count}");
    }

    if all_passed {
        println!("  ✅ Level {level} PASSED — {num_rounds} rounds, no deadlock");
    }
    all_passed
}

