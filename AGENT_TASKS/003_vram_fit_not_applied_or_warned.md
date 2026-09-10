# 003 — VRAM fit is calculated, then discarded; user never warned when a config won't fit

Status: OPEN — root-caused, fix not written
Found: 2026-09-10, while testing the Qwen 3.8 27B agent after the llama-cpp-rs v0.1.157 upgrade

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
