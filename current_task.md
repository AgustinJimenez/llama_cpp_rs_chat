# Current Task: CUDA deadlock during tool token injection

## Status: LIKELY FIXED — needs more testing to confirm

## The Bug

After a tool call completes, injecting the tool response tokens into the live context
via `decode()` causes either:
1. **C++ exception** (`0xE06D7363`) — caught by safe_wrapper.cpp try/catch
2. **Deadlock** in `sample()` — caught by watchdog (10s timeout, kills worker)

Both happen non-deterministically after 1-12 tool injections per conversation.

## Current Status

Three bugs were fixed simultaneously on 2026-05-06, and the deadlock appears resolved:
1. **CUDA graphs disabled** (`GGML_CUDA_GRAPHS=OFF` in build.rs)
2. **KV cache type enum values corrected** (turbo2→44, turbo3→42, turbo4→43 — were off by one)
3. **Default KV cache types fixed** (`SamplerConfig::Default` was turbo2/turbo3, now f16)

### Test results after all 3 fixes:
- **9B dense**: 102+ injections, 0 deadlocks (nimlang CRUD + PDF summaries)
- **35B MoE**: 61+ injections, 0 deadlocks (nimlang CRUD, direct injection, NO safe mode)

### Uncertainty: which fix actually matters?

The earlier "CUDA graphs OFF" test (documented as ❌ in the table below) was done **before** the KV cache enum fix. At that time, the KV cache was using `Q1_0` (type 41) instead of `TURBO3_0` (type 42) due to the off-by-one bug. So the previous test was tainted.

We cannot isolate which fix resolved the 35B deadlock without further testing:
- **Hypothesis A**: CUDA graphs OFF is the fix (graph caching becomes stale after injection)
- **Hypothesis B**: Correct KV cache types is the fix (Q1_0 type caused CUDA backend confusion)
- **Hypothesis C**: Both were needed together

To confirm, would need to re-enable CUDA graphs with correct KV types and test (~30 min CUDA rebuild).

### `safe_tool_injection` setting added

A user-facing toggle was added as a fallback: Settings → KV Cache → "Safe Tool Inject". When ON, stops and restarts context after each tool call (slower but guarantees no deadlock). Tested: 90+ tool_continues on 35B with zero deadlocks. Default: OFF.

### Safety nets (still active regardless)
- Watchdog (10s timeout) detects hang and kills worker
- Bridge auto-restarts worker process  
- Frontend auto-reloads model after crash
- `cuda_deadlock` is in AUTO_CONTINUE_REASONS (frontend auto-continues)

### System prompt display bug (minor)
After compaction, the system prompt renders as a full yellow warning box instead of the collapsible `▸ System prompt` widget. Cause: ConversationWatcher overwrites messages and loses the `isSystemPrompt` flag. Not fixed yet.

### Stop button on empty conversation (minor)
The Stop button shows on a new empty conversation while another conversation is generating. Cause: `isLoading` is global, not per-conversation. Not fixed yet.

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

### 2026-05-06 — Architecture-specific deadlock confirmed
1. **Qwen3.5-9B (dense)**: 102+ tool injections across 3 heavy tasks (nimlang CRUD, PDF chapter summaries), 20+ minutes continuous generation, **ZERO deadlocks**.
2. **Qwen3.6-35B-A3B (hybrid MoE+recurrent)**: deadlocks after **3 injections** consistently.
3. **CUDA graphs disabled** (`GGML_CUDA_GRAPHS=OFF`): no effect on 35B deadlock, but eliminates some instability.
4. **Root cause narrowed**: the deadlock is specific to the **hybrid MoE architecture** (attention + Gated Delta Net recurrent layers). The `ggml_backend_sched_synchronize()` hangs when switching between attention and recurrent layer backends after token injection.

### Bugs fixed (2026-05-06)
- **SamplerConfig default KV cache types**: was `turbo2`/`turbo3`, now `f16`. The wrong defaults caused `SET_ROWS` CUDA crash on model load because turbo enum values in `context_eval.rs` were off by one (41/42/43 → 42/43/44).
- **Build fix**: upstream llama.cpp renamed `common` library to `llama-common`.
- **CUDA graphs disabled**: `GGML_CUDA_GRAPHS=OFF` in build.rs. Doesn't fix the MoE deadlock but eliminates a source of graph caching issues after token injection.

### 2026-04-30 — Initial findings (some now CORRECTED)
1. The crash is **NOT specific to tool injection** — also happens during normal generation.
2. **NOT seed-dependent** — same seed crashes sometimes, works other times.
3. ~~NOT model-specific — crashes with both 35B and 9B~~ **CORRECTED 2026-05-06**: 9B (dense) does NOT deadlock. Only 35B (hybrid MoE) deadlocks. Earlier 9B crashes were caused by wrong KV cache types (turbo enum off-by-one), not the deadlock.
4. **NOT injection-size dependent** — crashes with 45-token, 101-token, and 1000+ token injections.

### Exact crash location
- Always in `llama_sampler_sample()` — the C++ function inside llama.cpp
- Last log: `Sampling token N (i=0) ...` — then function never returns
- `decode()` before it completes successfully (no error, no exception)
- Safe C++ wrapper catches nothing — it's NOT an exception, it's a true hang/deadlock
- The watchdog detects it after 10s and kills the worker process

### Call chain
```
crates/llama-chat-engine/src/token_loop.rs:294
  → let next_token = sampler.sample(context, -1);
    → deps/llama-cpp-rs/llama-cpp-2/src/sampling.rs:39
      → llama_sampler_sample_safe() in deps/llama-cpp-rs/llama-cpp-sys-2/safe_wrapper.cpp
        → llama_sampler_sample() in deps/llama-cpp-rs/llama-cpp-sys-2/llama.cpp/src/llama-sampler.cpp:806
          → llama_get_sampled_token_ith() at line 807
            → ctx->synchronize() at llama-context.cpp:3139
              → ggml_backend_sched_synchronize() → HANGS (CUDA never signals completion)
```

### C++ instrumentation result (2026-05-01)
Added `fprintf(stderr)` at each step inside `llama_sampler_sample()`:
- Last successful log: `step=6_accept (token=13378)` — Nth call completed all 6 steps
- The (N+1)th call **never logged step=1** — hangs at entry before `llama_get_sampled_token_ith()`
- `llama_get_sampled_token_ith()` calls `ctx->synchronize()` internally
- **Root cause: CUDA synchronization deadlock — GPU never signals completion**

### Possible hang locations inside `llama_sampler_sample()` (llama-sampler.cpp:806-873)
- **Line 807-810**: `llama_get_sampled_token_ith()` etc. — calls `ctx->synchronize()` internally
- **Line 849**: `llama_get_logits_ith(ctx, idx)` — logits retrieval
- **Line 864**: `llama_sampler_apply(smpl, &cur_p)` — applies the sampler chain (penalties, temp, top_k, top_p, dist)
- **Line 866**: `GGML_ASSERT(cur_p.selected >= 0)` — assert after apply

### How to debug the C++ hang

**Option 1: Visual Studio (attach to process)**
1. Build with debug symbols: set `strip = false` in `[profile.release]` (already set)
2. Launch the app normally (`npm run tauri:dev:release`)
3. Load the model, start a conversation that triggers tool calls
4. Open Visual Studio → Debug → Attach to Process → select `llama_chat_app.exe` (the worker PID)
5. When the hang occurs (watchdog hasn't killed it yet — increase `WATCHDOG_TIMEOUT_MS` to 60000 first)
6. Click "Break All" in Visual Studio
7. Check the call stack — it will show exactly where inside `llama_sampler_sample` the thread is stuck

**Option 2: WinDbg**
1. `windbg -p <worker_pid>` or attach via WinDbg UI
2. When hung, press Ctrl+Break
3. `!analyze -v` for crash analysis
4. `~*k` to see all thread stacks

**Option 3: CUDA compute-sanitizer**
```
compute-sanitizer --tool memcheck target/release/llama_chat_app.exe --worker --db-path ...
```
Detects illegal GPU memory access that could cause the hang.

**Prep: increase watchdog timeout for debugging**
In `token_loop.rs`, change `WATCHDOG_TIMEOUT_MS` from `10_000` to `60_000` or more so the debugger has time to break in before the watchdog kills the process.

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

## Upstream llama.cpp issues (2026-05-01)

### [#21383](https://github.com/ggml-org/llama.cpp/issues/21383) — Qwen3.5-27B CUDA illegal memory access (OPEN)
- Same pattern: agentic tool-call + CUDA sync hang
- Caused by commit `744c0c731` (PR #21038) — Hadamard activation rotation
- Workaround: `LLAMA_ATTN_ROT_DISABLE=1` — already disabled in our fork
- **We still crash even with rotation disabled** → different root cause

### [PR #22534](https://github.com/ggml-org/llama.cpp/pull/22534) — Hybrid model memory wipe (CLOSED, not merged)
- `llama_memory_seq_rm()` fails on hybrid models (Qwen3.5/3.6)
- Code then "destructively wipes ALL cached state" including valid checkpoints
- This corrupted KV cache could cause CUDA sync deadlocks on next operation
- PR was closed (AI content policy), fix not in upstream

### Key insight
**Hybrid/recurrent architectures (Qwen3.5/3.6 with Gated DeltaNet) have known memory management bugs in llama.cpp's CUDA backend.** The deadlock may be from internal seq_rm/memory operations corrupting the KV cache, not from our injection code.

## Latest attempts (2026-05-01)

| # | What | Result |
|---|------|--------|
| 17 | CUDA graphs disabled (GGML_CUDA_GRAPHS=OFF) | ❌ Still deadlocks |
| 18 | Windows TDR timeout 60s (registry) | ❌ Still deadlocks |
| 19 | Timed sample() on detached C++ thread (8s timeout) | ❌ CUDA deadlock freezes ALL threads including the polling thread |
| 20 | Auto-reload model from frontend | ✅ Frontend detects loaded→unloaded, auto-reloads last model |
| 21 | LLAMA_ATTN_ROT_DISABLE=1 env var | ❌ Already disabled in fork, still deadlocks |

### Key discovery: CUDA deadlock freezes ALL process threads
The timed sample approach (running `sample()` on a detached thread with polling timeout) failed because the CUDA deadlock blocks ALL threads in the process — not just the one calling CUDA. Only the watchdog works because it uses `process::exit(42)` which is handled by the OS, not CUDA.

## Current safety net
1. Watchdog detects deadlock after 10s → `process::exit(42)`
2. Bridge auto-restarts worker process
3. Frontend detects model unloaded → auto-reloads last known model (~30s)
4. Generation can auto-continue after reload

## Investigation to continue

1. **Test with Gemma-4 or Devstral** — Confirm if non-Qwen models have the same issue
2. **Test with upstream llama.cpp** — Rule out TurboQuant fork
3. **Attach C++ debugger** (Visual Studio) — Break when hung, get stack trace
4. **`compute-sanitizer --tool memcheck`** — NVIDIA CUDA memory checker
5. **Update CUDA drivers** — Newer drivers may fix
6. **Report to llama.cpp** — File an issue with our reproduction data
7. **Expose browser tools as Tauri commands** — Allow external control of in-app browser

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
