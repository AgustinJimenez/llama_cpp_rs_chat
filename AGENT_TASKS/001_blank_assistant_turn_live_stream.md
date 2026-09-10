# 001 — Assistant turn renders blank in live UI (present in DB, appears after reload)

Status: FIXED — verified 2026-09-10
Found: 2026-09-10, while smoke-testing the llama-cpp-rs v0.1.157 upgrade

## Symptom

Multi-turn chat, Qwen 9B agent. Second turn generated 91 tokens at 38.8 tok/s and
completed normally, but **no assistant bubble appeared in the UI at all** — not even a
Thinking block. The user is left staring at their own message with nothing after it.
Reloading the page rendered the turn correctly.

## What was ruled OUT (verified, not assumed)

Initially misdiagnosed as malformed model output breaking the parser. It is not.

- **Model output is well-formed.** Read straight from SQLite: normal *prefill-mode*
  thinking — the Qwen3 chat template injects the opening `<think>` itself, so the stream
  legitimately contains only the closing tag.
- **Persistence is fine.** The message is in the `messages` table with correct content.
- **Parsing is fine.** `wrapPrefillThinking()` (`src/utils/toolFormatUtils.ts:302`) handles a
  lone `</think>` correctly and `buildSegments()` calls it. A reload renders the turn
  perfectly.
- **Title-refresh handler is innocent.** `useChat.ts:413` only `prev.map()`s new titles onto
  existing messages — it cannot drop a message.
- **Not the terminal branches in `useGenerationStream.ts`** (my original prime suspect —
  wrong). The plain-completion path commits state the same way the tool path does.

## Root cause

There is already a DB reconciliation in `onComplete` (`useGenerationStream.ts`) written for
exactly this failure, and its own comment names the cause: *"Reload message content from DB
to fix truncation caused by streamSeq mismatch (e.g. yn_continue fired mid-stream and
discarded tokens that the backend already wrote to DB)."*

**It could never fire.** The chain:

1. The backend mints its own message UUID (`crates/llama-chat-db/src/logger.rs:130`,
   `Uuid::new_v4()`) and returns it on completion.
2. `src/utils/generationStream.ts` received it as **`_messageId` and discarded it**, so it
   never reached `GenerationResult`.
3. `useGenerationStream.ts` therefore matched the DB row against `assistantMessageId` — a
   *local client-side* placeholder from `generateId()` (`src/utils/messageUtils.ts:6`).
4. Two independent UUID generators never produce equal values, so
   `rows.find((m) => String(m.id) === assistantMessageId)` always returned `undefined`.
5. `if (!dbMsg) return;` → **the safety net silently no-opped on every generation.**

So whenever tokens were dropped from live React state, nothing recovered them and the
message stayed blank until a manual reload.

Corroborating: the sibling title handler at `useChat.ts:424` matches on **`sequence_order`**,
not id — someone already hit this mismatch and worked around it there, while the two
id-based lookups in `useGenerationStream.ts` were left broken.

## Fix

- Added `serverMessageId` to `GenerationResult` (`generationStream.ts`).
- Stopped discarding the backend id in the transport adapter (`_messageId` → `messageId`).
- Reconciliation now matches on `serverMessageId`, with a "last assistant row" fallback for
  transports that don't supply one, plus an empty-content guard so a blank DB row can never
  clobber good live state.

## Verified

Reproduced the original two-turn sequence (tool-call turn, then a no-tool follow-up) on the
Qwen 9B agent. Turn 2 now renders live without a reload:

> "The result of 17 * 23 was 391, and doubled that is 782. (391 * 2 = 782)"

`tsc`, `eslint --max-warnings 0` and `check-i18n-keys` clean.

## Remaining nuance — the underlying token drop was not cured

This fix makes **recovery** work; it does not identify why tokens went missing from live
state in the first place (most likely the `streamSeq` mismatch the original comment
describes). The turn now self-heals at `onComplete`, but a user may still see an empty
bubble *during* streaming before it fills in. Worth a follow-up if that flicker is observed.

## Related

Task 002 — this is the real safety net for the "user sees nothing" class. Note that 002's
Layer C (a `buildSegments` fallback) would **not** have caught this case, since the message
never reached React state for the parser to run on.
