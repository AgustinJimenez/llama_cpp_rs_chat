# 002 — Detect malformed model output and make the agent redo it

Status: ROOT CAUSE FOUND 2026-09-11 — the observed malformed output is OUR bug, not the model's
Opened: 2026-09-10

## 2026-09-11 — the "malformed model output" is self-inflicted

The premise below (small models emit broken structure, we should detect it) is **not what
was actually happening**. The corruption is injected by our own EOS probe.

### Reproduction (100%, both models, trivial prompt)

`POST /api/chat {"message": "Reply with exactly the word: OK"}` on Qwen3.5-9B stores:

```
The user wants me to reply with exactly the word OK. This is a simple acknowledgment request.
</think>

OK</think>

DONE<|im_end|>
```

One generation pass, 26 tokens, `finish_reason: "stop"` — not an auto-continue concatenation.

### Defect 1 — the probe verdict depends on the LITERAL first token

`token_loop.rs` intercepts EOS and calls `sub_checks::inline_eos_probe()`, which injects
`[SELF-CHECK] Are you completely done with the task? Type DONE if yes, or write your next
action if not.` and reads the reply. The verdict is decided at `sub_checks.rs:252`:

```rust
if i == 0 {
    let first = token_str.trim().to_uppercase();
    if first == "DONE" || first == "Y" || first.starts_with("YES") { ...complete... }
```

A **thinking** model does not answer with a bare word. Its first token is the thinking-close
tag. Server stderr, verbatim:

```
[EOS_PROBE] incomplete → 3 continuation tokens: "</think>\n\nDONE"
```

The model *did* say DONE — third token. The check never sees it, takes the continuation
path, and injects the entire probe reply into the response as if it were real content.
That is the whole observed artifact: the duplicate `</think>` and the stray `DONE`.

This fires on **every** agent-mode turn that makes no tool call (`probe_no_tool_calls`,
`token_loop.rs:239`), i.e. every ordinary conversational reply. Every thinking model is
affected — it is not a small-model problem.

It also explains the 27B's "hallucinated" `<tool_call>`: the model was obeying the hidden
probe's *"or write your next action if not"*. It was answering a question we asked it and
never showed the user.

### Defect 2 — the probe's KV rollback is invalid on M-RoPE models

Immediately after, in the same turn:

```
the last position stored in the KV cache for sequence 0 is X = 8070
the tokens for sequence 0 in the input batch have a starting position of Y = 8040
for M-RoPE, it is required that the position satisfies: X < Y
decode: failed to initialize batch / llama_decode: failed to decode, ret = -1
```

`inline_eos_probe` injects probe + sampled tokens into the **live** KV cache and relies on
`clear_kv_cache_seq(0, rollback_pos, None)` to undo it. On a multimodal-RoPE model (both of
these are vision-capable) re-decoding at a position ≤ the stored max is rejected outright,
so the turn's generation dies right there. This is what truncated the 27B mid-tag
(`</parameter` with no `>`).

### Defect 3 — NOT A BUG (retracted)

`POST /api/chat` returning `message.content: ""` is **by design**, `routes/chat.rs:226`:

```rust
content: "".to_string(), // Empty - real content comes via WebSocket
```

It is a fire-and-forget endpoint that returns `conversation_id` so the client can attach a
WebSocket. Recorded here because it was initially and wrongly cited as evidence of a
parser bug; it was API misuse on my part, not a defect.

### Consequence for this task's design

Layer C (frontend safety net) would have **hidden** this rather than fixed it, and Layer B
would have asked the model to "redo" output that our own probe corrupted. Fix the probe
first; the detection layers below remain worth doing, but as defence in depth, not as the
remedy.

## Fix applied 2026-09-11 — Defect 1

`sub_checks.rs` gained `probe_verdict_word(text, require_terminator)`, which strips markup
spans (anything between `<` and `>`) and returns the first bare word. The verdict is now
read from the accumulated reply rather than the literal first token, and the sampling loop
stops as soon as a delimiter-terminated word appears. Tag spans are dropped generically
rather than via `tool_tags.rs` config — a verdict is always a bare word, so this cannot
drift out of sync with per-model tag configuration.

Six unit tests pin the behaviour, including the exact observed reply `"</think>\n\nDONE"`.

**Verified live on Qwen3.5-9B**, same prompt as the reproduction:

| | stored assistant content | stderr |
|---|---|---|
| before | `...\n</think>\n\nOK</think>\n\nDONE<\|im_end\|>` | `[EOS_PROBE] incomplete → 3 continuation tokens` |
| after | `...\n</think>\n\nOK<\|im_end\|>` | `[EOS_PROBE] 'DONE' → task complete` |

A full agentic turn (tool call → result → summary) also runs clean. Note that turn
legitimately contains **two** `</think>` tags as matched pairs — any future structural
validator must count pairs, not occurrences, or it will flag correct output.

## Still open

- **Defect 2 (M-RoPE rollback) is unfixed and unreproduced.** It stopped triggering because
  the probe now takes the "complete" path, which rolls back and stops without re-decoding.
  The continuation path still injects into the live KV cache and rolls back, which the
  M-RoPE constraint `X < Y` rejects. I could not force a continuation to reproduce it after
  the fix, so I did not guess at a fix. Likely directions: skip the inline probe on M-RoPE
  models (falling back to a disposable context), or keep the probe tokens rather than
  rolling back. Needs a reliable way to trigger a continuation first.
- `check_eos_continuation()` has **no callers** but contains the identical first-token
  defect. Left unmodified (dead code); a warning comment now points at
  `probe_verdict_word`.
- Layers A/B/C below remain unimplemented, now correctly scoped as defence in depth.

---

## Original design (historical — premise partly invalidated above)

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
