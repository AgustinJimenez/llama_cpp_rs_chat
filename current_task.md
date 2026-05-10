# Current Task: CUDA deadlock during tool token injection

## Status: MITIGATED — injection delay + headless crash recovery (upstream bug, llama.cpp #21383)

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

## 2026-05-07 — Injection mitigation + headless crash recovery

### Pattern confirmed
The crash happens specifically at `sample()` **right after** `decode()` processes injected tool output tokens. Normal generation (no injection) never crashes. The crash is 100% reproducible with the PDF task (crashes on first `read_file` injection at 100K context).

### Injection mitigation (partial fix)
Three changes together improved stability significantly:

| Change | File | Purpose |
|--------|------|---------|
| Smaller injection chunks (512→128) + sync after each | `command_executor.rs` | Prevents CUDA async state accumulation |
| 100ms sleep after injection | `token_loop.rs` | Lets GPU driver flush operations |
| Warm decode (re-decode last token) before sample() | `token_loop.rs` | Forces CUDA compute graph re-sync |

**Results**: PDF task went from crashing in <30s to surviving 4+ minutes. NimLang CRUD task ran 10 min uninterrupted. Not a fix but dramatically reduces crash frequency.

### Headless crash recovery (backend)
Moved crash recovery from frontend React hooks to Rust backend:
- `CrashRecoveryCtx` persists `model_path`, `gpu_layers`, `conversation_id` across crash cycles
- After worker death: auto-restart → auto-reload model → auto-continue conversation
- Up to `MAX_AUTO_RECOVERY_CRASHES=5` retries before giving up
- Each cycle produces more output (conversation grows in DB)
- PDF task: 31 chars (before) → 12,925 chars (after persistent recovery)

### RTK always-on
Removed `use_rtk` config toggle. RTK prefix always applied to commands.
Pipeline: RTK (instant CPU filter, 60-90% reduction) → GPU summarizer (if >1500 chars).

### Test results (Qwen3.6-35B-A3B at 100K context)

| Task | Before mitigations | After mitigations |
|------|-------------------|-------------------|
| NimLang CRUD | 9 min, 75K chars, crash at end | 10 min, 75K chars, crash at end |
| PDF Summary | <30s, 31 chars, immediate crash | 4+ min, 13K chars, crash after progress |
| Simple generation (no tools) | Never crashes | Never crashes |

### Files changed
- `crates/llama-chat-worker/src/worker/worker_bridge.rs` — `CrashRecoveryCtx`, headless auto-reload/continue
- `crates/llama-chat-engine/src/command_executor.rs` — Smaller chunks (128), sync per chunk
- `crates/llama-chat-engine/src/token_loop.rs` — 100ms sleep + warm decode after injection
- `src/hooks/useChat.ts` — Frontend crash message + auto-continue (still useful for browser)
- `src/hooks/useModel.ts` — Frontend crash recovery events

### Ideas to try next
1. **Even smaller chunks (32 or 16)** — More sync points, slower but might eliminate crash
2. **Single-token injection** — Decode each token individually like normal generation (very slow but guaranteed safe)
3. **Inject into a fresh batch context** — Instead of continuing the existing context, save KV cache state, create fresh context, inject, then restore
4. **`llama_kv_self_clear()` before injection** — Reset KV cache state that might be corrupted
5. **Reduce context to 32K** — The crash may be worse at 100K due to memory pressure

## 2026-05-09 — Silent crash recovery + upstream research

### Recovery pipeline bugs fixed (3 bugs)

The crash recovery was broken — the model would crash, auto-restart, but fail to actually continue:

| Bug | Cause | Fix |
|-----|-------|-----|
| Stdout reader dying | Recovery thread's `block_on` returned → `LocalSet` dropped → `spawn_local` stdout reader killed | Await the stdout reader handle to keep the recovery thread alive |
| Frontend race | Both bridge (Rust) and frontend (JS) detected crash, both sent `LoadModel` → second load destroyed first's context → 120s timeout → another kill | Added `auto_recovering: Arc<AtomicBool>` flag; `is_loading()` returns true during recovery so frontend skips its own reload; `load_model()` rejects external requests during recovery |
| Missing pending request | Auto-continue Generate sent without registering a `PendingRequest` → stdout reader couldn't match the response → "No pending request for id=910001" | Register `PendingRequest` before sending auto-continue command |

### Silent recovery (no chat messages)

Changed recovery to be invisible to the user:
- `skip_user_logging: true` on auto-continue Generate — no "[System: interrupted]" message in DB
- Don't inject crash token into stream when `will_auto_recover` is true — no "[Worker process crashed]" text
- Don't clear `ActiveGeneration` during recovery — UI keeps showing spinner
- Frontend polling reconnect picks up the resumed generation via `active_conversation_id`
- Only visible artifact: brief "Connection closed unexpectedly" toast (from SSE stream dying)

### Test results (2026-05-09)
- **PDF summary (9B)**: Crash after read_file injection → silent recovery → auto-continue → deadlocks again at same point → repeats until MAX_AUTO_RECOVERY_CRASHES (2) exceeded
- **Nim CRUD (9B)**: Crash after tool injection → silent recovery → auto-continue → **successfully generated more tokens, wrote files, made tool calls** → deadlocked again → recovered again → deadlocked at 3rd crash → gave up
- **Key finding**: The deadlock is **deterministic per conversation state** — replaying the same conversation from DB hits the same deadlock at the same point. Auto-continue only helps when the new generation changes the conversation enough to avoid the same token positions.

### `MAX_AUTO_RECOVERY_CRASHES` reduced to 2

Was 5 — but since the deadlock is deterministic per conversation, retrying 5 times on a poisoned conversation just wastes time. Now gives up after 3 total crashes (crash_count > 2).

### Upstream issues found (2026-05-09)

| Issue | Description | Status |
|-------|-------------|--------|
| [#21383](https://github.com/ggml-org/llama.cpp/issues/21383) | Qwen3.5-27B CUDA illegal memory access in agentic tool-call pattern | OPEN |
| [#22450](https://github.com/ggml-org/llama.cpp/issues/22450) | Qwen3.6-35B-A3B slot hangs in TG after cache invalidation — `llama_decode` doesn't return | OPEN |
| [#22160](https://github.com/ggml-org/llama.cpp/issues/22160) | "Deadlock by Design" — client timeout + no prompt cache reuse = infinite retry loop | OPEN |
| [#20545](https://github.com/ggml-org/llama.cpp/issues/20545) | Infinite wait on ROCm with Qwen3.5-35B-A3B, same `ggml_backend_sched_synchronize` hang | OPEN |

All issues confirm: **hybrid MoE + recurrent models (Qwen3.5/3.6) have known CUDA/ROCm synchronization bugs in llama.cpp during agentic workflows**. No upstream fix exists.

### Understanding the deadlock

The deadlock is NOT in `sample()` itself. `sample()` is trivial — it just picks a token from probability distribution. But internally it first calls `ctx->synchronize()` to wait for the GPU to finish the previous `decode()`. The GPU never signals completion because the **hybrid architecture's backend scheduler** (`ggml_backend_sched`) gets into a broken state after mid-generation token injection.

Why it's specific to hybrid models: Qwen3.5/3.6 use both **attention layers** (CUDA backend) and **Gated Delta Net recurrent layers** (different CUDA kernels). The backend scheduler must synchronize between these two compute paths. After token injection mid-generation, the scheduler's internal state becomes inconsistent, and `synchronize()` waits forever for a CUDA event that will never fire.

Why `safe_tool_injection` (restart after tool) works: instead of calling `sample()` right after mid-generation `decode(injected_tokens)`, we stop and restart. The new generation cycle does `decode(full_prompt_from_DB)` from scratch, which rebuilds the scheduler state cleanly. `sample()` then works because the GPU state is consistent.

### Workaround options to explore

1. **`safe_tool_injection` flag** (implemented, disabled by default): Stop generation after each tool injection, auto-continue with fresh context. Avoids the deadlock entirely but adds ~2-5s pause per tool call. Already in config DB, just needs wiring in token_loop.rs.

2. **Warm decode before sample**: After injection, decode a dummy/repeat token to force the backend scheduler to re-sync before calling sample(). Already partially implemented (100ms sleep + synchronize). Could try a full re-decode of the last few tokens.

3. **KV cache checkpoint/restore**: Before injection, save KV cache state. After injection, if sample() would deadlock, restore and retry with a different approach. Requires llama.cpp API support (`llama_state_save`/`llama_state_load`).

4. **Reduce injection size**: Instead of injecting full tool output (1000+ tokens), always use GPU summarization to keep injections under 100 tokens. Smaller injections may not trigger the scheduler bug.

5. **Force backend re-initialization**: After injection, call `llama_kv_self_clear()` or equivalent to reset the backend scheduler state. Destructive (loses KV cache) but might avoid the deadlock.

6. **Wait for upstream fix**: Monitor #22450 and #21383. The llama.cpp team is aware of hybrid model issues.

### 2026-05-09 — Isolated test results (16 levels, all passed)

Created `crates/tests/src/threaded_injection.rs` with 16 levels of increasing complexity:

| Level | What | Result |
|-------|------|--------|
| 0 | Single-threaded baseline | ✅ PASSED |
| 1 | Tokio multi-thread runtime | ✅ PASSED |
| 2 | + Background async tasks (timers, channels) | ✅ PASSED |
| 3 | + Background std::threads (CPU work) | ✅ PASSED |
| 4 | + Periodic model metadata reads | ✅ PASSED |
| 5 | + Simulated IPC pipe readers | ✅ PASSED |
| 6 | Large prompt (~1K tokens with tool definitions) | ✅ PASSED |
| 7 | + Real subprocess spawning (cmd.exe) | ✅ PASSED |
| 8 | + Real pipe I/O (BufReader on ChildStdout) | ✅ PASSED |
| 9 | + SQLite DB writes during generation | ✅ PASSED |
| 10 | High token count (2000+ gen tokens, pos ~4K) | ✅ PASSED |
| 11 | Large injection (1000+ tokens per inject) | ✅ PASSED |
| 12 | Real tool pattern: gen→large_inject→gen loop (pos ~13K) | ✅ PASSED |
| 13 | + Blocking cmd.exe between inject cycles | ✅ PASSED |
| 14 | Real OS pipe blocking reads (persistent BufReader) | ✅ PASSED |
| 15 | CUDA runs in child process context (subprocess) | ✅ PASSED |
| 16 | Multiple simultaneous child processes | ✅ PASSED |
| 17 | Native Jinja chat template from GGUF + OpenAI tool defs | ✅ PASSED |
| 18 | Warmup context create/destroy before main context | ✅ PASSED |
| 19 | Multiple context create/destroy cycles (sub-agent) | ✅ PASSED |
| 20 | Full app sequence: warmup + sub-agent mid-generation | ✅ PASSED |
| 21 | Stdout pipe writes after every sample() (JSON IPC) | ✅ PASSED |
| 22 | Worker IPC pattern: stdin reader + stdout writer + CUDA | ✅ PASSED |
| 23 | VRAM pressure: 2GB system RAM allocation | ✅ PASSED |
| 24 | Kitchen sink: everything combined | ✅ PASSED |
| 25 | Abort callback registered during CUDA compute | ✅ PASSED |
| 26 | Long delays (3-7s) between gen and injection | ✅ PASSED |

**Conclusion**: The deadlock CANNOT be reproduced in an isolated test. All 26 levels pass with 0-4ms `sample()` latency at positions up to 13,553 with 1000+ token injections. The test exhaustively covers every individual app component: tokio runtime, background threads, real OS pipes, real subprocess spawning, blocking I/O, SQLite writes, child process CUDA context, multiple concurrent child processes, Jinja template rendering with full OpenAI tool definitions, context create/destroy cycles, sub-agent contexts mid-generation, abort callbacks, per-token pipe writes, memory pressure, and long idle delays.

**The deadlock is an emergent behavior of the full app** — it only occurs when ALL components run together in the specific sequence the app performs. No individual component or combination of components reproduces it.

### 2026-05-09 — BREAKTHROUGH: Deadlock is in decode(), not sample()

C++ instrumentation at every level revealed the actual hang point:

**The deadlock is in `llama_decode()` during single-token injection, NOT in `llama_sampler_sample()`.**

```
[INJECT_DECODE] 551/649 pos=6583 ok     ← decode #551 succeeded
[WATCHDOG] deadlock after 10664ms        ← decode #552 never returned
```

Key findings:
1. **`ggml_backend_sched_synchronize` completes successfully** — both CUDA0 and CPU backends sync fine before the hang
2. **The hang is inside `llama_decode()`** for a single token at context position ~6583
3. **It's deterministic**: both crashes hung at exactly token #551 of 649 in the injection loop
4. **Position ~6600 is the trigger**: the KV cache position where decode hangs is consistent
5. **Previous assumption was wrong**: we thought sample() hung, but sample() was never reached — decode() in the injection loop hung first

The injection does single-token decode (one token at a time, like normal generation). Token #1-551 decode fine. Token #552 hangs forever. This suggests a **KV cache position-dependent bug** in the hybrid architecture — something about position ~6600 causes the CUDA backend to deadlock.

Possible causes:
- **Recurrent state overflow**: Qwen3.5's Gated Delta Net recurrent layers have fixed-size state buffers. Position ~6500 might exceed an internal limit.
- **Attention window boundary**: `full_attention_interval=4` means attention layers process every 4th layer. A specific position might hit a boundary condition.
- **KV cache memory layout**: The hybrid KV cache has filtered (recurrent) and non-filtered (attention) layers. Position ~6600 might cause a memory allocation/mapping issue.

### 2026-05-09 — Isolated test results (pure generation + position sweep)

- **Pure generation (10K tokens, no injection)**: PASSED at 18 tok/s, positions up to 10,044
- **Position sweep (4000-6000)**: PASSED — inject 600 tokens at each position, all OK
- **Position sweep 6200**: appeared to hang but was caused by verbose SCHED logging overhead (15x slowdown from `fflush(stderr)`)
- **26-level threaded test**: all passed (tokio, pipes, subprocesses, DB, abort callback, etc.)

**Conclusion**: The deadlock CANNOT be reproduced in any isolated test. It requires the full app's runtime — specifically the interaction between the worker's IPC loop, the generation loop, and tool execution. The `fflush(stderr)` timing perturbation can make it appear in tests but is a false positive.

### What we know for certain:
1. **`decode()` is the hang point**, not `sample()` — confirmed by instrumentation
2. **`cudaStreamSynchronize` completes OK** — the hang is AFTER CUDA sync
3. **It happens at a consistent injection token index** (#551 of 649 in two crashes)
4. **It's timing-sensitive** — adding micro-delays (fflush) can prevent or trigger it
5. **It only happens in the full app** — not reproducible in isolation despite 26 levels of testing

### Exact hang location inside llama_decode():

```
llama_context::decode()
  → sched_reserve()          ✅ OK
  → memory_update()          ✅ OK  
  → memory->init_batch()     ✅ OK
  → process_ubatch()         💀 HANGS
    → mctx->apply()          ✅ OK
    → graph reuse/build      ✅ OK
    → set_inputs()           ✅ OK
    → graph_compute()        💀 ← HERE (CUDA graph execution)
```

The hang is in `graph_compute()` — the function that actually executes the CUDA computation graph. This is the ggml backend's `ggml_backend_graph_compute()` call.

**Critical observation**: Adding `fflush(stderr)` logging at each step PREVENTS the deadlock from occurring. The microsecond delays from fprintf/fflush change CUDA kernel scheduling enough to avoid the race condition. This confirms the deadlock is an extremely narrow timing-dependent race in the CUDA backend's graph execution for hybrid (attention + recurrent) models.

### Attempted fixes at C++ level:
- `fflush(stderr)` before graph_compute: prevents deadlock ONLY when combined with many other fflush calls (timing perturbation). Single fflush insufficient.
- `ggml_backend_sched_synchronize()` before graph_compute: **no effect** — sync completes but graph_compute still hangs
- The deadlock is inside `ggml_backend_sched_graph_compute_async()` — too deep to fix without modifying ggml internals

### Next steps:
1. **Implement `safe_tool_injection`** — stop generation after tool injection, auto-continue with fresh decode. The only guaranteed fix without upstream patch.
2. **Report to llama.cpp** with exact hang location (`graph_compute_async` on hybrid models during tool injection in multi-threaded IPC app)
3. **Test with upstream llama.cpp server** — does `llama-server` also deadlock with Qwen3.5 tool injection?

## 2026-05-10 — BREAKTHROUGH: Deadlock is NOT in the generation pipeline

### Real pipeline test (`real_pipeline_test.rs`)
Created a test that calls `generate_llama_response()` directly — the exact same function
the worker process uses. This includes:
- Real tool parsing and execution (read_file, execute_command, etc.)
- Real token injection via `command_executor::inject_output_tokens()`
- Real conversation logging to SQLite
- Real sampler chain, Jinja templates, prompt building
- Auto-continue loop (tool_continue → Continue → tool_continue → ...)
- Watchdog thread, first-injection workaround, GPU summarizer sub-agent

**Result (9B model):**
- **24 rounds, 78 tool injections, 19,365 tokens generated — ZERO deadlocks**
- Round 24 had **15 sequential tool calls** in one conversation
- Round 16 had **8 tool calls** (177 seconds continuous)
- All with single-token injection matching real app path exactly

### What this proves
**The deadlock is NOT in the generation pipeline.** The generation code (token_loop,
command_executor, sampler, tool injection, sub-agent) works perfectly when called directly.

The deadlock only occurs in the **full worker process** — the IPC/threading layer. The key
differences between the passing test and the failing app:

1. **`steal_stdout_for_ipc()`** — `_dup2(2, 1)` redirects C stdout → stderr at OS level
2. **3-thread architecture** — stdin reader, main crossbeam event loop, generation thread
3. **tokio mpsc → crossbeam bridge** — `blocking_recv()` thread between channel types
4. **Pipe I/O to parent** — JSON Lines serialized and written to OS pipe every token
5. **Parent `BufReader::lines()`** — constant pipe reads from worker stdout
6. **`SetUnhandledExceptionFilter`** — Windows SEH crash handler

### Implication for fix
The fix should target the worker's IPC architecture, not the generation code. Options:
1. **Reduce pipe write frequency** — buffer tokens, write every N tokens instead of every token
2. **Decouple CUDA from pipe I/O** — ensure pipe writes never block on same OS thread as CUDA
3. **Remove crossbeam bridge** — use pure tokio channels end-to-end
4. **Async pipe writes** — use non-blocking I/O for stdout writes
5. **Move pipe writes to dedicated thread** — isolate from generation thread entirely

### Also tested: threaded injection test (27 levels)
Updated the `threaded_injection.rs` test with two critical fixes:
- **Single-token injection** (was 512-token chunks) — matches real app's `inject_output_tokens()`
- **Level 27: stdout fd hijack** — `_dup2(2, 1)` like worker's `steal_stdout_for_ipc()`

All 27 levels pass with 9B model. But a previous background run DID hang (11GB stuck process),
suggesting the deadlock is possible even in isolation, just extremely rare without the full
worker IPC pipe pressure.

## Investigation to continue

1. **Test with Gemma-4 or Devstral** — Confirm if non-Qwen models have the same issue
2. **Test with upstream llama.cpp** — Rule out TurboQuant fork
3. **Profile pipe I/O during CUDA** — measure if pipe writes block during graph_compute
4. **Reduce pipe write frequency** — buffer tokens, test if reducing I/O eliminates deadlock
5. **Test with stdio unbuffered** — `setvbuf(stdout, NULL, _IONBF, 0)` to see if buffering matters
6. **Implement `safe_tool_injection` in token_loop.rs** — Wire existing config flag as guaranteed fix

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
