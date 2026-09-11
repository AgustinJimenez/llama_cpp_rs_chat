# 007 — Probe backend capabilities at runtime instead of hardcoding them

Status: INVESTIGATION — planning started 2026-09-11, nothing designed or implemented yet
Source: comparative review of `E:\repo\Atomic-Chat` (HEAD `64758ea`)

## Goal

Make "does this build actually support feature X?" a question answered by the running
backend, not by a constant in our source that silently rots.

## Why this surfaced

Our TurboQuant handling is three hardcoded assumptions that must agree and have no way to
detect when they stop agreeing:

1. `parse_kv_cache_type()` in `crates/llama-chat-engine/src/context_eval.rs` maps
   `turbo2`/`turbo3`/`turbo4` → `Q4_0`.
2. `src/utils/vramUtils.ts` carries matching per-element byte costs.
3. `AdvancedContextSection.tsx` labels the options "TQ2 — TurboQuant (currently Q4_0)".

(2) disagreed with (1) for months, under-counting KV cache by ~1.8×, which inflated
`optimalContextSize` for every turbo-configured model — and nothing errored. If the custom
TQ types ever come back, all three must change in one commit or the same class of silent
drift returns.

## Verified in the reference project

- `tests/fixtures/capabilities/` holds JSON descriptors per backend build, e.g.
  `turboquant-b10269-1.4.0.json` and `upstream-b10205.json`. Each records
  `schema_version`, `provider`, `source: "live-binary"`, `binary`, `version`, **`sha256`**,
  and the full sorted `flags` array scraped from the binary.
- `tests/capabilities.test.mjs` asserts against those fixtures, so a backend that quietly
  loses a flag fails a test rather than degrading at runtime.
- Their `AGENTS.md` §3 states the rule this enables: *"Fork-only cache types (`-ctk`/`-ctv
  turbo*`) must be guarded by **provider identity, not by OS**"* and *"Speculative flags are
  provider/build-gated … do not infer support from a model family or expose an unverified
  fork flag."*
- ADR `2026-05-19-windows-uses-upstream-…`: *"Any new fork-only flag wired into the
  extension must be guarded behind a runtime capability check, not assumed available on
  every host."* and *"UI surfaces that advertise TurboQuant / MTP / NextN must hide or
  disable the controls when the active backend build is the upstream Windows one."*

## The important difference from our situation

**Their probe is cheap because they shell out to `llama-server --help`.** We link llama.cpp
into the binary through Rust bindings — there is no `--help` to scrape. So the mechanism
does **not** port directly; only the principle does.

Plausible equivalents, none yet investigated:
- Ask ggml at startup whether a given type id is valid (the TQ crash was an out-of-bounds
  read in `ggml_blck_size()`, which implies a queryable type table).
- Emit a build-time capability descriptor from `build.rs` based on the resolved llama.cpp
  submodule commit and enabled features, and have the frontend read it via an endpoint.
- A tiny startup self-test that attempts each KV type against a throwaway context.

## Not yet verified

- Whether ggml exposes a safe "is this type id valid" query, or whether probing means
  risking the same ACCESS VIOLATION that disabled TurboQuant in the first place. **This is
  the gating question** — an unsafe probe is worse than a hardcoded constant.
- Whether the capability surface we care about is broader than KV types (flash attention on
  specific architectures, mmproj compatibility, CUDA graph support all have the same shape).

## Concrete next steps

- [ ] Read the current llama.cpp type-registry code in
      `deps/llama-cpp-rs/llama-cpp-sys-2/llama.cpp` and determine whether a **safe**
      validity query for a GGML type id exists.
- [ ] If it does: prototype a startup capability struct, exposed on `/api/model/info` or a
      new `/api/capabilities`, and make `AdvancedContextSection.tsx` hide unsupported KV
      options instead of relabelling them.
- [ ] If it does not: fall back to a build-time descriptor generated in `build.rs` — still
      better than three hand-synced constants, because it is derived from one source.

## Related

[[006_adr_decision_log_and_agents_md_budget]] — the TurboQuant policy is the ADR that
should record whatever this concludes.
