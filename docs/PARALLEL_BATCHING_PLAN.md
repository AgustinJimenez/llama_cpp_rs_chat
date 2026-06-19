# Parallel Tool Batching via KV Cache Rollback

## Problem

Models like Qwen3.6 are trained to expect `<tool_response>` after every `</tool_call>`. This makes
them generate sequential tool calls even when asked to run them in parallel. The `<parallel_calls>`
fence feature (already implemented) works but requires the model to adopt a custom format, which
Qwen refuses due to training momentum.

## Goal

Transparent parallel batching: detect multiple consecutive `write_file` (or other independent)
calls and execute them concurrently, without requiring any model format change.

## Proposed Mechanism: KV Cache Rollback Batching

### State machine

```
NORMAL → detect write_file call
         execute tool in background thread
         save KV cache snapshot at position of </tool_call>
         → BUFFERING

BUFFERING → model generates next tokens (likely <tool_response> garbage or <think>)
           → if another write_file detected within window:
               add to batch, keep background-executing it
               save new KV snapshot
               continue → BUFFERING
           → if non-batchable content OR EOS:
               wait for all background executions to finish
               RESTORE KV cache to first snapshot position
               discard all tokens generated since snapshot
               inject ONE combined <tool_response> with all results
               resume generation → NORMAL
```

### Why rollback is needed

After `</tool_call>`, Qwen generates `<tool_response>` itself (training reflex). Those tokens
pollute the KV cache. We must roll back to the `</tool_call>` position before injecting the real
combined response, otherwise the model context is inconsistent.

### llama.cpp APIs to use

- `llama_kv_cache_seq_rm(ctx, seq_id, p0, p1)` — remove tokens [p0, p1) from sequence
- Track `n_past` (token position) at the moment each `</tool_call>` is detected
- After collecting all writes: call `llama_kv_cache_seq_rm` from first snapshot position to current
- Then `llama_decode` the combined response tokens
- Resume generation from the new `n_past`

### Key invariants to maintain

1. Background tool execution starts IMMEDIATELY when `</tool_call>` is detected (don't wait)
2. Rollback position = `n_past` at the END of the `</tool_call>` token (inclusive)
3. Combined response format: same as current batch output — `[Tool 1: write_file]\n...\n[Tool 2: write_file]\n...`
4. Batchable tools (safe to run in parallel): write_file, execute_command (different paths), web_fetch
5. Non-batchable: read_file (might read a file written in same batch), edit_file (same path risk)

### Risks / open questions

- KV snapshot/restore cost: `llama_kv_cache_seq_rm` is O(n tokens) — should be fast but needs profiling
- Window timeout: how long to wait for a second tool call before committing the single-call path?
  Suggestion: 0 timeout — only batch if model generates another `</tool_call>` BEFORE generating
  non-tool content. No artificial delay.
- If the model generates `<tool_response>` itself before the second call, those tokens are discarded.
  Verify the model doesn't "remember" them via any path other than KV cache.
- Multiple sequences: if llama.cpp uses sequence IDs, make sure we operate on the right seq_id.

### Implementation sketch (token_loop.rs)

```rust
// State added to TokenGenState
struct PendingBatch {
    calls: Vec<(String, serde_json::Value)>,  // buffered tool calls
    results: Vec<JoinHandle<ToolResult>>,      // background execution handles
    rollback_pos: i32,                         // n_past to roll back to
}

// When tool call detected and it's batchable:
if is_batchable_tool(&tool_name) {
    let handle = std::thread::spawn(move || run_tool(...));
    gen.pending_batch.get_or_insert_default().calls.push(call);
    gen.pending_batch.as_mut().unwrap().results.push(handle);
    gen.pending_batch.as_mut().unwrap().rollback_pos = gen.token_pos;
    // DON'T inject response yet — let model continue
    continue 'token;
}

// When non-batchable content detected while batch is pending:
if let Some(batch) = gen.pending_batch.take() {
    // rollback KV cache
    llama_kv_cache_seq_rm(ctx, 0, batch.rollback_pos, -1);
    // wait for all results
    let results = batch.results.into_iter().map(|h| h.join().unwrap()).collect();
    // inject combined response
    inject_batch_response(&results, ...);
}
```

## Status

- `<parallel_calls>` fence: **IMPLEMENTED** (engine works, model adoption is the blocker)
- KV rollback batching: **PLANNED** (this document)

## Files to modify

- `crates/llama-chat-engine/src/token_loop.rs` — main state machine
- `crates/llama-chat-engine/src/token_loop/shared.rs` — add `pending_batch` to `TokenGenState`
- `crates/llama-chat-engine/src/command_executor/batch_exec.rs` — reuse for combined response
- May need to expose `llama_kv_cache_seq_rm` via llama-cpp-rs FFI if not already wrapped
