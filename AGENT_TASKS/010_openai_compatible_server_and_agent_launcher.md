# 010 — OpenAI-compatible API surface, and a launcher for external coding agents

Status: INVESTIGATION — planning started 2026-09-11, nothing designed or implemented yet
Source: comparative review of `E:\repo\Atomic-Chat` (HEAD `64758ea`)

## Goal

Let other tools use our loaded model. Today the only client of our inference stack is our
own frontend; every external agent, CLI and IDE plugin speaks the OpenAI API and cannot
talk to us at all.

## Why this is worth considering

We already run an HTTP server on `:18080` with a worker pool, VRAM-aware eviction, a slot
cap, and a tool-calling loop. The gap between that and "a local OpenAI endpoint" is a
translation layer, not new infrastructure. The payoff is that every OpenAI-compatible tool
becomes a client of the models we already host.

## Verified in the reference project

- README: *"Atomic Chat runs an **OpenAI-compatible server at `http://localhost:1337/v1`**
  — a drop-in replacement for the OpenAI SDK. Load a model in the app, then point any
  client at it."*
- Their `AGENTS.md` §6 rule 3 treats it as a contract: *"`http://localhost:1337/v1` must
  stay OpenAI-compatible — OpenCode, Codex, Hermes and others depend on it. Adding
  non-standard fields is fine; breaking standard ones is not."*
- A **Launch page** (`web-app/src/routes/launch/`, catalog in
  `web-app/src/constants/integrations.ts`, commands in
  `src-tauri/src/core/system/commands.rs`) installs and configures external agents
  one-click. ADRs cover Codex CLI, Cline, Goose, OpenHands, KiloCode, Pi, MiMo Code.
- Compatibility is not free — they needed a shim: ADR *"Add a `/v1/responses` translation
  shim to the local proxy so Codex CLI works on llama.cpp models"*. Further ADRs cover
  login-shell `PATH` resolution for detecting installed agents in packaged builds, WSL
  detection, JSON5-lenient parsing of `openclaw.json`, and seeding
  `gateway.auth.mode: "none"` on loopback. The integration tail is long.

## Honest assessment of fit

This is the **largest** item from the comparison and the least obviously ours.

Arguments for: it is leverage — our worker pool and eviction become infrastructure other
tools depend on, and the desktop app stops being the only consumer.

Arguments against:
- Our differentiator is the *agentic loop itself* (in-process KV manipulation, tool
  injection, the EOS probe, rollback). An OpenAI `/v1/chat/completions` endpoint exposes
  **token generation**, not that loop. We would be exposing the least distinctive layer.
- `/v1/chat/completions` with `tools` has its own tool-calling semantics that would have to
  coexist with ours (`tool_tags.rs`, the 6-detector `FORMAT_PRIORITY` chain). Two
  tool-calling models in one server is a real design burden, not a shim.
- The Launch page is downstream of the API and is mostly integration grunt work per agent.

**Suggested split:** treat the API and the launcher as separate decisions. The API may be
worth it alone; the launcher only makes sense once the API exists and is stable.

## Not yet verified

- Whether a minimal read-only subset (`/v1/models`, `/v1/chat/completions` without `tools`,
  streaming) is enough for the tools worth supporting, or whether they all require function
  calling — which would pull in the coexistence problem immediately.
- Whether `crates/llama-chat-web/src/providers/openai_compat/` (which already exists, for
  us acting as a *client* of OpenAI-compatible providers) contains reusable request/response
  types for the server direction.
- Port and auth policy. Binding an inference endpoint invites the same "Invalid host header"
  / trusted-hosts problems they wrote an ADR about.

## Concrete next steps

- [ ] Read `crates/llama-chat-web/src/providers/openai_compat/` and report how much of the
      schema is already modelled. This decides whether this is days or weeks.
- [ ] Pick **one** target client and define success as "that client works". A generic
      "be OpenAI-compatible" goal has no finish line.
- [ ] Decide explicitly whether tool calling is in scope for v1. Recommend **no**.
- [ ] Only then consider the launcher.
