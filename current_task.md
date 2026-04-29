# Current Task: C++ exception crash during decode() after tool injection

## Status: IN PROGRESS — Safety net works (auto-restart), root cause unresolved

## The Bug

`context.decode(batch)` throws a C++ exception (`0xE06D7363`) during tool response token injection. The exception propagates through the Rust FFI boundary and crashes the worker process. This happens non-deterministically — the exact same prompt + injection tokens pass in isolated tests.

## Symptoms

- Model generates tokens normally, calls a tool (browser_search, browser_get_text, etc.)
- Tool executes successfully (2-5 seconds)
- Tool response tokens are injected via `inject_output_tokens()` → `decode()`
- `decode()` throws C++ exception → worker crashes
- Happens consistently on 2nd message (after first completes fine)
- Happens with both TurboQuant (turbo2/turbo3) AND q8_0 KV cache types
- Non-deterministic: exact same data passes in test binary

## Root Cause Analysis

### Confirmed: C++ exception (NOT segfault)
- Exception code: `0xE06D7363` = Microsoft C++ exception (`msc`)
- Address: `0x7ffee53179da` (MSVC runtime `RaiseException`)
- The crash is in `decode()`, not in `sample()` as originally thought
- `catch_unwind` does NOT catch C++ exceptions on MSVC

### Chain of failure
1. `inject_output_tokens()` calls `context.decode(batch)` with tool response tokens
2. Inside llama.cpp C++ code, an exception is thrown (likely from `output_resolve_row`, ring buffer, or GGML_ASSERT)
3. Exception propagates through C FFI boundary (undefined behavior in Rust)
4. Process crashes

### Key difference between test and app
The test binary replays the exact same prompt + tokens and passes. Possible differentiators:
- **Time gap**: In the app, the GPU context sits idle for 2-5s while browser tools execute. Windows WDDM can preempt/reclaim idle GPU contexts. `synchronize()` before injection didn't help.
- **Thread context**: App runs generation in `thread::spawn`. The generation thread shares `Arc<Mutex<LlamaState>>` with the main worker thread. Test runs everything single-threaded.
- **Accumulated state**: First message's generation may leave residual state in the llama backend/CUDA that affects the second message's fresh context.

### What we tried (didn't fix)
- `context.synchronize()` before and after injection
- `catch_unwind` around decode()
- KV cache reuse vs fresh context each turn
- q8_0 KV cache instead of TurboQuant
- Exact token replay in test binary (always passes)
- Logits check before sample() (crash is in decode, not sample)

## Safety Net (Working)

1. **Crash handler** (`worker_main.rs`): Windows SEH catches exception, logs code + address, calls `process::exit(42)` for fast controlled exit
2. **Watchdog thread** (`token_loop.rs`): 10s heartbeat timeout, kills worker if sample() deadlocks
3. **Bridge auto-restart** (`worker_bridge.rs`): Detects worker death → clears generation → restarts worker → reconnects IO on new thread runtime
4. **User experience**: Generation stops, "[Worker crashed — restarting]" appears, model unloads, user reloads model and retries

## Key Files

- `crates/llama-chat-engine/src/command_executor.rs` — `inject_output_tokens()`, crash dump logging
- `crates/llama-chat-engine/src/token_loop.rs` — Watchdog thread, heartbeat, logits check
- `crates/llama-chat-engine/src/generation.rs` — Prompt dump, context creation
- `crates/llama-chat-worker/src/worker/worker_main.rs` — Crash handler (SEH), run_worker
- `crates/llama-chat-worker/src/worker/worker_bridge.rs` — Auto-restart, IO reconnection
- `crates/tests/src/main.rs` — Reproduction test (always passes)

## Dump Files

After each crash, written to `{LLAMA_CHAT_DATA_DIR}/logs/`:
- `last_prompt_dump.txt` — Full formatted prompt
- `last_inject_dump.txt` — Injection entries: `[INJECT pos=N count=M] [token_ids...]`
- `last_gen_tokens.txt` — Every generated token ID (one per line)

Default: `C:\Users\agus_\AppData\Roaming\com.llamachat.desktop`

## Next Steps to Investigate

1. **Run with `compute-sanitizer --tool memcheck`** — NVIDIA's CUDA memory checker will show illegal memory accesses
2. **Add C++ try/catch wrapper** — Create a `llama_decode_safe()` C++ function that wraps `llama_decode` in try/catch, returning error string instead of throwing
3. **Test with a different model** — Try Gemma-4-31B or Devstral to see if it's Qwen3.6-specific
4. **Test single-threaded** — Run generation on the main worker thread (not `thread::spawn`) to eliminate threading as a variable
5. **Update llama.cpp** — Check if newer versions fix the underlying throw

## Build Notes

- Fast release profile: `lto=false`, `codegen-units=4`, `incremental=true` (~16s rebuilds)
- Test crate: `npm run cargo -- run --release --features cuda,vision -p llama-chat-tests`
- CDP debugging on port 9222 (auto-enabled)
- Crash handler outputs to stderr (visible in Tauri dev console)
