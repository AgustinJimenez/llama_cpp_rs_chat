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
| 14 | RESTART_AFTER_TOOL_RESULT workaround | ✅ Avoids injection entirely, no crashes |

## Investigation to continue later

1. **Why does `sample()` deadlock?** — Not an exception, not caught by safe wrapper. Possibly CUDA stream issue specific to Qwen3.6 hybrid MoE+SSM + TurboQuant fork.
2. **Can the grammar handle injected tokens?** — With `accept_many` the grammar is synced, but the deadlock persists. Try re-enabling grammar + accept_many without the restart workaround on a different model.
3. **Is it model-specific?** — Only tested with Qwen3.6-35B-A3B. Test with Devstral, Gemma-4, or a dense model.
4. **Is it TurboQuant-specific?** — Test with f16 KV cache (not just q8_0).
5. **CUDA memory checker** — Run with `compute-sanitizer --tool memcheck` to detect illegal memory access during injection.
6. **Update llama.cpp fork** — The TurboQuant fork may have fixes for this.

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
