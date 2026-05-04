# Remote Provider Prompt Cache Preservation

DeepSeek (and other providers) use prompt caching: if the conversation messages array
stays byte-identical between API calls, the cached prefix is reused (cheaper/faster).
Any modification to prior messages breaks the cache.

## Rules

1. **Never mutate existing messages in the array** — only append new ones.
2. **Never inject system messages mid-conversation** — warnings go as user messages at the end, or are handled client-side.
3. **Save/load must be byte-identical** — messages stored in DB must reconstruct to the exact same JSON sent to the API.
4. **Preserve all provider-specific fields** — `reasoning_content` (DeepSeek), etc.
5. **Summarization is separate** — tool output summarization uses a separate API call, never modifies the main conversation array.
6. **No history stripping on error** — don't silently drop conversation history.

## Issues Found & Fixed (2026-05-03)

### 1. trim_old_tool_results() mutated messages in-place
**Was:** Replaced tool message content with `[Output: XX chars]` and truncated assistant messages.
**Fix:** Create a shallow copy of the messages array for the API request, only modifying the copy. Original array stays untouched for cache continuity.

### 2. Loop detection warning injected mid-conversation
**Was:** `messages.push(json!({"role": "system", "content": "WARNING: ..."}))` inserted dynamically.
**Fix:** Append warning as the last message (after all tool results), so it only extends the array without altering prior content.

### 3. reasoning_content lost on DB reload
**Was:** `reasoning_content` saved in assistant_msg but not included in DB JSON blob. On reload, field missing.
**Fix:** Include `reasoning_content` in the stored JSON and restore it on load.

### 4. Save/load format mismatch
**Was:** Assistant tool_call messages stored as `{"tool_calls": ..., "content": ...}`, but load used `starts_with` check that missed some formats.
**Fix:** Use `contains("\"tool_calls\":")` check. Ensure stored JSON field order matches what the API expects.

### 5. Error recovery stripped history
**Was:** On `reasoning_content` error, entire conversation replaced with just system+user.
**Fix:** Remove only the `reasoning_content` fields from loaded messages instead of dropping everything.

## Cache-Safe Patterns

```
// GOOD: append only
messages.push(new_message);

// BAD: mutate existing
messages[i]["content"] = json!("truncated");

// GOOD: separate copy for token-budget trimming
let mut trimmed = messages.clone();
trim_for_budget(&mut trimmed);
send_to_api(&trimmed);
// messages stays untouched for next iteration

// BAD: mutate and send
trim_for_budget(&mut messages);
send_to_api(&messages);
```

## Not Applicable to Local Models

Local models (llama.cpp) don't have prompt caching in the same way — they use
KV cache which is managed differently. These rules only apply to remote providers
in `crates/llama-chat-web/src/providers/openai_compat.rs`.
