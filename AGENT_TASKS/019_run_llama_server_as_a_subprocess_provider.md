# 019 — Run llama-server as a subprocess provider (switch llama.cpp builds without rebuilding)

Status: PROVEN FEASIBLE 2026-09-12 — works today with ZERO code changes; only management is missing
Goal (user's): swap llama.cpp builds/forks (e.g. `PrismML-Eng/llama.cpp`) without editing the
submodule and rebuilding for ~44 minutes.

## The finding: the inference half already exists

`providers/mod.rs` routes any provider id beginning with `custom_` to
`openai_compat::generate` with an arbitrary `base_url`. `llama-server` speaks
OpenAI-compatible `/v1`. So the two already fit together.

**Verified end to end, no code changed:**

1. Downloaded the official `llama-b11039-bin-win-cuda-12.4-x64.zip` (242 MB).
2. `llama-server --list-devices` → `CUDA0: NVIDIA GeForce RTX 4090 (23027 MiB, 21506 MiB free)`
   — **without** the 373 MB `cudart-*` package, because this host has the CUDA toolkit.
   That confirms the claim in [[017_consume_published_llama_cpp_backends_instead_of_compiling_cuda_in]]
   against an official binary.
3. Started it on `127.0.0.1:8080` with Qwen3.5-9B, `-ngl 99`, ctx 32768. `/health` → ok,
   VRAM 10,831 MiB.
4. Direct `/v1/chat/completions`: correct haiku + 🌊, **69.9 tok/s**.
5. Registered `custom_llamaserver` → `http://127.0.0.1:8080/v1` in `provider_api_keys` and
   asked **our app**:
   - `GET /api/providers/custom_llamaserver/models` → `["E:\ai_models\Qwen3.5-9B-Q8_0.gguf"]`
   - `POST /api/providers/custom_llamaserver/stream` → 28 streamed tokens, correct haiku,
     🌊 intact, zero `U+FFFD`.

So a user can already run any llama.cpp build — any fork, any tag — and use it from the app,
provided they start it themselves.

## What is actually missing (management, not inference)

| Piece | State |
|---|---|
| Talk to llama-server | **done** — `openai_compat` + `custom_` |
| Agentic tool loop over HTTP | **done** — that path has `max_turns` + `mcp_bridge` |
| Spawn / health-check / kill a child | **mostly** — `ProcessManager` already does this for workers |
| Download + resolve tag/asset/sha256 | **new** — `scripts/resolve-upstream-backend.mjs` in the Atomic Chat clone is a working reference |
| UI to pick a version/fork | **new**, small |
| Map llama-server (one model per process) onto our slot/eviction model | **new**, the fiddly part |

## Performance note — needs a fair re-test

llama-server measured **69.9 tok/s** vs our in-process **41.7 tok/s** on the same model and
GPU. **Not a clean comparison:** llama-server ran `-ngl 99` while our worker used
`gpu_layers=32`, so part of our model may have been on CPU. Re-measure with matched layer
counts before drawing any conclusion. If the gap survives, it matters a lot.

## The real cost

Two inference paths to maintain: in-process (KV surgery for tool injection, custom sampler
chains, MTMD vision, and the EOS probe from
[[012_eos_probe_prompt_leaks_into_visible_message]]) and subprocess. Atomic Chat carries
exactly this split (`llamacpp` + `llamacpp-upstream` as separate extensions) and a large share
of their 238 ADRs exist to keep the two honest.

Worth noting the subprocess path would *not* have the 012 class of bug at all — llama-server
has no self-check probe to leak.

## Next steps

- [ ] Re-measure tok/s with matched `gpu_layers` before treating the 69.9 vs 41.7 gap as real.
- [ ] Try `PrismML-Eng/llama.cpp`'s `dflash-*` branches this way — they ship no Windows release
      assets, so this needs building `llama-server.exe` from that fork once, which is still
      cheaper than rebuilding our whole bindings stack.
- [ ] Decide scope: a documented manual path (works today), or managed download + lifecycle.
- [ ] If managed: resolver with tag/sha256 first, since an ABI-mismatched binary fails as a
      silent CPU fallback rather than an error.

## Related

[[017_consume_published_llama_cpp_backends_instead_of_compiling_cuda_in]] — same published
artifacts, different consumption model. 017 loads `ggml-*.dll` into our process; this runs the
whole server out of process and sidesteps the ABI question entirely.
