# 013 — Multi-byte UTF-8 characters are corrupted at token boundaries

Status: ROOT CAUSE CONFIRMED 2026-09-11 — not yet fixed
Found: 2026-09-11, visible as `�` in the desktop app; confirmed in raw server output

## Symptom

Emoji and box-drawing characters render as the replacement character `�`.

In the desktop app: `Hi! � I'm here and ready to help…` (an emoji).

In a 60 KB agentic run, **59 occurrences**. The pattern is diagnostic:

```
tmp_project/
�── app/
│   ├── Http/
│   │   �── Controllers/
│       �── ProductController.php
```

`│` (U+2502) and `├` (U+251C) survive; `└` (U+2514) does not. All three are 3-byte
sequences differing only in the last byte, so this is not "the app can't do Unicode" — it
is position-dependent.

## Confirmed to be ours, not a tooling artifact

Checked deliberately, because the text passed through PowerShell before I saw it:

- Raw bytes written by `curl` straight from `/api/chat/stream`: **59** occurrences of
  `EF BF BD` (UTF-8 U+FFFD).
- Reassembled from the SSE JSON: same 59.

The corruption is already present in what the server sends. It is not the terminal, not
`Out-File`, and not the frontend.

## Root cause

`crates/llama-chat-engine/src/token_loop.rs:368` decodes **each token independently**:

```rust
let token_str = match model.token_to_str(next_token, Special::Tokenize) { … };
```

A multi-byte character whose bytes span two tokens cannot survive this: the first token
holds a truncated prefix and the second an orphaned continuation. Each decodes to invalid
UTF-8 on its own and collapses to `�`. Whether a given character breaks depends purely on
where the tokenizer split it — which is why `├` survives and `└` does not.

There is **no incomplete-sequence buffering anywhere** in the generation path (grepped
`token_loop.rs` and `generation/mod.rs` for `utf8` / `from_utf8` / buffering: no matches).

The same per-token-decode pattern is repeated in at least five other places, so any fix
should be a shared helper rather than a local patch:

- `crates/llama-chat-engine/src/token_loop.rs:306` and `:368`
- `crates/llama-chat-engine/src/sub_agent.rs:188`
- `crates/llama-chat-engine/src/tool_output/image_summary.rs:81`
- `crates/llama-chat-engine/src/tool_output/summarize.rs:95`, `:247`, `:362`

## Why it matters beyond cosmetics

1. The corrupted text is what gets **persisted and streamed** — it is in the DB, so it also
   re-enters the model's context on the next turn as `�`.
2. It lands in **generated artifacts**. The Laravel run wrote files whose content came from
   this path; a mangled byte inside a written source file is a real defect, not a display
   glitch. (Those particular files passed `php -l`, but that is luck — the damage was in
   tree-drawing inside a Markdown summary, not in code.)
3. Any structural validation added by [[002_malformed_response_detection_and_agent_feedback]]
   or [[009_automated_e2e_agent_harness]] would be matching against corrupted text.

## Fix direction

Standard approach for llama.cpp integrations: accumulate raw token **bytes** and only emit a
string when the buffer ends on a complete UTF-8 sequence; carry an incomplete tail forward to
the next token.

Open questions before designing:

- Does `llama-cpp-2`'s `token_to_str` expose a byte-level variant (`token_to_bytes` or
  similar)? If it already lossily substitutes `�` internally, we need the byte API, and if
  the bindings do not expose one this becomes a `deps/llama-cpp-rs` change.
- Streaming interacts with this: a held-back partial sequence delays a token by one step.
  That is acceptable, but the stop-condition and tool-tag detectors run on the accumulated
  string, so the buffering must sit *below* them — they must never see a partial char.
- `Special::Tokenize` renders special tokens as text (that is how `<|im_end|>` reached the
  output in 002). Confirm the byte-level path preserves that behaviour.

## Verification

- Unit: feed a token sequence that splits a 4-byte emoji and a 3-byte `└`; assert exact
  round-trip.
- Live: prompt for a directory tree and an emoji; assert zero `U+FFFD` in the raw stream.
- Regression: re-run the Laravel task and assert the `U+FFFD` count is 0 (was 59).
