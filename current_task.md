# Current Task: C++ exception / deadlock during tool token injection

## Status: WORKAROUND ACTIVE — `RESTART_AFTER_TOOL_RESULT = true`

## The Bug

After a tool call completes, injecting the tool response tokens into the live context
via `decode()` causes either:
1. **C++ exception** (`0xE06D7363`) — caught by safe_wrapper.cpp try/catch
2. **Deadlock** in `sample()` — caught by watchdog (10s timeout, kills worker)

Both happen non-deterministically after 1-12 tool injections per conversation.

## Active Workaround

`RESTART_AFTER_TOOL_RESULT = true` in `token_loop.rs`:
- After each tool call, stop generation with `finish_reason = "tool_continue"`
- Tool result is saved to DB as part of the conversation
- Frontend auto-continues on a **fresh context** (full prompt re-evaluation)
- Tradeoff: ~2-5s pause per tool call (prompt re-eval), but zero crashes

## Root Causes Found

### 1. Grammar sampler desync (C++ exception)
- The tool grammar sampler's `accept()` was never called on injected tokens
- After injection, the grammar's internal state was out of sync with the context
- When `sample()` ran, the grammar encountered `<think>` token in a corrupted state
- Exception: `"Unexpected empty grammar stack after accepting piece: <think> (248068)"`
- **Fix attempted**: `sampler.accept_many(injected_tokens)` after injection
- **Result**: Fixed the exception, but deadlock still happens

### 2. Deadlock in sample() (10s watchdog kill)
- `sample()` hangs indefinitely inside C/CUDA code
- Not an exception — the safe wrapper catches nothing
- Not reproducible in test binary with identical tokens
- Happens even with grammar sampler disabled
- Happens even with `accept_many` fix applied
- Cause unknown — possibly CUDA stream contention or KV cache corruption

## What We Tried (and results)

| # | What | Result |
|---|------|--------|
| 1 | `context.synchronize()` before/after injection | ❌ Still crashes |
| 2 | `catch_unwind` around decode() | ❌ Doesn't catch C++ exceptions on MSVC |
| 3 | Safe C++ wrapper (`safe_wrapper.cpp`) for decode() | ✅ Catches exception, returns error |
| 4 | Safe C++ wrapper for sample() | ✅ Catches exception, returns -1 |
| 5 | Logits check before sample() | ❌ Crash is in decode/sample, not logits |
| 6 | KV cache reuse between turns | ❌ Deadlock happens with both fresh and reused cache |
| 7 | q8_0 KV cache instead of TurboQuant | ❌ Same crash |
| 8 | Disable tool grammar sampler | ✅ Fixes C++ exception but deadlock remains |
| 9 | `sampler.accept_many()` on injected tokens | ✅ Fixes grammar desync but deadlock remains |
| 10 | Watchdog thread (10s heartbeat) | ✅ Detects deadlock, kills worker |
| 11 | Windows SEH crash handler | ✅ Catches exception code, controlled exit(42) |
| 12 | Bridge auto-restart on worker death | ✅ Worker restarts, app stays open |
| 13 | Exact token replay in test binary | ❌ Always passes (non-deterministic) |
| 14 | RESTART_AFTER_TOOL_RESULT workaround | ✅ Avoids injection crashes but causes stats flicker, message splitting, lost responses |
| 15 | Different seeds with same conversation | ❌ Crash is random regardless of seed |
| 16 | Crash during normal generation (no injection) | ❌ Also crashes with 0 injections — not injection-specific |

## Key Findings

### 2026-04-30
1. The crash is **NOT specific to tool injection** — also happens during normal generation.
2. **NOT seed-dependent** — same seed crashes sometimes, works other times.
3. **NOT TurboQuant-specific** — crashes with f16 KV cache too (confirmed `kv_cache=f16` in logs).
4. **NOT model-specific** — crashes with both Qwen3.6-35B (MoE+SSM) and Qwen3.5-9B (dense).
5. **NOT injection-size dependent** — crashes with 45-token, 101-token, and 1000+ token injections.

### Exact crash location
- Always in `llama_sampler_sample()` — the C++ function inside llama.cpp
- Last log: `Sampling token N (i=0) ...` — then function never returns
- `decode()` before it completes successfully (no error, no exception)
- Safe C++ wrapper catches nothing — it's NOT an exception, it's a true hang/deadlock
- The watchdog detects it after 10s and kills the worker process

### Call chain
```
token_loop.rs → sampler.sample(context, -1)
  → sampling.rs → llama_sampler_sample_safe() (C++ wrapper)
    → llama_sampler_sample() in llama-sampler.cpp
      → HANGS FOREVER (no exception, no return)
```

## Crash instances (parameters at time of crash)

| # | Model | KV Cache | Context | Flash Attn | GPU Layers | Injection tokens | Seed |
|---|-------|----------|---------|------------|------------|-----------------|------|
| 1 | Qwen3.6-35B-A3B-UD-IQ4_XS | turbo2/turbo3 | 119040 | true | 40/40 | 1338, 729, 1046 | -1 |
| 2 | Qwen3.6-35B-A3B-UD-IQ4_XS | q8_0/q8_0 | 103424 | true | 40/40 | 1264, 552 | -1 |
| 3 | Qwen3.6-35B-A3B-UD-IQ4_XS | f16/f16 | 60416 | true | 40/40 | 729, 729 | -1 |
| 4 | Qwen3.5-9B-Q8_0 | f16/f16 | 60416 | true | ? | ? | -1 |
| 5 | Qwen3.6-35B-A3B-UD-IQ4_XS | turbo2/turbo3 | 118528 | true | 40/40 | 294, 248 | -1 |
| 6 | Qwen3.6-35B-A3B-UD-IQ4_XS | turbo2/turbo3 | 137472 | true | 40/40 | 46, 297, 101 | -1 |
| 7 | Qwen3.6-35B-A3B-UD-IQ4_XS | turbo2/turbo3 | 137472 | true | 40/40 | 0 (no injection) | -1 |

All crashes: `llama_sampler_sample()` hangs after successful `decode()`. Watchdog kills after 10s.

## Investigation to continue

1. **Attach C++ debugger** — Use Visual Studio to attach to the worker process, break when hung, get stack trace inside `llama_sampler_sample`
2. **`compute-sanitizer --tool memcheck`** — NVIDIA CUDA memory checker
3. **Test with upstream llama.cpp** — Rule out TurboQuant fork as cause
4. **Test with Gemma-4 or Devstral** — Different model family (non-Qwen)
5. **Update CUDA drivers** — Newer drivers may fix
6. **Report to llama.cpp** — If reproducible on upstream, file an issue

## Key Files

- `crates/llama-chat-engine/src/token_loop.rs` — `RESTART_AFTER_TOOL_RESULT` flag, watchdog, accept_many
- `crates/llama-chat-engine/src/command_executor.rs` — `inject_output_tokens()`, synchronize, dump logging
- `crates/llama-chat-engine/src/sampler.rs` — Grammar sampler (currently disabled)
- `crates/llama-chat-engine/src/tool_grammar.rs` — GBNF grammar definition
- `crates/llama-chat-worker/src/worker/worker_main.rs` — Crash handler (SEH)
- `crates/llama-chat-worker/src/worker/worker_bridge.rs` — Auto-restart, IO reconnection
- `deps/llama-cpp-rs/llama-cpp-sys-2/safe_wrapper.cpp` — C++ try/catch for decode/sample
- `deps/llama-cpp-rs/llama-cpp-2/src/context.rs` — decode() using llama_decode_safe
- `deps/llama-cpp-rs/llama-cpp-2/src/sampling.rs` — sample() using llama_sampler_sample_safe
- `crates/tests/src/main.rs` — Reproduction test (always passes)

## Dump Files

After each crash, written to `{LLAMA_CHAT_DATA_DIR}/logs/`:
- `last_prompt_dump.txt` — Full formatted prompt
- `last_inject_dump.txt` — Injection entries with token IDs
- `last_gen_tokens.txt` — Every generated token ID

## Build Notes

- Fast release: `lto=false`, `codegen-units=4`, `incremental=true` (~16s rebuilds)
- Working on `develop` branch — merge to `master` when stable
- CDP debugging on port 9222
- Test: `npm run cargo -- run --release --features cuda,vision -p llama-chat-tests`
