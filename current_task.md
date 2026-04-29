# Current Task: CUDA sample() crash after tool injection

## Status: IN PROGRESS — Safety net implemented, root cause unresolved

## The Bug

`sampler.sample(context, -1)` crashes (segfault) or deadlocks after injecting tool response tokens into the context mid-generation. This happens non-deterministically — the same prompt + injection data passes in isolated tests but crashes in the live app.

## Symptoms

- Model generates tokens normally, calls a tool (browser_search, browser_get_text, etc.)
- Tool executes successfully, response tokens are injected into the context via `inject_output_tokens()`
- Next call to `sampler.sample()` either:
  - **Segfaults** (worker process dies instantly, no logs)
  - **Deadlocks** (sample() never returns, watchdog kills worker after 10s)
- Happens after 1-12 tool injections per conversation, non-deterministic
- More likely with longer conversations / more tool calls

## What We Know

1. **Not reproducible in isolation (yet)**: Test binary (`crates/tests/`) replays exact prompt + 12 injection token sequences from a real crash — all pass. BUT the test was using a **different sampler config** (missing penalties, top_k, tool_grammar). The generated tokens between injections are also different each run.
2. **Not related to KV cache reuse**: Happens with both fresh context (drop cache each turn) and reused cache
3. **Not related to TurboQuant KV types**: Test with TURBO2_0/TURBO3_0 KV cache passes
4. **Not a race condition**: `context.synchronize()` after injection doesn't help
5. **Non-deterministic**: Depends on actual tokens generated between injections (different each run)
6. **Model-specific?**: Only tested with Qwen3.6-35B-A3B (hybrid MoE+SSM). Might affect other models too.

## Root Cause FOUND (2026-04-29)

**Exception code 0xE06D7363 = C++ exception**, NOT a segfault!

`llama-context.cpp:74` throws: `"the backend samplers must be of type llama_sampler_chain"`

This means the sampler object passed to `sample()` is not a valid `llama_sampler_chain`. This could happen if:
- The sampler is corrupted after many sample() calls
- The Rust wrapper creates a sampler type that llama.cpp doesn't recognize as a chain
- The sampler is freed/moved while still in use

The crash is non-deterministic because the corruption accumulates over time.

## Previous Theory (WRONG)

~~Memory corruption in llama.cpp's CUDA code that accumulates over multiple `decode()` → `sample()` cycles with injected tokens.~~ Possibly related to:
- KV cache state corruption with TurboQuant (TURBO2_0=43, TURBO3_0=41) types
- CUDA graph caching issues (warmup/reset during mid-generation injection)
- Flash attention buffer management with hybrid MoE+SSM architecture

## Safety Net (Implemented)

1. **Watchdog thread** (`token_loop.rs`): Monitors heartbeat timestamp updated after each successful `sample()`/`decode()`. If no heartbeat for 10s, calls `process::exit(42)` to kill the worker.
2. **Bridge auto-restart** (`worker_bridge.rs`): When the bridge detects worker death (stdout reader exits), it:
   - Clears active generation (stops spinner)
   - Sends error message to UI
   - Fails pending requests
   - Restarts worker process via `ProcessManager::restart()`
   - Reconnects stdin/stdout IO on a new thread runtime
3. **User experience**: Generation stops, "[Worker crashed — restarting]" appears, model unloads, user reloads model and retries.

## Key Files

- `crates/llama-chat-engine/src/token_loop.rs` — Watchdog thread, heartbeat, sample()/decode() calls
- `crates/llama-chat-engine/src/command_executor.rs` — `inject_output_tokens()`, `synchronize()` call, dump logging
- `crates/llama-chat-engine/src/generation.rs` — Prompt dump to `last_prompt_dump.txt`
- `crates/llama-chat-worker/src/worker/worker_bridge.rs` — Bridge death detection, auto-restart, IO reconnection
- `crates/tests/src/main.rs` — Reproduction test (currently passes)
- `crates/tests/test_data_prompt.txt` — Real prompt from crash
- `crates/tests/test_data_inject.txt` — Real injection tokens from crash (12 entries)

## Dump Files (for reproduction)

After each crash, the app writes:
- `{LLAMA_CHAT_DATA_DIR}/logs/last_prompt_dump.txt` — Full formatted prompt
- `{LLAMA_CHAT_DATA_DIR}/logs/last_inject_dump.txt` — All injection entries with `[INJECT pos=N count=M] [token_ids...]`
- `{LLAMA_CHAT_DATA_DIR}/logs/last_gen_tokens.txt` — Every generated token ID (one per line), for exact replay

Default data dir: `C:\Users\agus_\AppData\Roaming\com.llamachat.desktop`

## To Investigate

1. **C++ debugger**: Attach to worker process, set breakpoint on `llama_sampler_sample`, reproduce crash, get stack trace
2. **CUDA memcheck**: Run with `compute-sanitizer --tool memcheck` to detect illegal memory access
3. **Reduce to minimal case**: Find the specific token sequence that triggers the crash (generate deterministically with temp=0, fixed seed)
4. **Test with different models**: Try a non-MoE model (e.g. Gemma-4-31B) to see if it's architecture-specific
5. **Test without TurboQuant**: Use f16 KV cache to rule out TURBO type corruption
6. **Update llama.cpp**: Check if newer llama.cpp versions fix the issue (related issues: #18310, #19219, #22320)

## Related llama.cpp Issues

- [#18310](https://github.com/ggml-org/llama.cpp/issues/18310) — Race condition in decode(): missing synchronize() after async tensor copy
- [#19219](https://github.com/ggml-org/llama.cpp/issues/19219) — MoE model inference hang on NVIDIA
- [#22320](https://github.com/ggml-org/llama.cpp/issues/22320) — Low GPU utilization with Qwen3.6-35B-A3B MoE+SSM hybrid

## Build Notes

- Fast release profile: `lto=false`, `codegen-units=4`, `incremental=true` (~16s rebuilds)
- Test crate: `npm run cargo -- run --release --features cuda,vision -p llama-chat-tests`
- CDP debugging on port 9222 (auto-enabled in dev builds)
