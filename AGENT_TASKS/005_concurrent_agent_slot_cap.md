# 005 — User-configurable cap on concurrently loaded agents (slot model)

Status: DESIGN — proposed by user 2026-09-10, not implemented

## Motivation

Today nothing deterministically limits how many models are resident at once. The intended
guard (`worker_pool.rs` auto-eviction "when capacity is tight") did not fire during agent
switching — see task 004 — leaving a 9B and a 27B co-resident and starving the GPU
(22482/23028 MiB, decode collapsed to ~0 tok/s).

A heuristic that silently fails is worse than an explicit rule. Replace "evict when things
look tight" with a **hard, user-visible slot count**.

## Proposed behaviour

A setting: **max concurrently loaded agents** (slots). Default **1**.

- **cap = 1** — activating an agent replaces whatever is loaded. Simple, safe, matches the
  common single-GPU case.
- **cap = 2** — if one slot is occupied, activating a second agent fills the free slot. A
  third activation evicts by policy.
- **cap = N** — generalises.

Eviction policy when all slots are full: **least-recently-used idle slot**. A slot that is
mid-generation must not be evicted — if every slot is busy, either queue the activation or
refuse it with a clear message. Silently killing a generating worker would surface as a
truncated/blank response, which is exactly the failure class task 001 is about.

## Important caveat — a count is necessary but not sufficient

**Slots are not equal in size.** Two 9B models fit comfortably in 24 GB; two 27B models do
not. A pure count-based cap set to 2 will still starve the GPU if both slots hold large
models — reproducing the exact bug this is meant to prevent.

So the cap must be a **ceiling combined with the VRAM fit check from task 003**, not a
replacement for it:

1. Is a slot free (or evictable)? — the cap
2. Does the incoming model actually fit in *live free* VRAM alongside what stays resident? —
   the fit check

If (1) passes but (2) fails, warn (per 003) rather than loading into a starved state.

## Implementation note — this also fixes the orphan leak

Task 004's worse defect is orphaned workers that `hard-unload` cannot reclaim, because the
pool tracks only the current handle. A slot table is precisely the registry that fixes it:
every slot owns a tracked PID, releasing a slot kills that process, and `hard-unload` sweeps
all slots. **Implementing slots properly makes the leak structurally impossible**, so 005 and
004 should be built together rather than separately.

Relevant files: `crates/llama-chat-web/src/worker_pool.rs` (slot table, eviction),
`src/web/worker/process_manager.rs` (spawn/kill ownership), plus a settings field in the
config DB and a control in the Settings UI.

## UX

- Setting lives in App Settings, near the other resource controls.
- The agents modal should show **slot occupancy** — which agents are currently loaded and
  how many slots remain — so "activating this will unload X" is predictable rather than a
  surprise.
- When an activation will evict something, say so before doing it (inline hint, not a modal —
  it's a routine action).

## Open questions

- **Do sub-agents consume slots?** The `spawn_agent` tool and sub-agent sessions reuse a
  CUDA context. A sub-agent running the *same* model should share the parent's slot, not
  claim a new one — otherwise cap=1 breaks sub-agent workflows entirely. Needs checking
  against `sub_agent.rs`.
- Should the cap be expressed in slots, or in a VRAM budget (e.g. "use up to 20 GB")? Slots
  are easier to reason about; a budget handles heterogeneous model sizes better. Slots +
  the 003 fit check is probably the right first cut.
- Should cap=1 unload eagerly on switch, or lazily on next load? Eager frees VRAM sooner but
  makes switching back slower.

## Verification

- Set cap=1, switch 9B → 27B, confirm exactly one worker holds a model and VRAM returns to
  baseline for the evicted one.
- Set cap=2, load two small models, confirm both stay resident and both answer.
- Set cap=2, try two 27B-class models, confirm the 003 fit warning fires instead of silently
  starving the GPU.
- Kill the server mid-load; confirm no orphaned worker survives (shared with task 004).
