# 001 — Assistant turn renders blank in live UI (present in DB, appears after reload)

Status: OPEN — root cause narrowed, not yet pinpointed
Found: 2026-09-10, while smoke-testing the llama-cpp-rs v0.1.157 upgrade

## Symptom

Multi-turn chat, Qwen 9B agent. Second turn generated 91 tokens at 38.8 tok/s and
completed normally, but **no assistant bubble appeared in the UI at all** — not even a
Thinking block. The user is left staring at their own message with nothing after it.

## What was ruled OUT (verified, not assumed)

This was initially misdiagnosed as malformed model output breaking the parser. It is not.

- **Model output is well-formed.** Read straight from SQLite:
  ```
  The user is asking me to recall the multiplication result from before (17 * 23 = 391)...
  </think>

  **17 * 23 = 391**

  **391 * 2 = 782**<|im_end|>
  ```
  This is normal *prefill-mode* thinking — the Qwen3 chat template injects the opening
  `<think>` itself, so the stream legitimately contains only the closing tag.
- **Persistence is fine.** The message is in the `messages` table with correct content.
- **Parsing is fine.** `wrapPrefillThinking()` (`src/utils/toolFormatUtils.ts:302`) wraps a
  lone `</think>` correctly, and `buildSegments()` (`src/utils/toolSpanCollectors.ts:527`)
  calls it. Reloading the page renders the turn perfectly:
  `Thinking / 17 * 23 = 391 / 391 * 2 = 782`.
- **Title-refresh handler is innocent.** `useChat.ts:413` only `prev.map()`s new titles onto
  existing messages — it cannot drop a message from the list.

## Conclusion

The defect is in the **live streaming → message-state commit path**, not in the model, the
backend, the DB, or the parser. Something between "stream finished" and "commit assistant
message to React state" drops the turn. A reload re-fetches from the DB and it is fine.

## Why turn 1 worked and turn 2 did not (lead)

Turn 1 (rendered fine) contained tool calls. Turn 2 (blank) was thinking + short text with
**no tool calls**. Prime suspect: a code path that only commits/refreshes the final assistant
message when the tool-execution branch ran, leaving the plain-text-completion branch
without a final `setMessages` commit.

## Next steps

1. Instrument / read `src/hooks/useGenerationStream.ts` — find every terminal branch
   (`finish_reason` handling) and confirm each one commits the accumulated assistant
   message to state. Compare the with-tool-calls path against the plain-completion path.
2. Reproduce deterministically: same agent, ask a question that needs **no** tools, twice in
   one conversation. If turn 2 reliably blanks, the branch hypothesis is confirmed.
3. Check whether the streamed message is ever appended at all, or appended then removed
   (add a temporary log on `setMessages` length transitions).

## Related

Task 002 adds a frontend safety net that makes a blank assistant turn structurally
impossible to render silently, regardless of this bug's root cause. 002 is a mitigation;
this task is the actual fix.
