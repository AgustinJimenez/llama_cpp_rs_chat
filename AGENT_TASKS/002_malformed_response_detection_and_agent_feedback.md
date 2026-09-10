# 002 — Detect malformed model output and make the agent redo it

Status: DESIGN — not implemented
Opened: 2026-09-10

## Problem

Small models (9B and below especially) sometimes emit structurally broken output: tool-call
JSON that doesn't parse, unclosed tool tags, mixed tag dialects, a closing tag with no
opening one. Today the system has no authority that says "this turn is structurally invalid".
Consequences observed or possible:

- A tool call the model *believes* it made never executes — no detector matched — so the
  agent waits on a result that will never come, or hallucinates the result itself.
- `stripUnclosedToolCallTail()` hides an incomplete tail, leaving a pending widget forever.
- All visible content gets swallowed by tag handling and the user sees an empty turn.

The system currently *tolerates* malformed output silently. It should *detect* it and give
the model a chance to fix it.

> Note: the blank-turn symptom that prompted this task turned out **not** to be malformed
> output (see task 001 — it's a live-state bug). This task stands on its own as a real gap,
> but it is preventive, not the fix for 001.

## Design principle

**The backend is the only authority on structural validity. The frontend never decides.**
The backend owns the agentic loop already (`command_executor.rs`, `generation/mod.rs`,
`loop_detection.rs`); validation belongs there so headless/API users get it too, not just
the web UI. The frontend gets a dumb safety net only.

## Three layers

### Layer A — Prevention: grammar-constrained tool calls (strongest, currently blocked)

`tool_grammar.rs` already implements a GBNF lazy grammar that constrains JSON tool calls,
activating on the `{"name"` trigger. It is **disabled** — see the comment in
`crates/llama-chat-engine/src/sampler.rs` (`push_tool_grammar`):

> Tool grammar sampler crashes with C++ exception "Unexpected empty grammar stack after
> accepting piece: `<think>`" when Qwen3.6 produces `<think>` tokens.

Making malformed tool JSON *impossible to sample* beats detecting it after the fact. The
blocker is that the grammar doesn't admit model-specific special tokens (`<think>`) outside
its definition. Worth costing out: extending the grammar to allow the resolved think/turn
tags for the active model would re-enable this for every model at once.

### Layer B — Detection + corrective re-prompt (the core of this task)

After a generation turn completes (finish reason known, before the turn is surfaced), run a
structural validation pass:

```
validate_response_structure(content, resolved_tool_tags) -> Vec<StructuralDefect>
```

Defect taxonomy (each must be cheap and tag-config-driven, never hardcoded per model):

| Defect | Signal |
|---|---|
| `UnparseableToolCall` | content looks like it wanted a tool call (`{"name"`, or a resolved `exec_open` tag) but **no** detector in `FORMAT_PRIORITY` matched |
| `UnclosedToolCall` | `exec_open` present with no matching `exec_close` |
| `OrphanCloseTag` | a close tag with no opener — *excluding* legitimate prefill `</think>` |
| `MixedTagDialects` | tags from two different model dialects in one turn |
| `EmptyVisibleContent` | turn produced neither a non-empty visible segment nor a tool call |

`EmptyVisibleContent` is the highest-value invariant and the cheapest: **a completed
assistant turn must yield at least one visible segment or at least one tool call.** That
single rule catches the whole "user sees nothing" class regardless of which tag went wrong.

On defect, do **not** surface the broken turn. Instead:

1. Roll back the turn (the KV-rollback machinery for tool injection is the existing
   precedent for mid-conversation surgery).
2. Inject a short corrective message naming the exact defect **in the model's own resolved
   tag vocabulary** — e.g. "Your last message opened `<tool_call>` but never closed it.
   Re-send the response with a complete `<tool_call>[...]</tool_call>` block."
3. Regenerate the turn.

Reuse for detection rather than writing new parsing:
- `tool_tags.rs` → `get_tool_tags_for_model()` for the resolved tags (backend), mirrored by
  `toolFormatUtils.ts` on the frontend — these are already kept in SYNC per AGENTS.md.
- The existing 6-detector `FORMAT_PRIORITY` chain in `command_executor.rs` already answers
  "did anything match?", which *is* the `UnparseableToolCall` signal.

**Retry budget:** max 1–2 corrective regenerations per turn, tracked per-turn. This must
follow the existing philosophy of `MAX_TOOL_ITERATIONS=20` and `loop_detection.rs` — a
malformed-output retry loop is exactly the kind of thing that can burn a context window.
On exhaustion, surface the raw output with a visible "malformed response" affordance.
**Never silently discard content.**

### Layer C — Frontend safety net (do this first; it's ~5 lines)

In `buildSegments()` (`src/utils/toolSpanCollectors.ts`): if the resulting segment list is
empty but `rawContent.trim()` is non-empty, emit a fallback `{ type: 'text', content: raw }`
segment (or a "couldn't parse — show raw" affordance).

This is defense in depth, not a fix: it guarantees a user never sees a blank assistant turn
no matter what the model emits or what the parser does with it. Cheap, isolated, testable,
and it would have turned the task-001 symptom into something visible rather than silent.

## Recommended order

1. **Layer C** — smallest, immediate user-visible benefit, no backend risk.
2. **Layer B, `EmptyVisibleContent` only** — one rule, biggest coverage, exercises the
   corrective-re-prompt plumbing end to end with minimal surface area.
3. **Layer B, remaining defects** — once the retry loop is proven safe.
4. **Layer A** — revisit the grammar sampler; highest payoff, highest risk, needs the
   `<think>`-token crash resolved first.

## Open questions

- Should the corrective re-prompt be a `role=system` nudge or a synthetic `role=tool` error
  result? The latter reuses the existing injection path, but pollutes tool history.
- Should defects be surfaced in the Event log so the user can see the agent self-corrected,
  or stay invisible? Leaning: log them — silent retries make token counts confusing.
- Does the retry counter reset per user turn, or per conversation? Per turn, probably.
