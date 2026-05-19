# Models

## Supported formats

- **GGUF** — the only supported format. Covers all major model families (LLaMA, Mistral, Qwen, Phi, Gemma, DeepSeek, etc.)
- Quantizations from Q2_K through Q8_0 and F16 are all supported

---

## GPU layers

`gpu_layers` controls how many transformer layers run on GPU vs CPU.

- Set to the model's total layer count to run everything on GPU (fastest)
- Reduce if you hit VRAM limits — layers overflow to CPU
- Set to `0` to run entirely on CPU

The **Load Model** dialog shows estimated VRAM usage as you adjust.

---

## Context size

`context_size` is the total token window (prompt + output).

- Larger context = more VRAM for the KV cache
- CUDA VMM silently pages to RAM if VRAM runs out → severe slowdown (70+ ms/tok)
- Start with 8192–32768 and increase if needed

---

## KV cache types

Controls the precision of the attention key/value cache. Lower precision = less VRAM, slight quality trade-off.

| Type | VRAM | Quality |
|------|------|---------|
| `f16` (default) | 100% | reference |
| `q8_0` | 50% | ≈ identical |
| `q4_0` | 25% | slight loss |
| `turbo2` / `turbo3` / `turbo4` | 12–25% | good for long context |

`turbo2`/`turbo3`/`turbo4` are asymmetric quantizations that compress better than symmetric types with less quality loss.

---

## Sampler configuration

| Parameter | Default | Effect |
|-----------|---------|--------|
| `temperature` | 0.8 | Randomness. 0 = greedy |
| `top_p` | 0.95 | Nucleus sampling |
| `top_k` | 40 | Top-K sampling |
| `min_p` | 0.05 | Minimum probability filter |
| `repeat_penalty` | 1.1 | Penalizes repeated tokens |

Samplers are applied as a chain: `penalties → DRY → top_n_sigma → top_k → top_p → min_p → temperature → mirostat`.

---

## Auto-configuration

Models that include a `general.sampling.*` section in their GGUF metadata load with preset samplers automatically. You can also define per-model presets in the UI settings.

---

## Vision (multimodal)

1. Download the matching `mmproj-*.gguf` projection file from the same model release
2. Place it in the **same directory** as the main `.gguf` file
3. The app detects it automatically on load

Supported: LLaVA-style models, BakLLaVA, Qwen-VL, Llama-3.2-Vision, etc.

---

## Recommended models

See [`docs/MODEL_CONFIGURATIONS.md`](../MODEL_CONFIGURATIONS.md) for tested configurations with exact settings and benchmark results.
