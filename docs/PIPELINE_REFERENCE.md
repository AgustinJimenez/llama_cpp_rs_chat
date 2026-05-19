# llama_cpp_rs_chat — Complete Technical Pipeline Reference

> A deep-dive for engineers wanting to understand the full system: from user message to rendered response, including inference, tool calls, browser automation, and the frontend parsing pipeline.

---

## 1. Architecture Overview

### High-Level Topology

```
┌─────────────────────────────────────────────────────────────────┐
│  Browser / Tauri Window                                         │
│  React + Vite (port 14000)                                      │
└─────────────────────┬───────────────────────────────────────────┘
                      │ WebSocket / SSE / Tauri IPC
┌─────────────────────▼───────────────────────────────────────────┐
│  Web Server (llama_chat_web)  — port 18080                      │
│  src/main_web.rs — hyper HTTP server                            │
│  src/web/routes/  — REST + WebSocket handlers                   │
│  src/web/worker/  — WorkerBridge (IPC to child process)         │
└─────────────────────┬───────────────────────────────────────────┘
          JSON Lines stdin/stdout pipe (IPC)
┌─────────────────────▼───────────────────────────────────────────┐
│  Worker Process (--worker flag)                                 │
│  crates/llama-chat-worker/src/worker/worker_main.rs             │
│  3 threads: stdin reader | main loop | generation               │
│  crates/llama-chat-engine/  — inference engine (llama.cpp)      │
└─────────────────────────────────────────────────────────────────┘
```

### Cargo Workspace Crates

| Crate | Path | Role |
|---|---|---|
| `llama_chat_web` | `src/main_web.rs` | Binary: HTTP server, route dispatch |
| `llama_chat_app` | `src/main.rs` | Binary: Tauri desktop app |
| `llama-chat-engine` | `crates/llama-chat-engine/` | Core inference: generation, prompt building, tool execution, sampler |
| `llama-chat-worker` | `crates/llama-chat-worker/` | Worker process entry, IPC protocol, MCP manager |
| `llama-chat-tools` | `crates/llama-chat-tools/` | 120+ tools: browser, file I/O, shell, Python; browser_session.rs |
| `llama-chat-types` | `crates/llama-chat-types/` | Shared types: `SamplerConfig`, `SharedLlamaState`, `InferenceCache`, IPC types, `ToolTags` |
| `llama-chat-db` | `crates/llama-chat-db/` | SQLite persistence: conversations, messages, config, event log |
| `llama-chat-config` | `crates/llama-chat-config/` | Config loading, per-conversation config |
| `llama-chat-command` | `crates/llama-chat-command/` | Shell execution (streaming, background, CWD/env persistence) |
| `llama-chat-desktop-tools` | `crates/llama-chat-desktop-tools/` | 90+ desktop automation tools (enigo) |

### Two Modes: Web vs Tauri

In **web mode** (`llama_chat_web`), the server listens on port 18080 and spawns the worker as a child process. The Vite frontend runs on port 14000 and proxies API calls. All chat and generation go through HTTP/WebSocket.

In **Tauri mode** (`llama_chat_app`), there is no HTTP server on 18080. The worker is still an out-of-process child, but the frontend communicates via Tauri's `invoke()` IPC. Cloud provider streaming uses a Tauri command (`stream_provider`) instead of the HTTP `/api/providers/*/stream` route.

### Out-of-Process Worker Design

The central design decision is that the LLM model runs in a **separate child process** (`llama_chat_web.exe --worker`). Benefits:

- Killing the worker via `POST /api/model/hard-unload` immediately frees all GPU VRAM.
- Crashes (CUDA deadlock, C++ exception) only kill the worker; the web server survives and auto-restarts.
- The watchdog in `token_loop.rs` calls `std::process::exit(42)` on CUDA deadlock; `ProcessManager` detects exit code 42 and auto-respawns.

The worker's `run_worker()` in `worker_main.rs` starts three threads:

1. **Thread 0** (stdin reader): reads JSON Lines from stdin → `stdin_tx` channel.
2. **Thread 1** (main loop): `crossbeam_channel::select!` between `token_rx` and `stdin_rx`. Tokens are batched at ~30fps (33ms window) to reduce kernel-level pipe pressure during CUDA computation.
3. **Thread 2** (generation, temporary): spawned for each `Generate` command, runs `generate_llama_response`, sends tokens via `TokenData` → `WorkerPayload::Token`.

A critical detail: because the generation loop is synchronous (no `await` yield points), the token forwarding from `tokio::sync::mpsc::UnboundedSender` must happen on a **real OS thread**, not a tokio task (which would be starved on the single-threaded runtime):

```rust
let forward_thread = thread::spawn(move || {
    loop {
        match token_receiver.blocking_recv() {
            Some(token_data) => {
                let response = WorkerResponse::ok(req_id, WorkerPayload::Token { ... });
                if tx_clone.send(response).is_err() { break; }
            }
            None => break, // generation ended
        }
    }
});
```

**Windows stdout corruption prevention**: `printf`/`fprintf(stdout)` from llama.cpp's C code would corrupt the JSON Lines IPC pipe. The worker calls `steal_stdout_for_ipc()` at startup: it `_dup()`s the real stdout fd, then `_dup2(stderr, stdout)` so C-level stdout redirects to stderr. The duplicated fd is the exclusive IPC channel.

### IPC Protocol

`WorkerRequest` and `WorkerResponse` are defined in `crates/llama-chat-types/src/ipc_types.rs` and serialized as JSON Lines.

Key commands:
- `WorkerCommand::Generate { user_message, conversation_id, image_data }` — triggers generation
- `WorkerCommand::LoadModel { model_path, gpu_layers, mmproj_path }` — loads GGUF
- `WorkerCommand::CancelGeneration` — sets `AtomicBool` cancel flag
- `WorkerCommand::GenerateTitle { prompt }` — generates 3–6 word title
- `WorkerCommand::RefreshMcpServers` — reconnects MCP tool servers

Key responses:
- `WorkerPayload::Token { token, tokens_used, max_tokens, status }` — streaming token
- `WorkerPayload::GenerationComplete { finish_reason, gen_tok_per_sec, ... }` — final stats
- `WorkerPayload::LoadingProgress { progress }` — 0–100 during model load, 101 = warmup phase
- `WorkerPayload::GenerationStarted { conversation_id }` — echoes conversation ID at start

The server-side `WorkerBridge` manages:
- `cmd_tx: mpsc::UnboundedSender<String>` — sends JSON to worker stdin
- `pending: HashMap<u64, oneshot::Sender<WorkerPayload>>` — correlates request IDs to response futures
- `active_generation: Option<ActiveGeneration>` — routes streaming tokens to the SSE/WS channel
- `model_meta: Option<ModelMeta>` — cached model info (name, context length, template type)

---

## 2. Token Generation Pipeline

### End-to-End Flow

```
User message (WebSocket or HTTP POST)
  │
  ▼
routes/chat.rs → WorkerBridge.generate()
  │
  ▼  [JSON over pipe]
worker_main.rs: WorkerCommand::Generate
  │
  ▼
run_generation() spawns generation thread
  │
  ▼
generation.rs: generate_llama_response()
  │
  ├─ load_config_for_conversation() → per-conv or global SamplerConfig
  ├─ load_model() if not already loaded
  ├─ resolve_tool_tags() → ToolTags (exec_open/close, output_open/close)
  ├─ maybe_compact_conversation() → auto-compaction if >70% context
  ├─ apply_system_prompt_by_type_with_tags() → formatted prompt string
  │     └─ try_jinja_render() → Jinja2 template via minijinja
  │     └─ fallback: get_universal_system_prompt_with_tags() (hardcoded)
  ├─ evaluate_text_prompt() → KV cache reuse or fresh context
  │
  ▼
token_loop.rs: run_generation_loop()
  │
  ├─ [loop] sampler.sample() → next_token
  ├─ context.decode(batch) → feed token back
  ├─ model.token_to_str() → token string
  ├─ check_stop_conditions() → EOS, stop tokens
  ├─ [if close char] check_and_execute_command_with_tags() → tool call?
  │     ├─ YES: execute tool, inject output tokens, continue
  │     └─ NO: push to response, stream to frontend
  └─ [on stop] return finish_reason
  │
  ▼
generation.rs (post-loop)
  ├─ Store context in inference_cache (KV cache reuse)
  ├─ quick_task_completion_check() → yn_continue check
  └─ Return GenerationOutput { finish_reason, timing, token_breakdown }
  │
  ▼
worker_main.rs: WorkerPayload::GenerationComplete → JSON to pipe
  │
  ▼
WorkerBridge → SSE/WebSocket → useGenerationStream.ts → React state update
```

### Prompt Batching

The tokenized prompt is decoded in chunks of 2048 tokens at a time (`PROMPT_BATCH_CAP = 2048`) via `LlamaBatch::new(batch_cap, 1)`. Only the last token in each batch has `logits=true` (needed for sampling). All other tokens use `logits=false` to avoid wasting VRAM on intermediate logits.

### Token Streaming

Each decoded token is sent via `tokio::sync::mpsc::UnboundedSender<TokenData>`. The forwarding thread converts this to `WorkerPayload::Token` and sends over the crossbeam channel. The main loop batches tokens within a 33ms window before flushing to the IPC pipe (64KB BufWriter). The server-side `WorkerBridge` routes tokens to the SSE or WebSocket channel for the active generation.

### Sampler Chain (11 types)

Configured by `SamplerConfig`. The chain order in `sampler.rs` is:

```
penalties → DRY → top_n_sigma → [top-k, top-p, min-p, typical-p, temperature] → GBNF grammar → terminal (dist or greedy)
```

`LlamaSampler::temp()` alone crashes — it must always be followed by a terminal sampler (`dist` or `greedy`). The GBNF grammar (`tool_grammar.rs`) is lazy: it only activates when the model outputs `{"name"` as a trigger, so it is transparent for XML-format tool calls.

### Context Size and Guard

Default context cap is 32768. If the tokenized prompt exceeds 95% of context size, generation is refused with an error. During generation, the 95% guard fires at the token level to avoid `NoKvCacheSlot` decode failures. Both the prompt phase and the injection phase check this guard independently.

---

## 3. Tool Call System

### Tool Tags and Format Detection

The system supports 6+ distinct tool call formats, all handled by a unified pipeline. Each model family has a `ToolTags` struct with four fields:

```rust
pub struct ToolTags {
    pub exec_open: String,    // e.g. "<tool_call>"
    pub exec_close: String,   // e.g. "</tool_call>"
    pub output_open: String,  // e.g. "<tool_response>"
    pub output_close: String, // e.g. "</tool_response>"
}
```

The `MODEL_TAG_MAP` in `tool_tags.rs` maps `general.name` GGUF metadata strings to tag families:

| Family | exec_open | exec_close |
|---|---|---|
| Qwen/GLM | `<tool_call>` | `</tool_call>` |
| Mistral | `[TOOL_CALLS]` | `[/TOOL_CALLS]` |
| Harmony | `<\|start\|>tool<\|message\|>` | `<\|end\|>` |
| LFM2 (Liquid AI) | `<\|tool_call_start\|>` | `<\|tool_call_end\|>` |
| Gemma 4 | `<\|tool_call>` | `<tool_call\|>` |
| Default | `<\|\|SYSTEM.EXEC>` | `<SYSTEM.EXEC\|\|>` |

Tag resolution priority (in `prompt_builder.rs: resolve_tool_tags()`):
1. Saved `tag_pairs` from DB config (user explicitly chose in Load Model modal).
2. Auto-detect from `general.name` GGUF field (exact match, then fuzzy normalized substring).
3. Legacy override fields + default SYSTEM.EXEC tags.

> **SYNC requirement**: `tool_tags.rs: MODEL_TAG_MAP` and `src/config/modelPresets.ts: MODEL_TOOL_TAGS` must be kept in sync. The backend uses the map for inference-time tag resolution; the frontend uses it for rendering-time format selection.

### Tool Detection in the Token Loop

A fast gate checks every token for close characters (`>`, `]`, `}`) before calling the expensive detector. This skips ~90% of tokens.

`command_executor.rs` uses `FORMAT_PRIORITY` — an ordered array of 6 detector functions — first match wins:
1. `model_specific` — matches the model's own `exec_open/exec_close` via regex
2. `exec` — SYSTEM.EXEC format
3. `llama3` — `<function=name><parameter=k>v</parameter></function>` XML
4. `harmony` — Harmony channel format
5. `mistral_bracket` — `[TOOL_CALLS]name[ARGS]{json}`
6. `mistral_json` — `[TOOL_CALLS]{...json...}[/TOOL_CALLS]`

### Tool Dispatch (`command_executor.rs`)

Once a tool call is detected, `check_and_execute_command_with_tags()` runs:

1. **Loop detection**: checks if the same command was recently executed. Returns `LoopCheckResult::ForceStop` or `LoopCheckResult::Blocked` if repeating excessively.
2. **Parse all calls**: `try_parse_all_from_raw()` extracts one or more `(name, args)` pairs. Batch calls are parallel (read-only tools) or serial (write tools), up to `MAX_PARALLEL_TOOLS = 10` concurrent via `std::thread::scope`.
3. **`spawn_agent` special case**: needs model/backend access, routed to `run_sub_agent()`.
4. **`execute_command` special case**: routes through `execute_command_streaming()` (live line-by-line output) or `execute_command_background()` (fire-and-forget with PID).
5. **Security checks**: `detect_command_injection()` blocks `curl|base64`, `eval+wget`. `detect_destructive_command()` warns on `rm -rf`, `DROP TABLE`, etc.
6. **`run_native_tool_with_timeout()`**: dispatches to the tool catalog. After execution, `quick_tool_result_check()` annotates with `[TOOL_RESULT:success]` or `[TOOL_RESULT:error]`.
7. **Summarization**: outputs >8K chars are summarized via `summarize_tool_output()` using a separate small context. The model sees the summary; the user/DB sees the full output. A `summary=false` argument opts out.

### Tool Response Injection

After execution, the tool output is tokenized and injected mid-generation:

```rust
// In token_loop.rs after exec_result is returned:
inject_output_tokens(&exec_result.model_tokens, batch, context, &mut gen.token_pos, ...);
sampler.accept_many(&injected_tokens);
std::thread::sleep(Duration::from_millis(50)); // CUDA settle
context.synchronize();
```

`inject_output_tokens()` decodes tokens **one at a time** — matching the normal generation path exactly. Batch injection was found to leave the GPU in a bad state causing CUDA deadlocks. Single-token injection is ~1ms/token vs ~0.05ms/token batched but is deadlock-free.

The injection wraps the tool output in the model's chat template turn structure via `wrap_output_for_model()`. The content seen by the model is:
```
[tool response turn start]
<output_open>
{result}
<output_close>
[assistant turn start]
```

### Tool Schema System

Only 24 core tools (~4K tokens) appear in every system prompt. Desktop tools (90+) and MCP tools are discoverable on demand via `list_tools(category)` and `get_tool_details(tool_name)`. 22 core tools have full JSON schema validation (required params, type coercion) before execution.

---

## 4. Browser Tool — Multi-Backend Fallback Chain

### Fallback Chain

```
Tauri WebView (HTTP to MCP bridge at 127.0.0.1:18091)
  ↓ (if unavailable)
wry native WebView (feature = "wry-browser")
  ↓ (if unavailable)
Chrome CDP (feature = "cdp", headless_chrome crate, headless: false)
  ↓ (if unavailable)
curl HTTP fetch (raw HTML, no JavaScript)
```

### `browser_session.rs`

The `BrowserSession` trait defines: `navigate`, `click`, `type_text`, `eval`, `html`, `screenshot`, `wait_for`, `press_key`, `snapshot`, `get_full_text`, `close`, `url`.

Primary implementation: `TauriHttpSession`:

```rust
pub struct TauriHttpSession {
    pub current_url: String,
    cached_html: Option<String>,
    cached_text: Option<String>,
}
```

Static globals track active session state:
```rust
static ACTIVE_URL: Mutex<Option<String>> = Mutex::new(None);
static CACHED_HTML: Mutex<Option<String>> = Mutex::new(None);
static CACHED_TEXT: Mutex<Option<String>> = Mutex::new(None);
```

`open_session(url)` fetches and caches the page immediately so subsequent `browser_get_text` calls are instant.

**`do_fetch()` — core page-reading function**:
1. Sends `browser_navigate` to the Tauri bridge (HTTP POST to `127.0.0.1:18091/bridge/browser/navigate`).
2. Polls `document.readyState` and `document.body?.innerText?.length` via `eval_in_browser_panel()` until ready (up to 15 seconds).
3. Runs cookie/consent banner dismissal JavaScript twice (with 1.5s delay for async CMPs like OneTrust, Cookiebot).
4. Reads `document.documentElement.outerHTML` via JavaScript eval.
5. Falls back to `curl_fetch()` if the eval returns non-HTML.

**`eval_in_browser_panel(js)` — full fallback chain**:
- Probes Tauri availability (HTTP POST to `/api/eval`, 500ms timeout, result cached 30 seconds).
- If Tauri: 3 retries to `18091/api/eval` with `{"js": js, "target": "browser-panel"}`.
- If wry compiled: `crate::wry_browser::evaluate(js)`.
- If cdp compiled: `cdp::evaluate(js)` (headless_chrome singleton).
- Otherwise: error.

**`strip_html(html)`** — zero-regex byte-level state machine. Removes `<script>` and `<style>` blocks entirely, replaces tags with spaces, collapses whitespace, decodes HTML entities.

### Cookie/Consent Banner Dismissal

30+ selector patterns covering OneTrust, Cookiebot, TrustArc, Evidon, and generic button patterns (by ID, class, aria-label, visible text). Fires twice — immediately and after 1.5 seconds — because CMPs often render asynchronously.

### Web Mode Limitation

In web mode (no Tauri running), all browser calls fall through to `curl_fetch()`:
- `browser_navigate` + `browser_get_text` work for static/server-rendered sites.
- `browser_search` (Google) fails — Google blocks curl.
- Full JS browsing requires the Tauri app.

---

## 5. yn_continue / Auto-Continue Mechanism

### Finish Reasons That Trigger Auto-Continue

The frontend's `useGenerationStream.ts` recognizes six finish reasons:

```typescript
const AUTO_CONTINUE_REASONS = new Set([
  'length',        // Context window filled up
  'yn_continue',   // Y/N check said task is incomplete
  'loop_recovery', // Stall or repetition loop detected
  'tool_continue', // Tool injection requires fresh context restart
  'cuda_deadlock', // CUDA sync deadlock detected by sampler
  'infinite_loop', // Infinite tool call loop detected
]);
```

### yn_continue: The Task Completion Check

After the model stops naturally (`finish_reason = "stop"`) AND tool calls were made during the turn (`tool_response_tokens > 0`), the backend performs a quick binary Y/N inference:

```rust
// generation.rs (post-loop)
if gen.finish_reason == "stop" && gen.tool_response_tokens > 0 {
    let check_text = format!(
        "USER REQUEST: {user_prefix}\n\nASSISTANT RESPONSE TAIL:\n{response_tail}"
    );
    let is_complete = quick_task_completion_check(
        model, &state.backend, chat_template_string, &conversation_id, &check_text,
    );
    if !is_complete {
        gen.finish_reason = "yn_continue".to_string();
    }
}
```

`quick_task_completion_check()` in `sub_checks.rs`:
1. Formats a prompt with binary rules ("If only some items requested are done → NO", "If response ends with tool output but no final summary → NO").
2. Creates a **fresh 1024-token context** (does not touch `inference_cache`).
3. Samples 1–5 tokens at temperature 0.1.
4. `YES` or `Y` → complete; anything else → incomplete.
5. Takes ~50ms on GPU.

### Frontend Auto-Continue Logic

```typescript
if (shouldAutoContinue && (isToolContinue || autoContinueCountRef.current < MAX_AUTO_CONTINUES)) {
  if (!isToolContinue) autoContinueCountRef.current += 1;
  setTimeout(() => {
    startGeneration({
      prompt: continueMsg,
      conversationId: convId,
      autoContinue: true,
    }, assistantMessageId);   // same assistantMessageId → appends to same message
  }, CONTINUE_DELAY_MS);      // 150ms delay
}
```

`MAX_AUTO_CONTINUES = 3` for non-tool-continue reasons. `tool_continue` is unlimited.

Continue message by reason:
- `loop_recovery` / `infinite_loop`: "[SYSTEM] Infinite loop detected — STOP your current approach..."
- Other: "Continue working on this task: '{first 200 chars}'. Pick up where you left off."

> **Known gap**: Auto-continue is frontend-driven. The SSE stream ends with `[DONE]` and the frontend sends a new request. Headless/API-only usage cannot auto-continue because there's no frontend. The planned fix is to move auto-continue logic into the backend's token loop or worker bridge.

### streamSeq and Token Discard

Each call to `startGeneration` increments `streamSeqRef.current`. The `onToken` callback guards:

```typescript
onToken: (token) => {
  if (streamSeqRef.current !== streamSeq) return; // discard if stale
  setMessages((prev) => ...append token...);
}
```

This means if `yn_continue` fires while the first stream is still delivering tokens, those tokens are silently discarded in React state. The DB always has the correct full content (written by the server). To compensate, `onComplete` now reloads the final message content from DB:

```typescript
// After normal completion (not auto-continue):
getConversation(convId).then((data) => {
  const dbMsg = data.messages.find(m => m.id === assistantMessageId);
  if (dbMsg && msg.content !== dbMsg.content) {
    setMessages(prev => prev.map(m => m.id === assistantMessageId
      ? { ...m, content: dbMsg.content } : m));
  }
});
```

---

## 6. Frontend Message Parsing

### `buildSegments()` — Main Entry Point

`src/utils/toolSpanCollectors.ts`:

```typescript
export function buildSegments(content: string, toolTags?: ToolTags): MessageSegment[]
```

A `MessageSegment` is one of:
- `{ type: 'text'; content: string }`
- `{ type: 'tool_call'; toolCall: ToolCall }`
- `{ type: 'thinking'; content: string }`

**Processing pipeline**:
1. `moveToolsOutOfThinking(content)` — extracts tool call+response pairs from inside `<think>` blocks and places them after the thinking block, so they render as widgets.
2. Strip all `<think>...</think>` blocks.
3. Strip unclosed `<think>` tag (streaming — model is mid-thought).
4. Strip orphan `</think>` tags.
5. `stripUnclosedToolCallTail(cleaned, toolTags)` — removes incomplete tool call markup at the tail.
6. `selectToolSpans(pruned, toolTags)` — runs format-specific collectors.
7. `collectExecSpans(pruned)` — SYSTEM.EXEC format collector.
8. Sort all spans by start position.
9. Build `MessageSegment[]`: text between spans → `type: 'text'`; span segments directly.

### `stripUnclosedToolCallTail()`

`src/utils/toolFormatUtils.ts`. Removes incomplete tool call markup during streaming so raw tags don't flash in the UI:

- **With `toolTags`** (model loaded): uses `exec_open`/`exec_close` to find the last unclosed open tag. GLM special case: also accepts `<|end_of_box|>` as an implicit close. Mistral special case: since `[/TOOL_CALLS]` is never emitted in bracket format, checks for complete JSON arguments instead. An open tag is NOT stripped if a `<tool_response>` follows it (tool executed).
- **Without `toolTags`** (old conversations): checks all known formats simultaneously.
- **Always**: checks `<function=` (Llama3 format). An unclosed `<function=` is NOT stripped if a `</tool_call>`, `<|end_of_box|>`, or `<tool_response>` follows it — meaning the function was executed (possibly via a yn_continue continuation that regenerated the call in a different format after EOS hit mid-call).

### Format-Specific Span Collectors

| Collector | Format | File |
|---|---|---|
| `collectQwenSpans` | `<tool_call>{json}</tool_call>` | `toolSpanCollectorsQwen.ts` |
| `collectMistralSpans` | `[TOOL_CALLS]...[/TOOL_CALLS]` | `toolSpanCollectors.ts` |
| `collectLlama3Spans` | `<function=name><parameter=k>v</parameter></function>` | `toolSpanCollectors.ts` |
| `collectGemma4Spans` | `<\|tool_call>call:name{args}<tool_call\|>` | `toolSpanCollectorsGemma4.ts` |
| `collectLfm2Spans` | `<\|tool_call_start\|>[func()]<\|tool_call_end\|>` | `toolSpanCollectorsLfm2.ts` |
| `collectExecSpans` | `<\|\|SYSTEM.EXEC>...<SYSTEM.EXEC\|\|>` | `toolSpanCollectors.ts` |

`selectToolSpans()` tries in priority order: Gemma4 → LFM2 → Qwen → Mistral → Llama3.

Each collector returns `Span[]` with `{ start, end, segment }`. The last call in a series can be streaming: if no matching response tag is found, `findStreamingResponse()` looks for an unclosed `<output_open>` tag and marks the span `isPending: true, isStreaming: true`.

Stable tool call IDs are generated as `tc-{name}-{charPos}` to prevent React key changes from resetting expand/collapse state during streaming.

### Qwen Span Collector — Multi-Format Body Parsing

`collectQwenSpans` handles the `<tool_call>` tag with 5 fallback body parsers:

1. Standard JSON: `{"name": "...", "arguments": {...}}`
2. GLM-4.7 "name{json}" format: `func_name{"key": "val"}`
3. GLM native XML args: `name\n<arg_key>k</arg_key>\n<arg_value>v</arg_value>`
4. Llama3 closed: `<function=name>...<\/function>` inside `<tool_call>`
5. Llama3 open: `<function=name>` without `</function>`
6. Fallback: extract name from any `name=` or `name:` pattern; render as raw widget

The `TOOL_CALL_REGEX` uses a lookahead so `<tool_call>` without `</tool_call>` is still matched if `<tool_response>` follows:
```
/<tool_call>([\s\S]*?)(?:<\/tool_call>|<\|end_of_box\|>|(?=\s*<tool_response>))/g
```

### MD Rendering

Text segments → Markdown renderer. Tool call segments → `CommandExecBlock` or `ToolCallBlock` React components. Thinking segments → `ThinkingBlock` (collapsible, with streaming support). Screenshots in tool output → `<img>` tags via `/api/images/` route.

---

## 7. KV Cache / Inference Cache

### `InferenceCache` Struct

```rust
pub struct InferenceCache {
    pub context: LlamaContext<'static>,   // 'static via transmute — owns the KV cache
    pub conversation_id: String,
    pub evaluated_tokens: Vec<LlamaToken>,// full prompt + generated tokens
    pub context_size: u32,
    pub offload_kqv: bool,
    pub flash_attention: bool,
    pub cache_type_k: String,
    pub cache_type_v: String,
}
```

The `LlamaContext<'static>` lifetime is obtained via `std::mem::transmute` — the context actually borrows the model, but the model lives as long as the state, so this is safe in practice.

### Cache Reuse Logic (`context_eval.rs`)

At the start of each generation turn, `evaluate_text_prompt()`:

1. Takes the cached `InferenceCache` out of `state.inference_cache`.
2. Checks compatibility: same `conversation_id`, `context_size`, `offload_kqv`, `flash_attention`, cache types.
3. Counts the **longest common prefix** of cached tokens vs new prompt tokens.
4. If prefix == cached length (cache is a true prefix), reuses the context and only evaluates new tokens.
5. If the prompt diverged (e.g. message editing), drops the old context and creates a fresh one.

After generation, cache is rebuilt with all evaluated tokens:
```rust
state.inference_cache = Some(InferenceCache {
    context,
    evaluated_tokens: all_evaluated, // prompt tokens + generated tokens
    ...
});
```

### System Prompt Warmup

After model load, `warmup_system_prompt()` pre-evaluates the system prompt into a KV cache entry stored under `"__warmup__"`. The first real generation sees this as a compatible prefix and skips re-evaluating the system prompt.

### Current Limitation

```rust
// generation.rs line ~161:
// Drop inference cache before each generation to avoid CUDA deadlock
// in sample() when tool response tokens are injected into a reused context.
state.inference_cache = None;
```

KV cache reuse is currently disabled per-turn. The warmup cache from model load still works (first turn only). This is a known TODO.

### KV Cache Quantization

Supported types: `f32`, `f16`, `q8_0`, `q4_0`, `q4_1`, `q5_0`, `q5_1`, plus TurboQuant `tq2_0`, `tq3_0`, `tq4_0`. Using `q8_0` reduces KV VRAM by ~50% vs `f16` with minimal quality loss.

### `conversation_context` Table

SQLite table caches system prompt token count and tool definition token count per conversation, keyed by a content hash. Avoids re-tokenizing the ~14K-token tool definitions on every turn.

---

## 8. Vision / MTMD Pipeline

### Compilation Gate

Vision support is gated behind `#[cfg(feature = "vision")]`, but all build scripts include it.

### Model Setup

`model_manager.rs` auto-detects a `mmproj` GGUF file in the same directory as the main model. If found, a `MtmdContext` (multimodal tokenizer/projector context) is initialized and stored in `state.vision_state`.

### Image Input Path

In `generation.rs`, if `image_data` is non-empty and `state.vision_state.is_some()`:

1. Base64-decode each image (stripping `data:image/...;base64,` prefix).
2. Create `MtmdBitmap` from raw bytes.
3. `inject_media_markers()` inserts `<__media__>` placeholders before the user's message.
4. `MtmdContext::tokenize()` produces "chunks" — mix of text token runs and image embedding tensors.
5. `chunks.eval_chunks()` evaluates all chunks into the context at position 0.
6. The returned `n_past` value becomes the starting `token_pos` for generation.

### Vision Tool Response Injection

When a tool returns images (e.g. `take_screenshot`) and the model has vision capability, `inject_tool_response_with_vision()` is called instead of `inject_output_tokens()`:

1. Prepends `<__media__>\n` markers to the model injection block text.
2. Creates `MtmdBitmap` from each raw image.
3. Tokenizes via `MtmdContext::tokenize()`.
4. Calls `chunks.eval_chunks()` continuing from the current `token_pos`.
5. Updates `token_pos` to the new `n_past`.

---

## 9. Jinja Chat Template System

### Template Extraction

`gguf_utils.rs` reads from GGUF metadata when loading a model:
- `tokenizer.chat_template` → stored in `state.chat_template_string`
- `tokenizer.chat_template_type` → for fallback selection
- `general.name` → for model tag lookup
- `llama.block_count` → for GPU layer capping
- `general.context_length` → for context size suggestion
- `general.sampling.*` → for auto-configured sampler parameters

### Template Rendering (`jinja_templates.rs`)

Uses the `minijinja` crate. `preprocess_template()` converts Python idioms to minijinja syntax:

| Python | minijinja |
|---|---|
| `.endswith("x")` | ` is endingwith("x")` |
| `.startswith("x")` | ` is startingwith("x")` |
| `.strip()` | ` \| trim` |
| `.items()` | ` \| items` |

Custom functions registered in the environment:
- `raise_exception(msg)` — used by GLM-4.6, Devstral, Ministral templates
- `strftime_now(fmt)` — current date formatting (Mistral templates inject the date)

Template context variables:
```rust
context! {
    messages => messages,
    tools => &tools_vec,              // OpenAI function schemas
    add_generation_prompt => true,
    bos_token => bos_token,
    eos_token => eos_token,
    enable_thinking => false,         // prevents GLM-4 from entering <think> mode
}
```

### Tool Definitions in Jinja Templates

`get_available_tools_openai_with_mcp()` generates OpenAI-format function calling schemas for all 24 core tools plus active MCP tools. These are passed as `tools` to the Jinja template. Models with `{% if tools %}` blocks in their templates inject tool definitions natively into the formatted prompt.

### Fallback to Hardcoded Templates

`templates.rs: apply_system_prompt_by_type_with_tags()` first calls `try_jinja_render()`. If that fails or is absent, it falls back to `get_universal_system_prompt_with_tags()` — a hardcoded template that explicitly lists all 24 tools with their formats using the model's `exec_open/exec_close` strings interpolated in.

### Behavioral System Prompt

`get_behavioral_system_prompt()` generates the behavior-only portion (no tool format, no tool list). It includes:
- Core behavior rules (autonomous operation, no manual instructions to user, always use `write_file` not `echo`)
- Tool usage guidelines (use `search_files` not grep, `browser_search` not curl)
- Background process rules, sub-agent guidance
- Environment block: current date/time, OS, CWD, shell type

---

## 10. Key Files Reference

### Backend (Rust)

| File | Role |
|---|---|
| `src/main_web.rs` | Binary entry; route dispatch table; server startup; single-instance enforcement |
| `crates/llama-chat-worker/src/worker/worker_main.rs` | Worker process: 3-thread design, stdin reader, command dispatch, generation thread lifecycle |
| `crates/llama-chat-worker/src/worker/worker_bridge.rs` | Server-side IPC abstraction: request correlation, token routing, model metadata cache |
| `crates/llama-chat-worker/src/worker/process_manager.rs` | Spawns/kills/monitors worker child; detects exit code 42 and auto-restarts |
| `crates/llama-chat-types/src/ipc_types.rs` | Shared IPC protocol: `WorkerRequest`, `WorkerResponse`, `WorkerCommand`, `WorkerPayload` |
| `crates/llama-chat-engine/src/generation.rs` | `generate_llama_response()`: main generation orchestrator; config, prompt, context, KV cache, yn_continue |
| `crates/llama-chat-engine/src/token_loop.rs` | `run_generation_loop()`: token-by-token sampling, tool call detection, watchdog (10s CUDA deadlock), loop detection |
| `crates/llama-chat-engine/src/command_executor.rs` | Tool call detection, dispatch, result injection; `check_and_execute_command_with_tags()`, `inject_output_tokens()` |
| `crates/llama-chat-engine/src/tool_tags.rs` | `MODEL_TAG_MAP`: model name → ToolTags; `derive_tool_tags_from_pairs()` |
| `crates/llama-chat-engine/src/prompt_builder.rs` | `resolve_tool_tags()`, `warmup_system_prompt()`, `snapshot_context_overhead()`, vision injection |
| `crates/llama-chat-engine/src/context_eval.rs` | `evaluate_text_prompt()` with KV cache reuse; `create_fresh_context()`; `parse_kv_cache_type()` |
| `crates/llama-chat-engine/src/templates.rs` | `apply_system_prompt_by_type_with_tags()`: Jinja render → fallback; 24 tools hardcoded |
| `crates/llama-chat-engine/src/jinja_templates.rs` | `apply_native_chat_template()`: minijinja render with Python compatibility preprocessing |
| `crates/llama-chat-engine/src/sub_checks.rs` | `quick_task_completion_check()` (yn_continue); `quick_tool_result_check()` (success/error); `generate_title_text()` |
| `crates/llama-chat-engine/src/sampler.rs` | `create_sampler()`: 11-type chain builder |
| `crates/llama-chat-engine/src/tool_grammar.rs` | Lazy GBNF grammar for JSON tool calls; activates only on `{"name"` trigger |
| `crates/llama-chat-engine/src/stop_conditions.rs` | Stop sequence checking; `ExecBlockTracker` — suppresses stop tokens inside tool call blocks |
| `crates/llama-chat-engine/src/loop_detection.rs` | Detects repeated identical tool calls; fuzzy dedup; HTTP error hints |
| `crates/llama-chat-engine/src/model_manager.rs` | `load_model()`: GGUF loading, VRAM calculation, mmproj detection, metadata extraction |
| `crates/llama-chat-engine/src/vram_calculator.rs` | `read_gguf_block_count()`, VRAM-based GPU layer calculation; caps ngl at block_count |
| `crates/llama-chat-engine/src/compaction.rs` | `maybe_compact_conversation()`: LLM map-reduce summarization at 70% context |
| `crates/llama-chat-engine/src/tool_output.rs` | `summarize_tool_output()`, `wrap_output_for_model()`, `tool_use_one_liner()` |
| `crates/llama-chat-engine/src/sub_agent.rs` | `run_sub_agent()`: mini generation loop in isolated context for `spawn_agent` tool |
| `crates/llama-chat-tools/src/browser_session.rs` | `TauriHttpSession`, `BrowserSession` trait, `eval_in_browser_panel()`, fallback chain, cookie banner dismissal |
| `crates/llama-chat-tools/src/browser_tools.rs` | `browser_search`, `browser_navigate`, `browser_get_text`, `browser_click`, `browser_query` tool implementations |
| `crates/llama-chat-command/src/lib.rs` | `execute_command_streaming()`, `execute_command_background()`, CWD/env persistence, PowerShell fallback |
| `src/web/native_tools/mod.rs` | Tool catalog dispatch: 24 core tools + `list_tools`/`get_tool_details`; MCP proxy; input validation |
| `src/web/routes/chat.rs` | `handle_websocket_chat_stream()`, `handle_post_chat_stream()` (SSE); conversation logger setup |

### Frontend (TypeScript/React)

| File | Role |
|---|---|
| `src/hooks/useGenerationStream.ts` | Core streaming hook: stream sequence tracking, token accumulation, auto-continue (6 reasons), DB reload on complete |
| `src/hooks/useChat.ts` | Chat orchestration: sends messages, creates assistant message placeholder, calls `startGeneration` |
| `src/utils/toolSpanCollectors.ts` | `buildSegments()`, `moveToolsOutOfThinking()`, `selectToolSpans()`, all format collectors |
| `src/utils/toolFormatUtils.ts` | `stripUnclosedToolCallTail()`, `extractBalancedJson()`, `parsePythonFunctionCall()`, `findStreamingResponse()` |
| `src/utils/toolSpanCollectorsQwen.ts` | Qwen/GLM `<tool_call>` format parser with 6 body format fallbacks |
| `src/utils/toolSpanCollectorsGemma4.ts` | Gemma 4 format parser |
| `src/utils/toolSpanCollectorsLfm2.ts` | LFM2 Python function call format parser |
| `src/utils/chatTransport.ts` | HTTP/WebSocket abstraction; handles web mode (HTTP) and Tauri mode (invoke) |
| `src/utils/generationStream.ts` | `createGenerationStream()`: normalizes local vs cloud provider streaming into unified callbacks |
| `src/config/modelPresets.ts` | `MODEL_TOOL_TAGS`, `MODEL_TAG_PAIRS`: frontend mirror of `tool_tags.rs` (must stay in sync) |
| `src/hooks/useMessageParsing.ts` | Per-message parsing: thinking/tool_call extraction, Harmony format, RAW/MD/TXT view dispatch |
| `src/molecules/CommandExecBlock.tsx` | Tool execution widget: animated state, collapsed CLI-style summary, status badge |
| `src/molecules/ThinkingBlock.tsx` | Collapsible reasoning block with streaming unclosed `<think>` detection |

---

## 11. Cross-Cutting Concerns

### Windows-Specific Pitfalls

- **All spawned child processes** must use `CREATE_NO_WINDOW` (0x08000000) to prevent terminal flicker.
- **All child processes** must use `stdin(Stdio::null())` — otherwise children inherit the worker's IPC stdin pipe and MSYS2 tools (wc, grep) hang forever waiting for EOF.
- PowerShell is used for shell builtins and commands with `|`, `>`, `&&`. Direct `Command::new` for everything else.
- Model loading: kill `llama_chat_web.exe` before rebuilding (Windows file locking).

### GPU Layer Safety

`ngl` (n_gpu_layers) must never exceed the model's `block_count`. Setting ngl higher offloads the output/embedding layer to GPU, causing `llama_decode` to hang on hybrid MoE+Mamba2 models (Qwen3.5) with large contexts. `vram_calculator.rs` reads `block_count` from GGUF and caps automatically.

### Watchdog + Tool Execution

The 10-second watchdog in `token_loop.rs` is paused (`watchdog_paused.store(true, ...)`) during:
- Tool execution (browser fetch can take 20+ seconds)
- Token injection (large tool responses take seconds to decode one-by-one)

After each pause, the heartbeat is reset so the watchdog has a fresh 10-second baseline.

### Performance Numbers (RTX 4090)

- ~80–95 tok/s decode (Qwen3.5-35B-A3B IQ4_XS, 32K context)
- Full CUDA rebuild: ~27 minutes; incremental Rust-only: ~20 seconds
- Tool definitions: ~14K tokens (~43% of a 32K context)
- Decode-only `llama_decode` benchmark: 91.12 tok/s (Rust = C, zero FFI overhead)

### Context Compaction

`maybe_compact_conversation()` fires at 70% context usage. Uses LLM map-reduce: splits conversation into chunks, summarizes each chunk, then synthesizes a compact conversation that fits in context while preserving key facts and tool call history.

### Single Server Instance Enforcement

`enforce_single_instance()` in `main_web.rs` writes the current PID to `assets/server.pid` on startup and reads + kills the previous PID if present. Prevents multiple server instances accumulating on rapid restarts.
