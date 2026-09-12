# 014 — Special tokens (`<|im_end|>`) are rendered as text, streamed, and persisted

Status: FIXED and VERIFIED 2026-09-12
Found: 2026-09-11, during a systematic leak-scan battery on Qwen3.5-9B

## Symptom

**Every** assistant turn ends with a literal `<|im_end|>` in the streamed output and in the
stored message. 7 of 7 prompts in the battery, without exception:

```
...Hello! How can I help you today?<|im_end|>
...so I should respond with only "4".\n</think>\n\n4<|im_end|>
...Feature and unit tests<|im_end|>
```

Confirmed persisted, not just streamed — read back from `/api/conversation/{id}`:

```
The user wants me to reply with exactly the word OK…
</think>

OK<|im_end|>
```

## Root cause

`crates/llama-chat-engine/src/token_loop.rs:368` (and `:306`) detokenise with
`Special::Tokenize`, which **renders special tokens as their text form** instead of
treating them as control tokens. The EOS token is `248046 '<|im_end|>'` for this model
(confirmed in the loader output), so when it is decoded for the response string it arrives
as six visible characters.

Same call-site family as [[013_multibyte_utf8_broken_at_token_boundaries]] — both are
consequences of how we detokenise — but a different symptom and a different fix, so tracked
separately.

## Why this is not merely cosmetic

The frontend evidently strips it for display (the user's screenshot shows no `<|im_end|>`),
which is exactly why it went unnoticed. The damage is downstream of display:

1. **It is stored in the DB.** Conversation history is rebuilt from stored messages, so
   every prior turn feeds `<|im_end|>` back into the model's context as *literal text*
   rather than as the control token the template is supposed to emit. The model then sees a
   malformed transcript of its own history.
2. Any structural validation ([[002_malformed_response_detection_and_agent_feedback]]) or
   automated assertion ([[009_automated_e2e_agent_harness]]) runs against text containing
   control-token noise.
3. Export / copy-to-clipboard paths inherit it.

## FIXED and VERIFIED 2026-09-12

`token_loop.rs` no longer appends or streams the EOS token's text. It is a control token,
not content.

**The open question below is resolved: `Special::Tokenize` was left alone.** Checking
`tool_tags.rs` showed every configured tag is plain text (`<tool_call>`, `[TOOL_CALLS]`,
`<SYSTEM.EXEC||>`, …), but some models tokenize those *as* special tokens, so dropping the
flag wholesale risked breaking tool detection for no extra benefit. Suppressing the single
EOS append is the targeted fix.

The same site now also flushes any partial multi-byte character held by
[[013_multibyte_utf8_broken_at_token_boundaries]]'s decoder, so a truncated sequence at end
of turn cannot vanish silently.

Verified against a `build_id`-asserted server:

| | Stream | Stored message |
|---|---|---|
| `<\|im_end\|>` present | **no** | **no** |
| `U+FFFD` present | **no** | **no** |

The emoji in the same reply (🌊) survived, confirming 013 still holds.

`useMessageParsing.ts`'s `EOS_TOKEN_CLEANUP` regex is deliberately left in place as a
frontend safety net — it is no longer load-bearing, and it still covers older stored
messages that already contain the token.

## Open questions (resolved — kept for the reasoning)

- Does `Special::Tokenize` need to stay for the tool-tag machinery? Some model dialects use
  special tokens as tool delimiters, and `tool_tags.rs` may depend on seeing them. Switching
  blindly to a non-special detokenisation could break tool-call detection — **check before
  changing the flag**.
- If the flag must stay, the fix is to strip the resolved EOS/turn tokens from the response
  string at the point it is accumulated, not at render time. The frontend already strips for
  display; the backend should strip for storage.
- Are other special tokens affected (`<|im_start|>`, BOS)? Only `<|im_end|>` appeared in
  this battery, but it was the only one the model had reason to emit.

## Verification

- Assert no `<|im_end|>` / `<|im_start|>` / `<|endoftext|>` in the streamed tokens **or** in
  the stored message, across the battery.
- Regression: tool calling still detected for every dialect in `FORMAT_PRIORITY` after any
  change to the detokenisation flag.

## Related

[[013_multibyte_utf8_broken_at_token_boundaries]] — same detokenisation call sites.
[[012_eos_probe_prompt_leaks_into_visible_message]] — found in the same battery.
