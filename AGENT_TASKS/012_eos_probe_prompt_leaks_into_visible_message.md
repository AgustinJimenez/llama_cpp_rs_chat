# 012 — The EOS self-check PROMPT leaks into the visible assistant message

Status: ROOT CAUSE CONFIRMED 2026-09-11 — regression in the 002 fix, not yet fixed
Found: 2026-09-11 by the user, in the freshly installed desktop app (Qwen 3.8 27B)

## Symptom

Sending `hi` to the `Qwen 3.8 27B` agent renders:

> Hi! � I'm here and ready to help. What would you like to work on today?**[SELF-CHECK] Are
> you completely done with the task? Type DONE if yes, or write your**

The app's own hidden prompt is displayed to the user, mid-sentence and truncated.

This is **distinct from [[002_malformed_response_detection_and_agent_feedback]]**. That task
fixed the model's *reply* to the probe leaking (`</think>\n\nDONE`). This is the probe
*question text itself* appearing in the response.

## The installed build DOES contain the 002 fix

Ruled out first, because "stale binary" is the obvious wrong answer:

| | |
|---|---|
| 002 fix committed | `596d5cc`, 2026-09-11 **10:42:59** -0300 |
| `llama_chat_app.exe` built | 2026-09-11 **11:08:58** |
| installer bundled | 2026-09-11 **11:16** |

The fix is in. This defect survives it.

## Root cause — confirmed by a failing test, not by reading

`probe_verdict_word()` in `crates/llama-chat-engine/src/sub_checks.rs` (added by the 002
fix) strips markup spans between `<` and `>`, then takes the first bare word with
`take_while(|c| c.is_alphanumeric())`.

When the model answers the probe by **echoing it**, the reply begins with `[`. That is not
alphanumeric, so `take_while` stops immediately, the word is empty, and the function returns
`None`. No verdict → `is_done` stays false → the continuation path fires → the echoed probe
text is pushed into `gen.response` and streamed to the user.

Verified with a test written against the observed string, which **fails** on current `main`:

```rust
let echoed = "[SELF-CHECK] Are you completely done with the task? Type DONE if yes,";
assert!(probe_verdict_word(echoed, false).is_some());   // FAILS -> None
```

(The diagnostic test was removed again so the tree stays green; re-add it with the fix.)

The 20-token probe budget explains the truncation at "or write your" — the reply is cut off
exactly where `max_probe_tokens` runs out.

## The deeper design flaw — do not stop at the punctuation bug

Patching `probe_verdict_word` to skip leading punctuation fixes *this* string. It does not
fix the shape of the problem:

**The continuation path injects whatever the model said, verbatim, into the user-visible
message.** The probe is a hidden question. Any unhelpful answer to it — an echo, a
restatement, a refusal, a fabricated tool call (already observed on the 27B in 002) — becomes
text the user sees, attributed to the assistant.

A correct fix needs both:

1. **Widen the verdict reader** so punctuation-led replies still yield a word.
2. **Never surface a reply that is not a genuine continuation.** At minimum reject a
   continuation that contains, or is a prefix of, `EOS_PROBE_TEXT`. Better: treat "no
   confident verdict" as *complete* (accept EOS) rather than as *continue*. Accepting EOS
   wrongly ends a turn early; continuing wrongly shows the user our internal prompt. The
   first failure is far cheaper, so the default should be biased toward "done".

## Fix direction

- Skip leading non-alphanumerics (not just `<...>` spans) before reading the verdict word.
- Add an explicit guard: if the continuation overlaps `EOS_PROBE_TEXT`, discard it and treat
  the turn as complete; log it.
- Flip the fallback when no verdict word is found at all: currently "continue", should be
  "complete".
- Re-add the regression test plus one asserting a probe echo never reaches `gen.response`.

## Verification

- Unit: `probe_verdict_word("[SELF-CHECK] …")` yields a word; an echoed probe is classified
  as complete.
- Live: send `hi` to a thinking agent; assert `[SELF-CHECK]` never appears in the streamed
  tokens or the stored message.
- Regression: keep the 002 checks green (`</think>\n\nDONE` still reads as DONE).

## Related

[[002_malformed_response_detection_and_agent_feedback]] — same subsystem; this is the second
leak path through it, and evidence the probe design is fragile rather than that one check
was wrong.
[[013_multibyte_utf8_broken_at_token_boundaries]] — the `�` in the same screenshot is a
separate defect, not part of this one.
