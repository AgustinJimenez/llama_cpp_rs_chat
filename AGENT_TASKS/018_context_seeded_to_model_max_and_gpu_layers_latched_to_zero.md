# 018 — Context seeded to the model maximum, and GPU layers latched to 0

Status: FIXED 2026-09-12 — root cause identified from live state; needs UI confirmation
Found: 2026-09-12 by the user, testing the freshly installed desktop app

## Symptom

Opening the Local Model modal for `Qwen3.8-27B-IQ4_XS.gguf` showed:

```
Model (GPU): 0.00 GB      Model (CPU): 14.60 GB
GPU Memory (VRAM): 0.00 / 22.49 GB (0.0%)
GPU Layers: 0 / 65        Context: 256K
```

The user's reasonable reading was *"does this mean CUDA is not detected?"* — which is
exactly how it looks.

## CUDA was fine — ruled out first

`GET /api/backends` on the running app:

```json
{"cuda_backend_loaded": true, "nvidia_gpu_detected": true,
 "backends":[{"name":"CUDA","available":true,
   "devices":[{"name":"CUDA0","description":"NVIDIA GeForce RTX 4090","vram_mb":23027}]}]}
```

Loaded-module inspection of the running process confirms it for real:

```
ggml-cuda.dll      …\target\release\build\llama-cpp-sys-2-…\out\backends\ggml-cuda.dll
cudart64_12.dll    C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.8\bin\…
cublas64_12.dll    …\v12.8\bin\…
cublasLt64_12.dll  …\v12.8\bin\…
```

(Incidentally this proves the model in [[017_consume_published_llama_cpp_backends_instead_of_compiling_cuda_in]]:
the NVIDIA libraries came from the host's own toolkit, nothing shipped.)

Model metadata was also complete and correct — `block_count 65`,
`attention_head_count 24`, `attention_head_count_kv 4`, `embedding_length 5120`, 51 GGUF
keys. The optimizer had everything it needed.

## Root cause — two ordering bugs, same shape

**1. The context seed wrote the GGUF ceiling as a provisional value.**

`model-config/index.tsx` seeded context from `modelInfo.context_length`, using the raw
maximum whenever `optimized.ready` was false:

```ts
setContextSize(optimized.ready ? Math.min(maxContext, optimized.optimalContextSize) : maxContext);
```

`optimized.ready` is false while `maxLayers` is still 0 — a real window before path
validation finishes. So 262144 got written. By the time the optimizer was ready and the
effect re-ran, the async `fetchSavedConfig()` had set `savedConfigLoaded.current = true`
**unconditionally at its end**, so the `if (savedConfigLoaded.current) return;` guard bailed
and the ceiling stuck.

The guard exists to protect a value the *user* chose. Here it protected a value nothing
chose.

**2. GPU layers were latched to 0 before VRAM was known.**

`useVramOptimizer` returns `ready: true` with `optimalGpuLayers: 0` when
`availableVramGb <= 0` — it cannot distinguish "VRAM not fetched yet" from "no GPU".
`SystemResourcesContext` starts at `totalVramGb: 0` and fills in on the first poll.

The auto-apply effect latches once per model path (`autoOptimizedForPath`), so firing
during that window pinned `gpu_layers: 0` permanently — and it never re-ran, because the
latch had already claimed the path.

Downstream, 14.6 GB of weights plus a 262144 KV cache against ~20.5 GB usable (22.49 minus
2 GB headroom) makes the optimizer walk its whole fallback chain — all layers → reduced →
split at minimum context → `optimalGpuLayers: 0` — so even a later recompute would have
agreed.

## Fix

- The context seed now **waits** for `optimized.ready` instead of writing the ceiling. The
  state already defaults to `DEFAULT_CONTEXT_SIZE`, so no provisional value is needed.
- The auto-apply effect is gated on `resourcesReady` from `useSystemResources()`, so it
  cannot latch a decision made with unknown VRAM. `resourcesReady` added to its deps.

`tsc`, `eslint --max-warnings 0` and the i18n check all pass.

## Not yet confirmed

The ordering was **reconstructed from live values**, not observed — the app's React state
is not directly inspectable, so the sequence is inferred from which code paths could produce
`Context: 256K` alongside `GPU Layers: 0/65` given a saved config of 32768. The fix
addresses both candidate paths, but confirmation means reopening the modal in a rebuilt app
and checking that context lands on the optimizer's value and GPU layers on 65.

## Why this kept happening

Third instance of the same shape: [[003_vram_fit_not_applied_or_warned]] fixed the optimizer
being computed and discarded, and added these very clamps — but the clamps sit behind a
guard that async initialisation can flip first. A correction that a later event can suppress
is not a fix; the value should never be wrong in the first place.
