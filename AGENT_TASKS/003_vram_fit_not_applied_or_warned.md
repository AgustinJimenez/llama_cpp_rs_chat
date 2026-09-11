# 003 — VRAM fit is calculated, then discarded; user never warned when a config won't fit

Status: FIXED — (a) and (b) 2026-09-10; (c) the warning 2026-09-11
Found: 2026-09-10, while testing the Qwen 3.8 27B agent after the llama-cpp-rs v0.1.157 upgrade

## Resolution of (c) — 2026-09-11

The warning now exists and was verified firing against the original scenario.

**Where the free-VRAM number comes from.** `/api/workers` now returns a `vram_gb` per
worker, computed by the *same* `vram_estimate()` the evictor uses (`worker_pool.rs`). This
was deliberate: had the modal used its own estimate, the warning and the eviction could
disagree, which is the exact shape of the bug this task is about. `useResidentModelsVram`
(new hook) polls that endpoint and sums the workers whose model differs from the one being
configured.

**The pessimism problem named in the original notes is handled by exclusion, not by
subtraction.** The model currently being reconfigured is filtered out by filename, so
reloading the resident model — the most common path — never triggers the warning.

**Three states**, in `MemoryVisualization.tsx`:
- `overcommitted` (unchanged): projected > *total* VRAM. Hard red.
- `contended` (new): fits alone, but projected + other residents > total. Red, names the
  numbers, and offers a remedy.
- a softer amber notice when other models are resident but everything still fits.

**The "use recommended" button does NOT use `optimalContextSize`.** That was the first
implementation and it was wrong in the only case that matters: `useVramOptimizer` budgets
against *total* VRAM, so under contention it recommended 256K — a value that does not fit
either. Clicking it would have changed nothing. That is the same dead-safety-net shape as
the original bug, so the button now solves for the largest context that fits the *remaining*
VRAM directly (KV cache is linear in context, so the current `(contextSize, kvCache)` pair
gives the per-token cost). When even a small context won't fit, it says so and points at
unloading instead of offering a fake fix.

**Verified live** with the 9B resident (8.9 GB) and the 27B agent open at ctx 262144:

> Won't fit alongside the models already loaded
> This config needs 21.4 GB (of which 4.8 GB is KV cache). Other loaded models are holding
> 8.9 GB, for 30.3 GB against 22.5 GB of VRAM.
> CUDA will not error — it pages to system RAM and generation slows to roughly zero
> tokens/sec. Unload the other model, or reduce the context size.
> *No usable context size fits in the VRAM left over. Unload another model, or reduce GPU layers.*

Before this change that same screen showed no warning at all.

**Still open:** the `Qwen 3.8 27B` agent remains stored at `context_size = 262144`. The
warning now makes that visible when the agent is edited, but nothing migrates it.

## Progress (historical — (a) and (b))

**Done — (a) + (b): the optimizer's context is now applied.**
`model-config/index.tsx` no longer takes the GGUF `context_length` as a default. The
model-max seed clamps to `optimized.optimalContextSize` once the optimizer is ready, and the
auto-apply effect now sets the context too (delivering what its comment already promised).
Both respect `savedConfigLoaded` — an explicit user config still wins.

**Two important limits of that fix:**

1. **Existing saved agents are NOT migrated.** The `Qwen 3.8 27B` agent is still stored at
   `context_size = 262144` and will still starve VRAM until changed. Overriding a saved
   user value was deliberately left alone (the guard is pre-existing and intentional), so
   fixing already-broken agents needs either a manual edit or an explicit migration
   decision.
2. It prevents *new* misconfigurations only, which is why (c) still matters.

**Still open — (c) the warning. Now much cheaper than first thought:**

The warning **already exists** — `MemoryVisualization.tsx:184` renders
`t('memoryVisualization.overcommitted')` in red, driven by `vram.overcommitted` from
`useMemoryCalculation` (`useMemoryCalculation.ts:239`).

It never fired because it compares projected usage against **total** VRAM:
`model-config/index.tsx:87` aliases `totalVramGb` as `availableVramGb`. In the failing case
~20.4 GB projected vs 22.49 GB *total* is not "overcommitted" — even though another model
was already holding ~7 GB, so it genuinely did not fit.

The missing input is already available: `useSystemResources()` exposes `usage.vram_used_gb`
(`SystemResourcesContext.tsx:26`), so free VRAM is `totalVramGb - usage.vram_used_gb` with no
context change required.

**The one design care-point:** free VRAM is pessimistic when the user is simply
*reconfiguring the model that is currently loaded* — its own VRAM counts as "used" but will
be released on reload. Naively budgeting against free VRAM there would cry wolf on the most
common path, and a warning that fires spuriously gets ignored. Either subtract the
outgoing model's footprint, or keep `overcommitted` (vs total) as the hard red error and add
a distinct softer amber notice for "N GB is currently in use by other models" — the latter is
probably the honest framing since we can't always know what will be evicted.

## Symptom

Loading the `Qwen 3.8 27B` agent (IQ4_XS, 15.7 GB weights) on a 24 GB card produced a
model that loads and processes the prompt fine but decodes at effectively **zero tok/s** —
64 s of generation with not a single token rendered, GPU pinned at 100%, VRAM at
22482 / 23028 MiB (97.6%).

Critically this is **not** an OOM. Per AGENTS.md: *"context_size MUST fit in VRAM — CUDA VMM
silently pages to RAM if oversubscribed → 70ms/tok instead of 5ms."* The user gets no error,
no warning, nothing — just a chat that appears to hang forever.

## Root cause — three layers, all in `src/components/organisms/model-config/index.tsx`

**1. The optimizer's context result is computed and thrown away.** `useVramOptimizer`
returns `optimalContextSize` (declared in the type at `src/hooks/useVramOptimizer.ts:23`,
returned from all six code paths). The auto-apply effect at `index.tsx:306-320` says:

```js
// Auto-apply VRAM-optimized gpu_layers and context_size once per model
setConfig((prev) => ({ ...prev, gpu_layers: optimized.optimalGpuLayers }));
```

The comment promises `context_size`. The code only applies `gpu_layers`.
`optimized.optimalContextSize` is never read anywhere in the file.

**2. A separate effect overwrites context with the model's native max** (`index.tsx:220-230`):

```js
if (modelInfo?.context_length) {
  const maxContext = parseInt(modelInfo.context_length.toString().replaceAll(',', ''));
  if (!isNaN(maxContext)) setContextSize(maxContext);   // 262144 for this model
}
```

So the resolution order ends up: preset (`32768`) → optimizer (VRAM-safe value) → **both
overwritten by model max (`262144`)**.

**3. The memory estimate is advisory only.** The modal renders a full breakdown (Model GPU /
KV Cache / Model CPU, plus VRAM and RAM bars via `useMemoryCalculation`). It will happily
display 97% VRAM and let the config save with no warning and no confirmation.

Confirmed in the DB — the agent was saved with the model max, not the preset:

```
Qwen 9B      | ctx  32768 | f16    | f16
Qwen 3.8 27B | ctx 262144 | turbo2 | turbo3     <- modelPresets.ts says 32768
```

## Fix direction

**a. Apply what we already compute.** In the `index.tsx:306` effect, also set
`context_size: optimized.optimalContextSize` — honouring the existing precedence (a user's
explicitly saved config should still win; this is for first-load/auto).

**b. Make the model-max effect VRAM-aware.** It should clamp to the optimizer's value
rather than blindly taking `context_length` from GGUF metadata. Model max is a *ceiling*,
not a default.

**c. Warn when it still won't fit — required, not optional.** Even with (a) and (b), the
optimum can exceed what's actually free. Use **live free VRAM**, not total: the check must
reflect other resident models (see task 004 — an orphaned worker was holding ~7 GB, so even
32K would have been tight).

Suggested states, driven off the numbers `useMemoryCalculation` already produces:

| Projected / free VRAM | State | Behaviour |
|---|---|---|
| < ~85% | OK | current display |
| ~85-100% | Tight | amber inline warning near the context slider |
| > 100% | Won't fit | red warning + explicit "this will page to RAM and generate at ~0 tok/s" |

The warning should name the cause and the remedy concretely, e.g.
*"Context 262144 needs 4.6 GB of KV cache. Projected 22.9 / 23.0 GB VRAM. Reduce context to
32768 (recommended) or expect severe slowdown."* — ideally with a one-click "Use recommended"
that applies `optimalContextSize`.

Do **not** hard-block saving; the user may know something we don't. Warn clearly and let
them proceed.

## Why this matters more than a normal UX gap

Every other resource mistake in this app surfaces as an error. This one surfaces as an
apparent hang, which is the hardest failure mode to diagnose — it cost most of a debugging
session to trace, and the numbers needed to prevent it were already on screen the whole time.

## Verification

- Create an agent for `Qwen3.8-27B-IQ4_XS.gguf` on a 24 GB card; confirm the context lands on
  the preset/optimizer value, not 262144.
- Force a too-large context manually; confirm the warning appears and names both the KV cache
  cost and the recommended value.
- With a second model resident, confirm the warning reacts to *free* VRAM, not total.
