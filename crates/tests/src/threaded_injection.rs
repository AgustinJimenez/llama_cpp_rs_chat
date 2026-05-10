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
use llama_cpp_2::token::LlamaToken;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use serde_json::json;

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

fn level_description(level: u32) -> &'static str {
    match level {
        0 => "baseline (single-threaded, no concurrency)",
        1 => "tokio multi-thread runtime",
        2 => "tokio runtime + background async tasks (timers, channels)",
        3 => "tokio + background std::threads doing CPU work",
        4 => "tokio + threads + periodic model metadata reads (CUDA-adjacent)",
        5 => "tokio + threads + simulated IPC pipe readers (like app's stdin/stdout)",
        6 => "large prompt (~5K tokens with full tool JSON, like real app)",
        7 => "large prompt + real subprocess spawning (cmd.exe) during generation",
        8 => "large prompt + real subprocess + real pipe I/O (child stdin/stdout)",
        9 => "all above + SQLite DB writes during generation",
        10 => "high token count (2000+ gen tokens before injection)",
        11 => "large injection (1000+ tokens per inject, like real read_file)",
        12 => "real tool pattern: gen→large_inject→gen→large_inject (loop)",
        13 => "everything + blocking cmd.exe execution between inject cycles",
        14 => "real OS pipe blocking reads (BufReader on ChildStdout, like worker)",
        15 => "CUDA runs in child process context (subprocess with inherited pipes)",
        16 => "multiple simultaneous child processes (worker + tool subprocesses)",
        17 => "native Jinja chat template from GGUF + OpenAI tool defs (exact app prompt)",
        18 => "warmup context create/destroy before main context",
        19 => "multiple context create/destroy cycles (sub-agent simulation)",
        20 => "full app sequence: warmup + sub-agent context mid-generation",
        21 => "stdout pipe writes after every sample() (JSON IPC, like worker)",
        22 => "worker IPC pattern: stdin reader + stdout JSON writer + CUDA",
        23 => "VRAM pressure: large allocations to simulate near-limit memory",
        24 => "kitchen sink: everything combined",
        25 => "abort callback registered during CUDA compute (like app's cancel check)",
        26 => "long delays (5-10s) between gen and injection (real tool exec time)",
        27 => "stdout fd hijack (_dup2 redirect, like worker's steal_stdout_for_ipc)",
        _ => "unknown",
    }
}

/// Build a large system prompt with full tool definitions JSON, matching the real app (~5K tokens).
fn build_large_prompt() -> String {
    let tools_json = r#"

# Tools

You have access to the following tools. To use a tool, output a tool call in the format shown.

## read_file
Read a file from the filesystem. Supports text files and PDFs.
Parameters:
- path (string, required): The file path to read
- summary (boolean): If true, return a GPU-summarized version for large files
- pages (string): Page range for PDF files (e.g. "1-5")

## write_file
Write content to a file, creating directories as needed.
Parameters:
- path (string, required): The file path to write to
- content (string, required): The content to write

## edit_file
Edit a file by replacing text.
Parameters:
- path (string, required): The file path to edit
- old_text (string, required): The text to find and replace
- new_text (string, required): The replacement text

## execute_command
Execute a shell command and return its output.
Parameters:
- command (string, required): The command to execute
- background (boolean): Run in background (for servers/daemons)
- timeout (integer): Timeout in seconds (default: 120)
- summary (boolean): If true, return a GPU-summarized version for large outputs

## list_directory
List files and directories at the given path.
Parameters:
- path (string, required): The directory path to list
- recursive (boolean): List recursively
- max_depth (integer): Maximum recursion depth

## search_files
Search for text patterns in files using regex.
Parameters:
- pattern (string, required): The regex pattern to search for
- path (string): The directory to search in (default: current directory)
- file_pattern (string): Glob pattern to filter files (e.g. "*.rs")
- context_lines (integer): Number of context lines around matches

## find_files
Find files by name pattern.
Parameters:
- pattern (string, required): The glob pattern to match (e.g. "**/*.rs")
- path (string): The directory to search in
- max_results (integer): Maximum number of results

## web_search
Search the web for information.
Parameters:
- query (string, required): The search query
- num_results (integer): Number of results to return (default: 5)

## web_fetch
Fetch a web page and return its text content.
Parameters:
- url (string, required): The URL to fetch
- use_htmd (boolean): Use HTMD for better HTML-to-text conversion

## take_screenshot
Take a screenshot of the current screen.
Parameters: none

## click_screen
Click at screen coordinates.
Parameters:
- x (integer, required): X coordinate
- y (integer, required): Y coordinate
- button (string): Mouse button ("left", "right", "middle")

## type_text
Type text using the keyboard.
Parameters:
- text (string, required): The text to type

## press_key
Press a keyboard key or key combination.
Parameters:
- key (string, required): The key to press (e.g. "Enter", "Ctrl+C")

## execute_python
Execute a Python script and return its output.
Parameters:
- code (string, required): The Python code to execute
"#;

    let system_prompt = format!(
        "You are a highly capable AI coding assistant with access to tools for file manipulation, \
        command execution, web search, and desktop automation. You help users with software \
        engineering tasks including writing code, debugging, creating projects, and system \
        administration.\n\n\
        When the user asks you to do something, think step by step about the best approach, \
        then use the available tools to accomplish the task. Always explain what you're doing \
        and show relevant output.\n\n\
        Important guidelines:\n\
        - Always check if files exist before reading them\n\
        - Create directories before writing files to new paths\n\
        - Use appropriate error handling in generated code\n\
        - Prefer standard libraries over third-party dependencies when possible\n\
        - Test code after writing it\n\
        - Keep responses focused and avoid unnecessary explanations\n\
        {tools_json}"
    );

    format!(
        "<|im_start|>system\n{system_prompt}<|im_end|>\n\
        <|im_start|>user\nCreate a NimLang web app with Jester that has a CRUD for people (name, age, email). \
        Use an in-memory seq for storage. Put the project in E:/repo/tmp_project/nim_crud_test. \
        Show me how to run it.<|im_end|>\n\
        <|im_start|>assistant\n"
    )
}

fn run_level_15_as_child(model_path: &str) -> bool {
    let exe = std::env::current_exe().unwrap();
    println!("  Spawning self as child process...");
    let mut child = std::process::Command::new(&exe)
        .args([model_path, "15"])
        .env("LLAMA_TEST_CHILD", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .expect("Failed to spawn child process");

    // Read child's stdout (like the server's bridge stdout reader)
    use std::io::BufRead;
    if let Some(stdout) = child.stdout.take() {
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines() {
            if let Ok(l) = line {
                println!("  [CHILD] {l}");
            }
        }
    }

    let status = child.wait().expect("Child process failed");
    status.success()
}

fn run_level(level: u32, backend: &LlamaBackend, model: &LlamaModel, context_size: u32) -> bool {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║  Level {level}: {:<46}║", level_description(level));
    println!("╚══════════════════════════════════════════════════════╝");

    let n_ctx = NonZeroU32::new(context_size).unwrap();

    // Levels 18-20: Warmup context create/destroy before main context
    // This matches the app's system prompt warmup cycle
    if level >= 18 {
        println!("  Warmup: creating temporary context for system prompt warmup...");
        let warmup_ctx_size = NonZeroU32::new(4096).unwrap();
        let warmup_params = LlamaContextParams::default()
            .with_n_ctx(Some(warmup_ctx_size))
            .with_flash_attention_policy(1)
            .with_offload_kqv(true);
        let mut warmup_ctx = model.new_context(backend, warmup_params).expect("warmup context failed");

        // Decode a system prompt (like the app's warmup_system_prompt)
        let warmup_prompt = "<|im_start|>system\nYou are a helpful assistant.<|im_end|>\n";
        let warmup_tokens = model.str_to_token(warmup_prompt, AddBos::Always).expect("tokenize failed");
        let mut warmup_batch = LlamaBatch::new(2048, 1);
        eval_tokens(&mut warmup_ctx, &mut warmup_batch, &warmup_tokens, 0);
        warmup_ctx.synchronize();
        println!("  Warmup: {} tokens decoded, destroying context...", warmup_tokens.len());

        // Drop the warmup context — this frees CUDA memory but may leave driver state
        drop(warmup_batch);
        drop(warmup_ctx);
        println!("  Warmup: context destroyed.");
    }

    // Levels 19-20: Multiple context create/destroy cycles
    if level >= 19 {
        for cycle in 1..=3 {
            println!("  Sub-agent cycle {cycle}: creating 4K context...");
            let sub_ctx_size = NonZeroU32::new(4096).unwrap();
            let sub_params = LlamaContextParams::default()
                .with_n_ctx(Some(sub_ctx_size))
                .with_flash_attention_policy(1)
                .with_offload_kqv(true);
            let mut sub_ctx = model.new_context(backend, sub_params).expect("sub context failed");

            let sub_prompt = format!(
                "<|im_start|>system\nSummarize the following text concisely.<|im_end|>\n\
                <|im_start|>user\nCycle {cycle}: Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
                Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam.\
                <|im_end|>\n<|im_start|>assistant\n"
            );
            let sub_tokens = model.str_to_token(&sub_prompt, AddBos::Always).expect("tokenize failed");
            let mut sub_batch = LlamaBatch::new(2048, 1);
            eval_tokens(&mut sub_ctx, &mut sub_batch, &sub_tokens, 0);
            sub_ctx.synchronize();

            // Generate a few tokens in the sub-agent context
            let mut sub_sampler = LlamaSampler::chain(vec![
                LlamaSampler::temp(0.3),
                LlamaSampler::top_k(10),
                LlamaSampler::dist(42),
            ], true);
            let mut sub_pos = sub_tokens.len() as i32;
            for _ in 0..50 {
                let next = sub_sampler.sample(&sub_ctx, -1);
                if next == model.token_eos() { break; }
                sub_batch.clear();
                sub_batch.add(next, sub_pos, &[0], true).unwrap();
                sub_ctx.decode(&mut sub_batch).expect("sub decode failed");
                sub_pos += 1;
            }

            println!("  Sub-agent cycle {cycle}: generated tokens, destroying context...");
            drop(sub_sampler);
            drop(sub_batch);
            drop(sub_ctx);
        }
        println!("  All sub-agent cycles complete.");
    }

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

    // Shared state for background tasks
    let running = Arc::new(AtomicBool::new(true));
    let heartbeat = Arc::new(AtomicU64::new(0));

    // --- Start background noise based on level ---
    let mut handles: Vec<std::thread::JoinHandle<()>> = Vec::new();
    let rt_handle: Option<tokio::runtime::Runtime>;

    match level {
        0 => {
            // No concurrency
            rt_handle = None;
        }
        1 => {
            // Just tokio runtime (no background tasks)
            rt_handle = Some(
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(4)
                    .enable_all()
                    .build()
                    .unwrap()
            );
        }
        2 => {
            // Tokio runtime + background async tasks
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .enable_all()
                .build()
                .unwrap();

            let r = running.clone();
            let hb = heartbeat.clone();
            rt.spawn(async move {
                // Timer task (like the app's status polling)
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
                while r.load(Ordering::Relaxed) {
                    interval.tick().await;
                    hb.fetch_add(1, Ordering::Relaxed);
                }
            });

            let r2 = running.clone();
            rt.spawn(async move {
                // Channel task (like the app's token forwarding)
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
                let _tx = tx; // keep alive
                while r2.load(Ordering::Relaxed) {
                    tokio::select! {
                        _ = rx.recv() => {},
                        _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {},
                    }
                }
            });

            rt_handle = Some(rt);
        }
        3 => {
            // Tokio + std::threads doing CPU work
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .enable_all()
                .build()
                .unwrap();

            let r = running.clone();
            rt.spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
                while r.load(Ordering::Relaxed) {
                    interval.tick().await;
                }
            });

            // CPU-busy std::threads (like the app's command execution)
            for i in 0..2 {
                let r = running.clone();
                handles.push(std::thread::spawn(move || {
                    while r.load(Ordering::Relaxed) {
                        // Simulate CPU work (like parsing tool output, RTK filtering)
                        let mut sum: u64 = 0;
                        for j in 0..10000 {
                            sum = sum.wrapping_add(j * (i as u64 + 1));
                        }
                        std::hint::black_box(sum);
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }));
            }

            rt_handle = Some(rt);
        }
        4 => {
            // Tokio + threads + periodic model metadata reads
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .enable_all()
                .build()
                .unwrap();

            let r = running.clone();
            let hb = heartbeat.clone();
            rt.spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
                while r.load(Ordering::Relaxed) {
                    interval.tick().await;
                    hb.fetch_add(1, Ordering::Relaxed);
                }
            });

            // CPU threads
            for i in 0..2 {
                let r = running.clone();
                handles.push(std::thread::spawn(move || {
                    while r.load(Ordering::Relaxed) {
                        let mut sum: u64 = 0;
                        for j in 0..10000 { sum = sum.wrapping_add(j * (i as u64 + 1)); }
                        std::hint::black_box(sum);
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }));
            }

            // Thread that periodically reads model metadata (CUDA-adjacent)
            // In the app, the status API calls model_status() which reads cached metadata
            // while the worker thread is doing CUDA decode/sample
            let r = running.clone();
            handles.push(std::thread::spawn(move || {
                while r.load(Ordering::Relaxed) {
                    // Simulate reading model info (the app does this via IPC)
                    // This doesn't touch CUDA directly but accesses shared memory
                    std::hint::black_box(std::time::SystemTime::now());
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }));

            rt_handle = Some(rt);
        }
        5 => {
            // Tokio + threads + simulated IPC pipe readers
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .enable_all()
                .build()
                .unwrap();

            let r = running.clone();
            let hb = heartbeat.clone();
            rt.spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
                while r.load(Ordering::Relaxed) {
                    interval.tick().await;
                    hb.fetch_add(1, Ordering::Relaxed);
                }
            });

            // Simulated stdin reader (like the app's worker stdin reader)
            let r = running.clone();
            handles.push(std::thread::spawn(move || {
                let (tx, rx) = std::sync::mpsc::channel::<String>();
                // Simulate periodic commands coming in
                let r2 = r.clone();
                std::thread::spawn(move || {
                    while r2.load(Ordering::Relaxed) {
                        let _ = tx.send(format!("{{\"id\":1,\"command\":\"status\"}}"));
                        std::thread::sleep(std::time::Duration::from_millis(200));
                    }
                });
                while r.load(Ordering::Relaxed) {
                    if let Ok(_msg) = rx.recv_timeout(std::time::Duration::from_millis(100)) {
                        // Process command (like the app's worker command handler)
                        std::hint::black_box(42);
                    }
                }
            }));

            // Simulated stdout writer (like the app's worker stdout writer)
            let r = running.clone();
            handles.push(std::thread::spawn(move || {
                while r.load(Ordering::Relaxed) {
                    // Simulate writing JSON responses
                    let response = format!("{{\"id\":1,\"type\":\"status\",\"data\":{{\"ok\":true}}}}");
                    std::hint::black_box(response);
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            }));

            // CPU threads
            for i in 0..2 {
                let r = running.clone();
                handles.push(std::thread::spawn(move || {
                    while r.load(Ordering::Relaxed) {
                        let mut sum: u64 = 0;
                        for j in 0..10000 { sum = sum.wrapping_add(j * (i as u64 + 1)); }
                        std::hint::black_box(sum);
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }));
            }

            rt_handle = Some(rt);
        }
        6 => {
            // Large prompt only (no extra concurrency beyond what CUDA itself does)
            rt_handle = None;
        }
        7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 16 | 17 | 18 | 19 | 20 | 21 | 22 | 23 | 24 | 25 | 26 => {
            // Large prompt + tokio + real subprocess spawning + pipe I/O + optional DB
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .enable_all()
                .build()
                .unwrap();

            // Background tokio timer (status polling)
            let r = running.clone();
            let hb = heartbeat.clone();
            rt.spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
                while r.load(Ordering::Relaxed) {
                    interval.tick().await;
                    hb.fetch_add(1, Ordering::Relaxed);
                }
            });

            // Real subprocess spawning thread (like the app's command execution)
            let r = running.clone();
            handles.push(std::thread::spawn(move || {
                while r.load(Ordering::Relaxed) {
                    // Spawn a real cmd.exe process (like execute_command does)
                    let child = std::process::Command::new("cmd.exe")
                        .args(["/C", "echo test_output & timeout /t 1 /nobreak >nul"])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped())
                        .spawn();
                    if let Ok(mut child) = child {
                        let _ = child.wait();
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }));

            if level >= 8 {
                // Real pipe I/O thread: spawn subprocess and read its stdout line by line
                // (like the app's worker stdout reader with BufReader)
                let r = running.clone();
                handles.push(std::thread::spawn(move || {
                    use std::io::BufRead;
                    while r.load(Ordering::Relaxed) {
                        let child = std::process::Command::new("cmd.exe")
                            .args(["/C", "echo line1 & echo line2 & echo line3 & ping -n 2 127.0.0.1 >nul"])
                            .stdin(std::process::Stdio::null())
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::null())
                            .spawn();
                        if let Ok(mut child) = child {
                            if let Some(stdout) = child.stdout.take() {
                                let reader = std::io::BufReader::new(stdout);
                                for line in reader.lines() {
                                    if let Ok(l) = line {
                                        std::hint::black_box(l);
                                    }
                                }
                            }
                            let _ = child.wait();
                        }
                        std::thread::sleep(std::time::Duration::from_millis(300));
                    }
                }));
            }

            if level >= 9 {
                // SQLite DB writes during generation (like the app's conversation logger)
                let r = running.clone();
                handles.push(std::thread::spawn(move || {
                    // Create a temp SQLite database
                    let db_path = std::env::temp_dir().join("llama_test_deadlock.db");
                    let conn = match rusqlite::Connection::open(&db_path) {
                        Ok(c) => c,
                        Err(_) => return,
                    };
                    let _ = conn.execute(
                        "CREATE TABLE IF NOT EXISTS tokens (id INTEGER PRIMARY KEY, token TEXT, ts INTEGER)",
                        [],
                    );
                    let mut counter = 0u64;
                    while r.load(Ordering::Relaxed) {
                        counter += 1;
                        let _ = conn.execute(
                            "INSERT INTO tokens (token, ts) VALUES (?1, ?2)",
                            rusqlite::params![format!("tok_{counter}"), counter],
                        );
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    // Cleanup
                    let _ = std::fs::remove_file(&db_path);
                }));
            }

            if level >= 14 {
                // REAL blocking pipe reader — spawn a long-running process and keep
                // a BufReader blocked on its stdout THE ENTIRE TIME CUDA runs.
                // This matches the worker's stdin reader thread: always blocked on
                // pipe read while CUDA decode/sample runs on the main thread.
                let r = running.clone();
                handles.push(std::thread::spawn(move || {
                    use std::io::BufRead;
                    // Spawn a process that outputs slowly (one line per second)
                    // so our BufReader is BLOCKED between lines — exactly like
                    // the worker waiting for the next command on stdin.
                    let child = std::process::Command::new("cmd.exe")
                        .args(["/C", "for /L %i in (1,1,3600) do @(echo heartbeat_%i & ping -n 2 127.0.0.1 >nul)"])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                    if let Ok(mut child) = child {
                        if let Some(stdout) = child.stdout.take() {
                            let reader = std::io::BufReader::new(stdout);
                            for line in reader.lines() {
                                if !r.load(Ordering::Relaxed) { break; }
                                if let Ok(l) = line {
                                    std::hint::black_box(l);
                                }
                            }
                        }
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                }));

                // Second blocking pipe reader (simulating stdout writer thread)
                let r = running.clone();
                handles.push(std::thread::spawn(move || {
                    use std::io::BufRead;
                    let child = std::process::Command::new("cmd.exe")
                        .args(["/C", "for /L %i in (1,1,3600) do @(echo output_%i & ping -n 3 127.0.0.1 >nul)"])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                    if let Ok(mut child) = child {
                        if let Some(stdout) = child.stdout.take() {
                            let reader = std::io::BufReader::new(stdout);
                            for line in reader.lines() {
                                if !r.load(Ordering::Relaxed) { break; }
                                if let Ok(l) = line {
                                    std::hint::black_box(l);
                                }
                            }
                        }
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                }));
            }

            if level >= 16 {
                // Multiple simultaneous child processes — like the worker having
                // multiple tool execution subprocesses running at the same time.
                for i in 0..3 {
                    let r = running.clone();
                    handles.push(std::thread::spawn(move || {
                        use std::io::BufRead;
                        while r.load(Ordering::Relaxed) {
                            let child = std::process::Command::new("cmd.exe")
                                .args(["/C", &format!(
                                    "echo child_{i}_start & dir /s /b C:\\Windows\\System32\\*.dll 2>nul | find /c \"dll\" & echo child_{i}_done"
                                )])
                                .stdin(std::process::Stdio::null())
                                .stdout(std::process::Stdio::piped())
                                .stderr(std::process::Stdio::null())
                                .spawn();
                            if let Ok(mut child) = child {
                                if let Some(stdout) = child.stdout.take() {
                                    let reader = std::io::BufReader::new(stdout);
                                    for line in reader.lines() {
                                        if let Ok(l) = line {
                                            std::hint::black_box(l);
                                        }
                                    }
                                }
                                let _ = child.wait();
                            }
                            std::thread::sleep(std::time::Duration::from_millis(200));
                        }
                    }));
                }
            }

            rt_handle = Some(rt);
        }
        15 => {
            // Run as child process — handled at top of main(), but we
            // need the model_path to pass to the child. Use a workaround.
            rt_handle = None;
            // This level is handled specially — see run_level_15_as_child()
        }
        _ => {
            rt_handle = None;
        }
    }

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
    print!("  Pre-generate {} tokens: ", pre_gen_count);
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
            println!("    [sub-agent] Creating 4K summarizer context mid-generation...");
            let sub_ctx_size = NonZeroU32::new(4096).unwrap();
            let sub_params = LlamaContextParams::default()
                .with_n_ctx(Some(sub_ctx_size))
                .with_flash_attention_policy(1)
                .with_offload_kqv(true);
            let mut sub_ctx = model.new_context(backend, sub_params).expect("sub context failed");

            let sub_prompt = "<|im_start|>system\nSummarize briefly.<|im_end|>\n<|im_start|>user\nSummarize this tool output for the main conversation.<|im_end|>\n<|im_start|>assistant\n";
            let sub_tokens = model.str_to_token(sub_prompt, AddBos::Always).expect("tokenize failed");
            let mut sub_batch = LlamaBatch::new(512, 1);
            eval_tokens(&mut sub_ctx, &mut sub_batch, &sub_tokens, 0);
            sub_ctx.synchronize();

            // Generate summary tokens
            let mut sub_sampler = LlamaSampler::chain(vec![
                LlamaSampler::temp(0.3),
                LlamaSampler::top_k(10),
                LlamaSampler::dist(42),
            ], true);
            let mut sub_pos = sub_tokens.len() as i32;
            for _ in 0..30 {
                let next = sub_sampler.sample(&sub_ctx, -1);
                if next == model.token_eos() { break; }
                sub_batch.clear();
                sub_batch.add(next, sub_pos, &[0], true).unwrap();
                sub_ctx.decode(&mut sub_batch).expect("sub decode failed");
                sub_pos += 1;
            }
            println!("    [sub-agent] Done, destroying sub-context...");
            drop(sub_sampler);
            drop(sub_batch);
            drop(sub_ctx);
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
            print!("    [delay {}s: CUDA idle while tool executes] ", delay_secs);
            // Also run a real command during the delay (like the app does)
            let child = std::process::Command::new("cmd.exe")
                .args(["/C", &format!("ping -n {} 127.0.0.1 >nul", delay_secs)])
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
        println!(" (pos={})", token_pos);
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

/// Build prompt using the model's native Jinja chat template with OpenAI tool definitions.
/// This matches exactly what the app does — extract template from GGUF, render via minijinja.
fn build_jinja_prompt(model: &LlamaModel) -> Result<String, String> {
    // Get the chat template from the model's GGUF metadata
    let template = model.chat_template(None)
        .map_err(|e| format!("No chat template in model: {e}"))?;
    let template_str = template.to_string().map_err(|e| format!("Template UTF-8 error: {e}"))?;

    // Preprocess for minijinja (same as app's preprocess_template)
    let processed = template_str
        .replace(".endswith(", " is endingwith(")
        .replace(".startswith(", " is startingwith(")
        .replace(".strip()", " | trim")
        .replace(".upper()", " | upper")
        .replace(".lower()", " | lower");

    // Build tool definitions in OpenAI function format (matching the app's get_available_tools_openai)
    let tools: Vec<serde_json::Value> = vec![
        json!({"type": "function", "function": {
            "name": "read_file", "description": "Read a file from the filesystem",
            "parameters": {"type": "object", "properties": {
                "path": {"type": "string", "description": "The file path to read"},
                "summary": {"type": "boolean", "description": "Return GPU-summarized version"},
                "pages": {"type": "string", "description": "Page range for PDF files"}
            }, "required": ["path"]}
        }}),
        json!({"type": "function", "function": {
            "name": "write_file", "description": "Write content to a file",
            "parameters": {"type": "object", "properties": {
                "path": {"type": "string", "description": "The file path to write to"},
                "content": {"type": "string", "description": "The content to write"}
            }, "required": ["path", "content"]}
        }}),
        json!({"type": "function", "function": {
            "name": "execute_command", "description": "Execute a shell command",
            "parameters": {"type": "object", "properties": {
                "command": {"type": "string", "description": "The command to execute"},
                "background": {"type": "boolean", "description": "Run in background"},
                "timeout": {"type": "integer", "description": "Timeout in seconds"},
                "summary": {"type": "boolean", "description": "Return summarized output"}
            }, "required": ["command"]}
        }}),
        json!({"type": "function", "function": {
            "name": "list_directory", "description": "List files in a directory",
            "parameters": {"type": "object", "properties": {
                "path": {"type": "string", "description": "Directory path"},
                "recursive": {"type": "boolean", "description": "List recursively"},
                "max_depth": {"type": "integer", "description": "Max recursion depth"}
            }, "required": ["path"]}
        }}),
        json!({"type": "function", "function": {
            "name": "search_files", "description": "Search for text patterns in files",
            "parameters": {"type": "object", "properties": {
                "pattern": {"type": "string", "description": "Regex pattern to search"},
                "path": {"type": "string", "description": "Directory to search"},
                "file_pattern": {"type": "string", "description": "Glob filter"},
                "context_lines": {"type": "integer", "description": "Context lines"}
            }, "required": ["pattern"]}
        }}),
        json!({"type": "function", "function": {
            "name": "find_files", "description": "Find files by name pattern",
            "parameters": {"type": "object", "properties": {
                "pattern": {"type": "string", "description": "Glob pattern"},
                "path": {"type": "string", "description": "Directory to search"},
                "max_results": {"type": "integer", "description": "Max results"}
            }, "required": ["pattern"]}
        }}),
        json!({"type": "function", "function": {
            "name": "web_search", "description": "Search the web for information",
            "parameters": {"type": "object", "properties": {
                "query": {"type": "string", "description": "Search query"},
                "num_results": {"type": "integer", "description": "Number of results"}
            }, "required": ["query"]}
        }}),
        json!({"type": "function", "function": {
            "name": "web_fetch", "description": "Fetch a web page as text",
            "parameters": {"type": "object", "properties": {
                "url": {"type": "string", "description": "URL to fetch"},
                "use_htmd": {"type": "boolean", "description": "Use HTMD converter"}
            }, "required": ["url"]}
        }}),
        json!({"type": "function", "function": {
            "name": "execute_python", "description": "Execute Python code",
            "parameters": {"type": "object", "properties": {
                "code": {"type": "string", "description": "Python code to execute"}
            }, "required": ["code"]}
        }}),
        json!({"type": "function", "function": {
            "name": "take_screenshot", "description": "Take a screenshot of the screen",
            "parameters": {"type": "object", "properties": {}, "required": []}
        }}),
    ];

    // Build messages
    #[derive(serde::Serialize)]
    struct Msg { role: String, content: String }

    let messages = vec![
        Msg {
            role: "system".into(),
            content: "You are a helpful AI coding assistant with access to tools for file \
                manipulation, command execution, web search, and desktop automation. \
                You help users with software engineering tasks. Use the available tools \
                to accomplish tasks. Always explain what you're doing.".into(),
        },
        Msg {
            role: "user".into(),
            content: "Create a NimLang web app with Jester that has a CRUD for people \
                (name, age, email). Use an in-memory seq for storage. Put the project \
                in E:/repo/tmp_project/nim_crud_test. Show me how to run it.".into(),
        },
    ];

    // Render with minijinja
    let mut env = minijinja::Environment::new();
    env.add_function("raise_exception", |msg: String| -> Result<String, minijinja::Error> {
        Err(minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, msg))
    });
    env.add_function("strftime_now", |_fmt: String| -> String {
        "2026-05-09".to_string()
    });
    env.add_filter("tojson", |val: minijinja::Value| -> String {
        // Convert minijinja Value to JSON string
        serde_json::to_string(&val).unwrap_or_else(|_| format!("{val}"))
    });

    env.add_template("chat_template", &processed)
        .map_err(|e| format!("Template parse error: {e}"))?;

    let ctx = minijinja::context! {
        messages => &messages,
        tools => &tools,
        documents => Vec::<serde_json::Value>::new(),
        add_generation_prompt => true,
        available_tools => &tools,
        bos_token => "",
        eos_token => "<|im_end|>",
        enable_thinking => false,
    };

    let template = env.get_template("chat_template")
        .map_err(|e| format!("Get template error: {e}"))?;

    template.render(&ctx)
        .map_err(|e| format!("Render error: {e}"))
}

/// Build a large tool output (~1000+ tokens) simulating a real read_file result.
fn build_large_tool_output() -> String {
    let mut output = String::from("<tool_response>\n");
    // Simulate a real file content (Nim source code)
    output.push_str(r#"import jester, json, strutils, sequtils, os

type
  Person = object
    id: int
    name: string
    age: int
    email: string

var
  people: seq[Person] = @[]
  nextId: int = 1

proc findPerson(id: int): int =
  for i, p in people:
    if p.id == id:
      return i
  return -1

proc toJson(p: Person): JsonNode =
  %*{"id": p.id, "name": p.name, "age": p.age, "email": p.email}

proc toJson(ps: seq[Person]): JsonNode =
  var arr = newJArray()
  for p in ps:
    arr.add(p.toJson())
  return arr

routes:
  get "/":
    resp "Welcome to People CRUD API"

  get "/people":
    resp Http200, $people.toJson(), "application/json"

  get "/people/@id":
    let idx = findPerson(parseInt(@"id"))
    if idx >= 0:
      resp Http200, $people[idx].toJson(), "application/json"
    else:
      resp Http404, """{"error": "Person not found"}""", "application/json"

  post "/people":
    try:
      let body = parseJson(request.body)
      let person = Person(
        id: nextId,
        name: body["name"].getStr(),
        age: body["age"].getInt(),
        email: body["email"].getStr()
      )
      inc nextId
      people.add(person)
      resp Http201, $person.toJson(), "application/json"
    except:
      resp Http400, """{"error": "Invalid JSON body"}""", "application/json"

  put "/people/@id":
    let idx = findPerson(parseInt(@"id"))
    if idx >= 0:
      try:
        let body = parseJson(request.body)
        if body.hasKey("name"): people[idx].name = body["name"].getStr()
        if body.hasKey("age"): people[idx].age = body["age"].getInt()
        if body.hasKey("email"): people[idx].email = body["email"].getStr()
        resp Http200, $people[idx].toJson(), "application/json"
      except:
        resp Http400, """{"error": "Invalid JSON body"}""", "application/json"
    else:
      resp Http404, """{"error": "Person not found"}""", "application/json"

  delete "/people/@id":
    let idx = findPerson(parseInt(@"id"))
    if idx >= 0:
      people.delete(idx)
      resp Http204, "", "application/json"
    else:
      resp Http404, """{"error": "Person not found"}""", "application/json"

# HTML template for the web interface
const htmlTemplate = """
<!DOCTYPE html>
<html>
<head>
    <title>People CRUD</title>
    <style>
        body { font-family: Arial, sans-serif; max-width: 800px; margin: 0 auto; padding: 20px; }
        table { width: 100%; border-collapse: collapse; margin: 20px 0; }
        th, td { border: 1px solid #ddd; padding: 8px; text-align: left; }
        th { background-color: #4CAF50; color: white; }
        tr:nth-child(even) { background-color: #f2f2f2; }
        .btn { padding: 5px 10px; margin: 2px; cursor: pointer; border: none; border-radius: 3px; }
        .btn-edit { background-color: #2196F3; color: white; }
        .btn-delete { background-color: #f44336; color: white; }
        .btn-add { background-color: #4CAF50; color: white; padding: 10px 20px; font-size: 16px; }
        form { background: #f9f9f9; padding: 20px; border-radius: 5px; margin: 20px 0; }
        input { padding: 8px; margin: 5px 0; width: 100%; box-sizing: border-box; }
        label { font-weight: bold; }
    </style>
</head>
<body>
    <h1>People Management</h1>
    <div id="people-list"></div>
    <button class="btn btn-add" onclick="showForm()">Add Person</button>
    <div id="form-container" style="display:none">
        <form onsubmit="savePerson(event)">
            <input type="hidden" id="person-id">
            <label>Name:</label><input type="text" id="name" required>
            <label>Age:</label><input type="number" id="age" required>
            <label>Email:</label><input type="email" id="email" required>
            <button type="submit" class="btn btn-add">Save</button>
            <button type="button" class="btn" onclick="hideForm()">Cancel</button>
        </form>
    </div>
    <script>
        async function loadPeople() {
            const res = await fetch('/people');
            const people = await res.json();
            const html = '<table><tr><th>ID</th><th>Name</th><th>Age</th><th>Email</th><th>Actions</th></tr>' +
                people.map(p => '<tr><td>'+p.id+'</td><td>'+p.name+'</td><td>'+p.age+'</td><td>'+p.email+'</td>' +
                '<td><button class="btn btn-edit" onclick="editPerson('+p.id+')">Edit</button> ' +
                '<button class="btn btn-delete" onclick="deletePerson('+p.id+')">Delete</button></td></tr>').join('') +
                '</table>';
            document.getElementById('people-list').innerHTML = html;
        }
        loadPeople();
    </script>
</body>
</html>
"""
"#);
    output.push_str("\n</tool_response>");
    output
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
