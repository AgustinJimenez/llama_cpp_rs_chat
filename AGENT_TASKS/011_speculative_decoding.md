# 011 — Speculative decoding (MTP / NextN / EAGLE-3 style drafting)

Status: INVESTIGATION — planning started 2026-09-11, nothing designed or implemented yet
Source: comparative review of `E:\repo\Atomic-Chat` (HEAD `64758ea`)

## Goal

Raise generation throughput without changing models, by drafting several tokens per
verification step.

## Why this surfaced

Measured on this machine today:

| Model | Generation |
|---|---|
| Qwen3.5-9B Q8_0 | ~40–46 tok/s |
| Qwen3.8-27B IQ4_XS | ~30 tok/s |

Prompt processing is fast (~1,950–3,650 tok/s) — decode is the bottleneck, which is exactly
what speculative decoding targets. Agentic runs make this worse: today's Laravel task spent
**307 s generating 14,097 tokens**, and long agent turns are the dominant cost in this app.

## Verified in the reference project

They ship three distinct drafting families, each provider-gated:

- **Gemma 4 MTP** — a separate draft head; upstreamed as llama.cpp PR #23398. Flags
  `--mtp-head`, `--spec-type mtp`.
- **Qwen 3.6 NextN** — `--spec-type nextn`.
- **EAGLE-3** and **DFlash** on the MLX side — `--draft-kind dflash|eagle3|mtp`.

And, importantly, several ADRs about it going *wrong*:
- *"Fix MTP speculative rollback crash on Gemma 4 + DeepSeek-V4"*
- *"Gate the global `mtp` flag on per-model capability at load time so non-MTP models can't
  be bricked by a stale toggle"*
- *"Reset the global `llamacpp-upstream` MTP toggle on active-model change so it can't stay
  'on' for a non-MTP model"*
- *"…reactive MTP-disable fallback"*

That second and third pair are our exact failure class: a **global toggle that outlives the
model it was valid for**. We have live instances of that shape already (the 27B's stored
`turbo2`/`turbo3`, the `context_size = 262144`). Any speculative feature we add must be
gated per-model at load time, not stored as a global preference.

## What is unknown and gating

- **Does our llama.cpp pin support any of this?** The nested submodule
  (`deps/llama-cpp-rs/llama-cpp-sys-2/llama.cpp`) tracks a plain upstream mirror, so
  whatever upstream has at that commit is what we have. MTP was upstreamed via PR #23398 —
  needs checking against our pin.
- **Does `llama-cpp-rs` expose it?** This is the real blocker. Even where llama.cpp supports
  drafting, the Rust bindings must surface the draft-model/draft-head APIs. If they do not,
  this becomes fork work on `deps/llama-cpp-rs` — a much larger commitment than a flag.
- **Does it compose with our token loop?** `token_loop.rs` does non-trivial things per
  token: exec-block tracking, stop conditions, repetition detection, the EOS probe, tool
  injection with KV rollback. Speculative decoding accepts/rejects *batches* of tokens, so
  every per-token invariant needs re-examining. The M-RoPE rollback defect in
  [[002_malformed_response_detection_and_agent_feedback]] is a warning: KV surgery in this
  codebase is already fragile.
- **Does a suitable draft model exist** for the models we actually run? Classic speculative
  decoding needs a small model sharing the tokenizer; self-drafting (MTP/NextN) needs the
  model to ship the head. Neither is guaranteed for Qwen3.5-9B or Qwen3.8-27B.

## Honest priority

This is the **highest-risk, highest-ceiling** item from the comparison and probably should
not be started until [[009_automated_e2e_agent_harness]] exists — a change that alters how
tokens are produced, inside a loop that does KV surgery, without automated structural
verification is how the 002-class defects get introduced rather than caught.

## Concrete next steps

- [ ] Check whether the pinned llama.cpp commit contains the MTP / NextN paths at all.
- [ ] Grep `deps/llama-cpp-rs/llama-cpp-2/src/` for any draft-model or speculative API
      surface. **If absent, stop here and record that** — the answer alone is worth having.
- [ ] Check whether Qwen3.5/3.8 ship a draft head or have a tokenizer-compatible small
      sibling.
- [ ] Only if all three are yes: design how drafting interacts with `token_loop.rs`'s
      per-token invariants, per-model gating included from the start.
