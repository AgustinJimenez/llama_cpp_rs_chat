# 006 — Adopt an ADR decision log and put AGENTS.md on a size budget

Status: INVESTIGATION — planning started 2026-09-11, nothing designed or implemented yet
Source: comparative review of `E:\repo\Atomic-Chat` (AtomicBot-ai/Atomic-Chat, HEAD `64758ea`, 2026-09-09)

## Goal

Stop using `AGENTS.md` as a decision log. Give durable engineering decisions their own
dated files so the next session can find *why* something is the way it is without paying a
context tax on every unrelated task.

## Why this surfaced

Today alone I appended two substantial paragraphs to `AGENTS.md` — the TurboQuant
inactive-fallback note and the model-residency two-guards note. Both are genuine decisions
with context, consequences and a "keep these two places in sync" warning. Neither belongs
in a file that is loaded on every task, including tasks that never touch inference.

`AGENTS.md` is currently **141 lines** and drifting upward, one incident at a time.

## Verified in the reference project

- `docs/decisions/` holds **238** dated ADR files. Each carries `date` + `title`
  frontmatter and a Context / Decision / Consequences / Owner / Links body.
- There is a `_TEMPLATE.md` and a one-line-per-record `docs/decisions/INDEX.md`.
- Their `AGENTS.md` is **190 lines** and §7 states the policy explicitly:
  > "`AGENTS.md` is loaded into context on every single task, so its size is a tax on every
  > task. **Target ≤ 200 lines. Never exceed 300.** … **No decision log in this file.**
  > Each ADR is its own file under `docs/decisions/` … **At most 10 ADRs may be referenced
  > from this file**, and only ones that change how you write code today."
- Their §6 rule 8 makes it a hard obligation: *"Record non-trivial decisions as a new file
  in `docs/decisions/` (architecture, backend selection, perf trade-off, security default,
  schema or migration). Same session, before you finish."*

## Not yet verified / open

- Whether an ADR log and `AGENT_TASKS/` should be **separate** or whether tasks should
  simply gain a "decision" flavour. They are different things — a task is work to do, an
  ADR is a choice already made and its consequences — but two parallel logs may be more
  ceremony than this repo needs.
- Whether the ADR index should be generated or hand-maintained. Hand-maintained indexes
  rot; a generated one is another script to own.

## Direction (not yet a design)

1. Add `docs/decisions/` with `_TEMPLATE.md` and `INDEX.md`.
2. Migrate the existing standing policies out of `AGENTS.md` into ADRs, starting with the
   ones written this session:
   - TurboQuant is inactive; `turbo*` aliases to `q4_0`; the two cost models must agree.
   - Model residency is governed by the slot cap *and* the free-VRAM fit check.
   - Desktop dev builds embed `devUrl` and need Vite + a CSP entry.
   - llama-cpp-rs is a fork; the nested llama.cpp submodule tracks a plain mirror.
3. Leave behind a one-line link in `AGENTS.md` per migrated topic, not a summary.
4. Add the size budget to `AGENTS.md` itself so it is self-enforcing.

## Concrete next steps

- [ ] Decide: separate ADR log, or extend `AGENT_TASKS/` (blocks everything else here).
- [ ] Count how many lines of the current `AGENTS.md` are decision-log material vs
      genuinely per-task guidance. That number decides whether this is worth the ceremony.
- [ ] If yes: write the template, migrate 4 policies, measure the resulting `AGENTS.md`.

## Related

[[007_backend_capability_probing]] — the TurboQuant ADR is the first candidate to migrate.
