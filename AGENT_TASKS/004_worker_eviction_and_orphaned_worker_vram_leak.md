# 004 — Worker eviction doesn't fire, and orphaned workers leak VRAM that hard-unload can't reclaim

Status: FIXED (both defects) — verified 2026-09-10, not yet committed
Found: 2026-09-10, while testing the Qwen 3.8 27B agent

## Outcome

| | before | after |
|---|---|---|
| Eviction on agent switch | never fired | fires (`[WORKER] Model unloaded`) |
| `hard-unload` with an agent worker resident | 11820 → ~11000 MiB, process survives | 11820 → **577 MiB** (baseline) |

Fixes, both verified end to end on a 24 GB card:

- **A** — `worker_pool.rs`: added `free_gpu_vram_bytes()` (nvidia-smi `memory.free`) and
  reworked `evict_to_fit()` to normalise both strategies to `(needed, available)`, with the
  GPU path starting from *real free VRAM*. Orphans and unrelated GPU processes are therefore
  accounted for implicitly. Non-NVIDIA machines keep the old modelled path. Also extracted
  `vram_estimate()` (the keep-worker case needed the same math).
- **B** — `lifecycle.rs` + `src/web/http_dispatch.rs`: `handle_post_model_hard_unload` now
  also takes the pool and calls the new `WorkerPool::kill_named_workers()`, reporting
  `named_workers_killed` in the response.

Still open from this area: task 005's slot cap (makes eviction *deterministic* rather than
budget-driven, and is the better long-term shape).

## Symptom

Switching agents (Qwen 9B → Qwen 3.8 27B) left **both** models resident:

```
PID 11964:  30.72 GB   <- Qwen 9B worker, should have been evicted
PID 12116:  30.62 GB   <- Qwen 3.8 27B worker
PID 23536:   0.30 GB   <- server
```

VRAM sat at 22482 / 23028 MiB (97.6%), starving the newly-loaded model and causing the
near-zero decode speed investigated in task 003.

## Two distinct defects

### A. Eviction did not fire on agent switch

AGENTS.md documents the intended behaviour:

> *Worker memory eviction: On memory-constrained machines, two co-resident model copies OOM
> the GPU — surfaces as "Decode Error -3" mid-decode. `crates/llama-chat-web/src/worker_pool.rs`
> auto-evicts idle workers before loading a new model when capacity is tight. Hooked into
> `spawn_worker_with_options` and `handle_post_model_load`.*

That did not happen here. The 9B worker stayed alive and resident while the 27B loaded.

**Root cause identified** — `evict_to_fit()` in `crates/llama-chat-web/src/worker_pool.rs:193-261`
decides whether memory is "tight" from a *modelled estimate*, never from actual GPU state:

- **Capacity uses TOTAL VRAM, not FREE VRAM** (`worker_pool.rs:198-204`,
  `total_gpu_vram_bytes()`). Anything consuming VRAM outside the pool is invisible.
- **Residency is inferred by summing tracked workers' file sizes** scaled by
  `gpu_layers/block_count` (`worker_pool.rs:217-239`). Only workers still in `list_entries()`
  are counted — so an **orphaned worker (defect B) contributes 0 to the estimate** even
  while holding 20 GB. Other GPU consumers (browser, other apps — Chrome and an Unreal
  editor were both on this GPU) are likewise invisible.
- Result: `new_size + resident <= budget` at `worker_pool.rs:242` returns early and evicts
  nothing, while real VRAM is already exhausted. **The budget is computed against a fiction.**

The two defects compound viciously: an orphan is invisible to the accounting, so the
accounting says there's room, so nothing is evicted, so the next model starves.

**The fix is already written and unused.** `get_available_vram_gb()` in
`crates/llama-chat-engine/src/vram_calculator.rs:43-65` queries `nvidia-smi
--query-gpu=memory.free` — i.e. real live free VRAM — and is marked `#[allow(dead_code)]`
with a TODO to integrate it. Using free VRAM as ground truth makes eviction robust to
orphans, other GPU apps, and estimation error simultaneously.

### B. Orphaned workers survive `hard-unload` — the worse bug

`POST /api/model/hard-unload` is the documented escape hatch ("kill worker process, reclaim
all VRAM"). It reported success:

```
{"success":true,"message":"Worker process killed, memory reclaimed"}
```

…but VRAM only fell 22482 → 21590 MiB (~900 MB). PID 12116 was still alive holding ~30 GB
working set. The server had already moved on to a freshly-spawned worker (PID 24436), so the
old process was **orphaned** — no longer tracked by the pool, therefore unreachable by
hard-unload.

Recovering it required killing the PID by hand:

```
Stop-Process -Id 12116 -Force     # VRAM 21590 -> 14742 MiB
```

**Root cause — narrower than "orphaning" suggests.** The pool does *not* lose handles: every
removal path (`remove_worker`, `evict_named_worker`) correctly calls `entry.bridge.kill()`.
The actual bug was that `handle_post_model_hard_unload`
(`crates/llama-chat-web/src/routes/model/lifecycle.rs`) received a single
`SharedWorkerBridge` and **never consulted the pool**, so it could only ever reclaim the
default worker while reporting that all memory was reclaimed. Any agent or overflow worker
holding a model was untouched and unreachable by any API call.

Telling detail: the sibling endpoint `handle_post_model_unload` (soft unload) *already*
swept every pool worker — the multi-worker fix had been applied there and missed here.

## Fix direction

1. **Find the orphaning window.** Determine how a worker becomes untracked — likely a
   spawn/replace path that overwrites the tracked handle without killing the previous child
   (candidates: `process_manager.rs`, `worker_pool.rs` around `spawn_worker_with_options`).
   Whatever replaces a worker handle must kill the old process first, or transfer ownership.
2. **Make hard-unload authoritative.** It should reclaim *any* worker child this server
   spawned, not just the currently-tracked one — e.g. track every spawned PID in a registry
   and sweep it, rather than trusting a single handle. Note the constraint from CLAUDE.md:
   never kill by process name (it would kill unrelated processes) — sweep by recorded PID.
3. **Kill on startup too.** The server already logs `[SERVER] Killed previous instance (PID …)`
   for itself; the same treatment should extend to stale worker children left by a crash.
4. **Surface it.** Free VRAM should be visible where it matters (see task 003's warning) so a
   leaked worker shows up as "only 7 GB free" instead of silently degrading the next model.

## Verification

- Switch agents 9B → 27B and confirm exactly one worker holds a model afterwards.
- Call `hard-unload` and confirm `nvidia-smi` returns to baseline, not a partial drop.
- Kill the server mid-load and confirm no orphaned `llama_chat_web.exe --worker` survives.

## Related

Task 003 — the VRAM starvation this caused was misattributed to context size at first; both
defects compounded. The task-003 warning would have made this leak visible to the user.

Task 005 — proposes replacing defect A's failed "capacity is tight" heuristic with an
explicit user-configurable slot cap. Its slot table is also the PID registry defect B needs,
so **005 and this task should be implemented together**: done properly, slots make the
orphan leak structurally impossible rather than something hard-unload has to chase.
