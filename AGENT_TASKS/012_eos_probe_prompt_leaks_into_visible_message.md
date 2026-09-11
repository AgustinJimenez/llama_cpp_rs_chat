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

## 2026-09-11 update — the punctuation bug is NOT the main cause

A 7-prompt battery on Qwen3.5-9B reproduced the leak on `hi` with a **different** reply
shape, which corrects the analysis above. Raw stream, verbatim:

```
The user just said "hi" … I should respond warmly and offer to help.
</think>

Hello! How can I help you today?</think>

I'm<|im_end|>
```

The probe reply was `</think>\n\nI'm` — the model simply continuing its greeting ("I'm here
and ready to help…", which is exactly what the user's 27B screenshot shows).

Here `probe_verdict_word` worked **perfectly**: markup stripped, first bare word `"I"`.
`"I"` is not `DONE`/`Y`/`YES`, so the continuation path fired and injected the reply.

So the punctuation hole is real but secondary. **Fixing it would not have prevented this
case.** The defect is the design: *any* reply that is not an explicit completion verdict is
treated as legitimate continuation and surfaced verbatim. Confirming the fix must be the
fallback flip described below, not the parser patch.

Also learned: the leak is **intermittent**. Of two trivial prompts, `hi` leaked and
`What is 2+2?` did not — it depends entirely on what the model happens to say to a hidden
question. Intermittent makes it worse, not better: it will not show up reliably in manual
testing.

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

## Fix attempt #1 — 2026-09-11 — DID NOT RESOLVE THE SYMPTOM

Changed `inline_eos_probe` to (a) skip leading punctuation when reading the verdict word and
(b) **discard the model's probe reply entirely**, returning a neutral `"\n\n"` nudge instead,
so the reply can never become response content. 71 unit tests pass, clippy clean.

**The live symptom is unchanged.** Re-ran an 11-prompt battery on Qwen3.5-9B: all 5 greeting
prompts still produce

```
…Hello! How can I help you today?</think>\n\nI'm<|im_end|>
```

Do not repeat these checks — they are already done:

| Ruled out | Evidence |
|---|---|
| Stale binary | Old strings `returning {} continuation tokens` / `inline probe continuation:` **absent** from `llama_chat_web.exe`; new `nudging continuation (probe reply discarded)` **present** (UTF-8 byte scan both directions) |
| Stale process | Binary built 14:13:18, both processes started 14:17:46 |
| Edit in the wrong function | Nudge code is at `sub_checks.rs:359-387`, inside `inline_eos_probe` (starts line 232), not the dead `check_eos_continuation` (line 27) |
| Duplicate source copy | Only one live `sub_checks.rs`; the other hit is an unused `.claude/worktrees/` checkout |
| Frontend rendering | Present in the **raw SSE bytes** off `curl` |

## The contradiction that blocks progress

The leaked text arrives as **one SSE event containing multi-token text**, with `tokens_used`
not advancing:

```
data: {"token":"?","tokens_used":8045,...}
data: {"token":"</think>\n\nI'm","tokens_used":8045,...}
data: {"token":"<|im_end|>","tokens_used":8045,...}
```

A sampled token is always one token and advances the counter. A lump that does not advance
it matches exactly one sender — `token_loop.rs:265-273`, which streams
`check.continuation_text` from the probe. But that field is now hard-coded to `"\n\n"` in a
binary verified to contain the new code.

**So either the lump comes from a sender I have not identified, or my model of this path is
wrong.** Both are possible; I could not distinguish them.

## Observability is the blocker — fix this first

`[EOS_PROBE]` never appears in captured output, so the probe's actual behaviour is
unobservable:

- `Stdio::inherit()` is set for the worker (`process_manager.rs:130`), and its `[WORKER]`
  startup lines **do** reach the parent's redirected stderr — but `llama_model_loader`
  output and every `eprintln!` from `sub_checks.rs` do not.
- Tried `Start-Process -RedirectStandardError` and `cmd /c … > file 2>&1`; neither captures
  them. No stderr redirection or `llama_log_set` call found in the worker source.
- The DB `logs` table has no rows for these runs, so `log_info!`/`log_event` are also
  silent (`disable_file_logging` defaults to 1).

**Next step is instrumentation, not another fix attempt.** Add an unmissable channel —
e.g. carry the probe outcome in the SSE `done` event, or write directly to a file from
`inline_eos_probe` — then re-run `hi`. Guessing further without it will just produce a
third wrong diagnosis after the first two.

### Instrumentation attempt — file writes from the worker FAIL SILENTLY

Tried exactly that and it did not work. Recorded so the next attempt does not repeat it.

Added a `probe_debug()` helper appending to a file, and called it from five places:
`inline_eos_probe` entry, its verdict, its return paths, `run_generation_loop` entry,
`generate_llama_response` entry (the function the worker actually imports), per sampled
token, and — as a **control** — `handle_load_model` in
`crates/llama-chat-worker/src/worker/worker_main/model_commands.rs`.

Result: **the log file was never created, not even by the control marker**, on a run that
definitely loaded a model and definitely generated 3019 bytes of output.

- Tried a relative path (`target/probe_debug.log`) and an absolute one
  (`E:/Temp/probe_debug.log`). Neither.
- The directory is writable — a PowerShell canary wrote to the same absolute path fine.
- Binary verified to contain all marker strings; processes verified to start after the
  build; build verified to print `Finished`.

Conclusion: **file I/O from inside the worker process is failing**, and the failure was
invisible because the helper swallowed it (`if let Ok(mut f) = OpenOptions…`). Cause not
yet identified — the worker is spawned with `CREATE_NO_WINDOW` and piped stdin/stdout
(`process_manager.rs:119-141`), which should not block file access. This is the same shape
as the rest of this task: a mechanism that looks like it works and silently does nothing.

**Do not use file-based instrumentation in the worker until that is understood.** Prefer a
channel that already demonstrably crosses the process boundary — the SSE stream itself
(e.g. an extra field on the `done` event) or the IPC payload.

### Two false conclusions I drew along the way

Recorded because both cost real time:

1. "The engine code is not executing." Two runs produced no output at all because I reused
   a stale `worker_id` in the request body, so the server answered
   `{"error":"No model loaded and no model configured for this conversation"}`. **Always
   assert the run produced tokens before interpreting the absence of a log line.**
2. "There is a duplicate live copy in `src/web/chat`." There is not —
   `src/web/mod.rs:19` is `pub mod chat { pub use llama_chat_engine::*; }`, a re-export.
   The stale `src/web/chat/mod.rs` file on disk is shadowed by it and unused.

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
