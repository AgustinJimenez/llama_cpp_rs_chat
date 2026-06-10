use std::io::BufRead;
use std::num::NonZeroU32;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;

use super::helpers::eval_tokens;

pub(super) const FAKE_TOOL_OUTPUT: &str = r#"<tool_response>
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

pub(super) const SYSTEM_PROMPT: &str = r#"You are a helpful AI assistant with access to tools. You can read files, write files, execute commands, and search the web to help the user.

When the user asks you to do something, use the available tools to accomplish the task. Always explain what you're doing and why.

Available tools:
- read_file: Read a file from the filesystem
- write_file: Write content to a file
- execute_command: Run a shell command
- list_directory: List files in a directory
- search_files: Search for text patterns in files
- web_search: Search the web for information"#;

pub(super) fn level_description(level: u32) -> &'static str {
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

pub(super) fn run_level_15_as_child(model_path: &str) -> bool {
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

    if let Some(stdout) = child.stdout.take() {
        let reader = std::io::BufReader::new(stdout);
        for l in reader.lines().filter_map(|r| r.ok()) {
            println!("  [CHILD] {l}");
        }
    }

    let status = child.wait().expect("Child process failed");
    status.success()
}

pub(super) fn run_warmup_cycles(
    level: u32,
    backend: &LlamaBackend,
    model: &LlamaModel,
) {
    if level >= 18 {
        println!("  Warmup: creating temporary context for system prompt warmup...");
        let warmup_ctx_size = NonZeroU32::new(4096).unwrap();
        let warmup_params = LlamaContextParams::default()
            .with_n_ctx(Some(warmup_ctx_size))
            .with_flash_attention_policy(1)
            .with_offload_kqv(true);
        let mut warmup_ctx = model
            .new_context(backend, warmup_params)
            .expect("warmup context failed");

        let warmup_prompt = "<|im_start|>system\nYou are a helpful assistant.<|im_end|>\n";
        let warmup_tokens = model
            .str_to_token(warmup_prompt, AddBos::Always)
            .expect("tokenize failed");
        let mut warmup_batch = LlamaBatch::new(2048, 1);
        eval_tokens(&mut warmup_ctx, &mut warmup_batch, &warmup_tokens, 0);
        warmup_ctx.synchronize();
        println!(
            "  Warmup: {} tokens decoded, destroying context...",
            warmup_tokens.len()
        );

        drop(warmup_batch);
        drop(warmup_ctx);
        println!("  Warmup: context destroyed.");
    }

    if level >= 19 {
        for cycle in 1..=3 {
            println!("  Sub-agent cycle {cycle}: creating 4K context...");
            let sub_ctx_size = NonZeroU32::new(4096).unwrap();
            let sub_params = LlamaContextParams::default()
                .with_n_ctx(Some(sub_ctx_size))
                .with_flash_attention_policy(1)
                .with_offload_kqv(true);
            let mut sub_ctx = model
                .new_context(backend, sub_params)
                .expect("sub context failed");

            let sub_prompt = format!(
                "<|im_start|>system\nSummarize the following text concisely.<|im_end|>\n\
                <|im_start|>user\nCycle {cycle}: Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
                Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam.\
                <|im_end|>\n<|im_start|>assistant\n"
            );
            let sub_tokens = model
                .str_to_token(&sub_prompt, AddBos::Always)
                .expect("tokenize failed");
            let mut sub_batch = LlamaBatch::new(2048, 1);
            eval_tokens(&mut sub_ctx, &mut sub_batch, &sub_tokens, 0);
            sub_ctx.synchronize();

            let mut sub_sampler = LlamaSampler::chain(
                vec![
                    LlamaSampler::temp(0.3),
                    LlamaSampler::top_k(10),
                    LlamaSampler::dist(42),
                ],
                true,
            );
            let sub_base = sub_tokens.len() as i32;
            for i in 0..50_i32 {
                let next = sub_sampler.sample(&sub_ctx, -1);
                if next == model.token_eos() {
                    break;
                }
                sub_batch.clear();
                sub_batch.add(next, sub_base + i, &[0], true).unwrap();
                sub_ctx.decode(&mut sub_batch).expect("sub decode failed");
            }

            println!("  Sub-agent cycle {cycle}: generated tokens, destroying context...");
            drop(sub_sampler);
            drop(sub_batch);
            drop(sub_ctx);
        }
        println!("  All sub-agent cycles complete.");
    }
}

pub(super) fn run_mid_generation_sub_agent(
    backend: &LlamaBackend,
    model: &LlamaModel,
) {
    println!("    [sub-agent] Creating 4K summarizer context mid-generation...");
    let sub_ctx_size = NonZeroU32::new(4096).unwrap();
    let sub_params = LlamaContextParams::default()
        .with_n_ctx(Some(sub_ctx_size))
        .with_flash_attention_policy(1)
        .with_offload_kqv(true);
    let mut sub_ctx = model
        .new_context(backend, sub_params)
        .expect("sub context failed");

    let sub_prompt = "<|im_start|>system\nSummarize briefly.<|im_end|>\n<|im_start|>user\nSummarize this tool output for the main conversation.<|im_end|>\n<|im_start|>assistant\n";
    let sub_tokens = model
        .str_to_token(sub_prompt, AddBos::Always)
        .expect("tokenize failed");
    let mut sub_batch = LlamaBatch::new(512, 1);
    eval_tokens(&mut sub_ctx, &mut sub_batch, &sub_tokens, 0);
    sub_ctx.synchronize();

    let mut sub_sampler = LlamaSampler::chain(
        vec![
            LlamaSampler::temp(0.3),
            LlamaSampler::top_k(10),
            LlamaSampler::dist(42),
        ],
        true,
    );
    let sub_base = sub_tokens.len() as i32;
    for i in 0..30_i32 {
        let next = sub_sampler.sample(&sub_ctx, -1);
        if next == model.token_eos() {
            break;
        }
        sub_batch.clear();
        sub_batch.add(next, sub_base + i, &[0], true).unwrap();
        sub_ctx.decode(&mut sub_batch).expect("sub decode failed");
    }
    println!("    [sub-agent] Done, destroying sub-context...");
    drop(sub_sampler);
    drop(sub_batch);
    drop(sub_ctx);
}
