# Agentic Loop — Research Findings & Planned Improvements

_Researched 2026-05-12. Based on analysis of LangChain, AutoGen, LlamaIndex, LobeChat,
Open WebUI, llama.cpp PR #18675, and other open-source agent frameworks._

---

## What We Found

### How Every Major Framework Handles This

| Framework | Loop driver | Y/N check | Tool format detection |
|---|---|---|---|
| LangChain/LangGraph | Server-side (Python) | No — model decides by not calling a tool | Single format per model, schema-driven |
| AutoGen | Server-side | No | OpenAI function calling only |
| LlamaIndex | Server-side | No | Schema-driven |
| LobeChat | Server-side (Node.js) | No | SSE with structured events |
| Open WebUI | Server-side (Python) | No | Per-model format config |
| llama.cpp (PR #18675) | N/A (library) | No | Jinja template introspection → GBNF grammar |

**Universal pattern**: model stops → if it emitted a tool call, execute it and continue;
if it stopped with plain text, the task is done. No secondary inference pass.

---

## Our Current Architecture vs Industry

### What We Do (Current)
```
User message → Backend generates → streams tokens to frontend
→ Frontend detects tool call spans (regex)
→ Frontend sends tool results back as new message
→ Backend generates next turn
→ Backend: if finish=stop && tool_response_tokens > 0 → Y/N inference check
→ If Y/N says "NO" → finish_reason=yn_continue → frontend auto-continues
→ Else frontend also auto-continues for: length, loop_recovery, cuda_deadlock, infinite_loop
```

### What Industry Does
```
User message → Backend: generate → if tool call → execute → inject result → continue
(all in one long-running SSE stream, frontend just renders)
→ Model stops with plain text → done
```

### Problems with Our Approach
1. **Frontend-driven loop** — agentic logic lives in React hooks, not the server
2. **Y/N inference check** — wasteful ~50ms per stop, fires false-positives (model
   writing a summary after tools looks like "incomplete" to the checker)
3. **Regex tool detection on frontend** — fragile, 6+ format variants, causes
   `stripUnclosedToolCallTail` false-positives during streaming
4. **No structured streaming events** — frontend must parse raw text to find tool calls,
   tool results, status messages
5. **KV cache disabled** — each turn restarts from scratch, losing prompt eval savings

---

## Planned Improvements

### Fix 1 — Move Agentic Loop Server-Side _(DONE — 2026-05-13)_

Added server-side auto-continue loop in `crates/llama-chat-web/src/websocket.rs`.

**What changed**:
- `handle_websocket` now loops internally over `'gen_loop` when `finish_reason` is
  `length`, `cuda_deadlock`, `loop_recovery`, or `infinite_loop`.
- A continuation message is injected (`make_server_continuation_message`) and a new
  `bridge.generate` call is made without closing the WebSocket.
- `MAX_SERVER_AUTO_CONTINUES = 3` caps the loop.
- Title generation only runs after the final completion (not per-turn).
- Frontend `AUTO_CONTINUE_REASONS` reduced to `{'tool_continue'}` only.

**Frontend cleanup** (`src/hooks/useGenerationStream.ts`): removed `length`, `cuda_deadlock`,
`loop_recovery`, `infinite_loop` from `AUTO_CONTINUE_REASONS`.

**Not changed**: `tool_continue` still goes through the frontend path (it was already
handled by `token_loop.rs` server-side — the frontend just re-submits the same conversation).
The `yn_continue` path is disabled entirely (see Fix 2).

---

### Fix 2 — Remove Y/N Continue Check _(DONE — 2026-05-12)_

Replaced `quick_task_completion_check` with a comment. The model itself decides
completion by not emitting a tool call. No secondary inference needed.

**Files modified**: `crates/llama-chat-engine/src/generation.rs`

**Impact**: ~50ms savings per completed agentic turn. Eliminates false `yn_continue`
triggers when model writes a summary paragraph after tool use.

---

### Fix 3 — Structured SSE Events _(MEDIUM, not yet started)_

Emit typed events from the server instead of raw token text:

```
event: text
data: {"content": "Let me search for that..."}

event: tool_call
data: {"name": "web_search", "args": {"query": "..."}, "id": "tc_001"}

event: tool_result
data: {"tool_call_id": "tc_001", "content": "...results..."}

event: status
data: {"message": "Searching the web..."}

event: done
data: {"finish_reason": "stop", "tokens_used": 1234}
```

Frontend becomes pure rendering — no regex, no format detection, no false-positives.

**Files**: `crates/llama-chat-engine/src/generation.rs` (emit events),
`src/web/routes/chat_stream.rs` (SSE encoder),
`src/utils/generationStream.ts` (parse typed events),
`src/hooks/useGenerationStream.ts` (remove parsing logic)

---

### Fix 4 — Re-Enable KV Cache _(DONE — 2026-05-13)_

Removed the `state.inference_cache = None` guard at the start of `generation.rs`.

**Root cause was already fixed**: The CUDA deadlock that forced this guard was traced to
IPC pipe over-flushing (fixed in `b8fed73`) and watchdog killing the worker during large
inject_output_tokens calls (fixed in `469c67c`). The generation code itself was never
the cause — confirmed by the `real_pipeline_test` (78 injections, 0 crashes).

**Files modified**: `crates/llama-chat-engine/src/generation.rs` (line 155).

**Impact**: Saves full prompt re-evaluation on each turn (~300-500ms for long convos).
The vision path still explicitly drops the cache (image embeddings can't be cached).

---

### Fix 5 — Simplify Format Detectors _(LOW PRIORITY)_

Currently trying 6+ tool call formats (JSON object, Mistral bracket, Llama3 XML,
GLM XML, SYSTEM.EXEC, Python function call). This causes the `stripUnclosedToolCallTail`
complexity and edge cases.

**llama.cpp approach (PR #18675)**: Parse the Jinja chat template at model load time,
extract which tags the model uses for tool calls, register a GBNF grammar to constrain
output to valid tool call format. One format, grammar-constrained = no ambiguity.

**Our approach**: We already extract `exec_open`/`exec_close`/`output_open`/`output_close`
from the template into `ToolTags`. The next step is using those tags as the _only_
format to detect, eliminating the fallback multi-format scanning.

**Files**: `src/utils/toolFormatUtils.ts` (remove fallback formats),
`src/utils/toolParser.ts`, `src/utils/toolSpanCollectors.ts`

---

## Prioritization

| # | Fix | Impact | Risk | Effort | Status |
|---|-----|--------|------|--------|--------|
| 2 | Remove Y/N check | Medium | Low | Trivial | **DONE** (2026-05-13) |
| 4 | KV cache | Medium | Low | Trivial | **DONE** (2026-05-13) |
| 5 | Format detectors | Low | Low | Small | **DONE** (2026-05-13) |
| 1 | Server-side loop | High | Low | Medium | **DONE** (2026-05-13) |
| 3 | Structured SSE | High | Medium | Large | Pending |

---

## Notes

- The `yn_continue` finish reason is now dead code in the frontend auto-continue logic
  (`AUTO_CONTINUE_REASONS` set in `useGenerationStream.ts`). It can be removed once
  Fix 3 (structured SSE) is implemented and we're confident no old conversations replay it.
- The `loop_recovery` and `infinite_loop` continue reasons are still valid — they come
  from repetition loop detection in `token_loop.rs`, not from the Y/N check.
- `tool_continue` is also still valid — that's the within-turn tool call loop.
