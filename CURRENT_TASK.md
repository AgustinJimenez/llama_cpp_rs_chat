# Current Task

Investigating and fixing two bugs found 2026-06-16 while testing the Qwen 3.5 35B
agent on the standard "hacker news" stress-test prompt:

> check online about 5 hacker news posts, get into each one, read it all and make a summary of each one

Conversation under test: `chat_2026-06-16-18-44-35-877`. Model was unloaded by the
user after the hang (Bug #2) was confirmed, so the live repro is gone — but the
DB/log artifacts for that conversation should still exist and can be re-read if
needed (`assets/llama_chat.db`, conversation name `chat_2026-06-16-18-44-35-877`).

This file is the persistent scratchpad for this investigation — update it as
findings/fixes land so work survives a context compaction or a new session.

---

## Bug #1: No real per-tab browser state (tab_id is silently ignored)

### Symptom
Model opens "5 tabs" by calling `browser_navigate` 5 times in a row (one URL each),
then tries to read each tab back with `browser_eval`/`browser_get_text` passing a
distinct `tab_id` (`tab-1` .. `tab-5`) per the tool's documented contract. All 5
calls returned **identical** content — the title/body of whichever URL was loaded
*last* (`Mechanical Watch – Bartosz Ciechanowski`), regardless of which `tab_id`
was requested. The model itself caught this mid-run:

> "Let me read each page properly - it seems tab-1 got mixed up... All tabs seem
> to be showing the same page. Let me close the browser and start fresh with
> sequential navigation."

This is **not** a model misuse problem — the raw transcript shows it called the
tool exactly per spec (`<parameter=tab_id>tab-1</parameter>` etc.) and the backend
just didn't honor it.

### Root cause (confirmed by reading code)
`crates/llama-chat-tools/src/browser_session/backends.rs:128-147`:

```rust
// ─── Tab-aware public API ───────────────────────────────────────────────────

/// Navigate a specific named browser tab. In CDP mode each tab_id is isolated.
/// In Tauri/wry mode tab_id is ignored (single panel).
pub fn navigate_browser_tab(url: &str, _tab_id: &str) -> Result<(), String> {
    // Tauri and wry don't support multi-tab — delegate to single-tab navigate
    notify_tauri_browser_navigate(url)
}

/// Evaluate JS in a specific named browser tab.
/// In Tauri/wry mode tab_id is ignored (single panel).
pub fn eval_in_browser_tab(js: &str, _tab_id: &str) -> Result<String, String> {
    // Tauri and wry don't support multi-tab — delegate to single-panel eval
    eval_in_browser_panel(js)
}

/// No-op: tab isolation is CDP-only; wry/Tauri use a single panel.
pub fn close_browser_tab(_tab_id: &str) -> Result<(), String> {
    Ok(())
}
```

`tab_id` is explicitly prefixed with `_` (intentionally unused) and every "tab"
operation collapses onto the single shared Tauri WebView / wry window. There is
exactly one browser surface in both the Tauri desktop backend and the wry
fallback backend, so "5 parallel tabs" is fiction — it's one window being
navigated 5 times in a row, and only the last navigation's content is ever
readable.

This matches/confirms the previously-tracked tasks:
- #11 Refactor browser session_state/backends for real per-tab state
- #12 Implement multi-window support in wry_browser.rs
- #13 Implement multi-webview support in Tauri browser-panel path
- #14 Build and smoke-test both fixes

### Status
Root-caused. Fix not yet designed/implemented — see "Investigation notes" below.

---

## Bug #2: Apparent deadlock after `browser_close` tool call

### Symptom
After self-diagnosing Bug #1, the model called `browser_close` to recover. Its
`<tool_response>` came back **empty**, and generation froze immediately after:
- Token counter stuck at exactly `~24.2K/173.6K` across multiple checks spanning
  10+ minutes (verified via `evaluate_script` reading the page's token-count
  button — byte-identical text every time).
- `GET /api/model/status` kept reporting `"generating": true` the whole time.
- Worker process CPU time was flat: `988.21875s -> 988.265625s` over 5 real
  wall-clock seconds (checked via `Get-Process`). A model actively generating
  or even just "thinking" at ~95 tok/s would burn close to 100% of a core: this
  proves the worker was **blocked/idle**, not slow-computing.

### What's ruled out
`notify_tauri_browser_close()` (`crates/llama-chat-tools/src/browser_session/backends.rs:115`)
itself cannot be the hang — it has bounded timeouts:
```rust
pub fn notify_tauri_browser_close() -> Result<(), String> {
    let _ = ureq::post(&format!("{TAURI_UI_BRIDGE_BASE}/bridge/browser/close"))
        .timeout(std::time::Duration::from_secs(3))
        .call();
    #[cfg(feature = "wry-browser")]
    { let _ = crate::wry_browser::close(); }
    Ok(())
}
```
3s HTTP timeout, errors swallowed with `let _ =`, always returns `Ok(())`. So the
`close` branch in `browser_tools.rs::handle_browser_tool` (`"close" => { ... }`)
should return near-instantly. The hang must be **downstream** of this function
returning — somewhere in how the `NativeToolResult` gets serialized, sent back
over the worker's stdin/stdout JSON-lines IPC, and re-injected into the
inference loop's context before generation can resume.

### Hypotheses to check (not yet confirmed)
1. The wry window's native `close()` call (Windows: likely destroys a WebView2
   window) might block the calling thread if it's called from the wrong thread
   (wry/WebView2 generally requires window operations to happen on the thread
   that created the window — if `close()` is called from a worker/tokio thread
   instead of the window's owning thread, this could deadlock on a thread-affinity
   violation rather than time out cleanly).
2. IPC pipe deadlock: project memory notes `stdin(Stdio::null())` is required on
   **all** child processes to prevent IPC pipe inheritance hangs — worth checking
   whether `browser_close` (or whatever it shells out to, if anything) spawns a
   child process without this flag.
3. A mutex/lock held by a panicked or stuck thread (e.g. the session lock from
   `current_session()`) never released, so the *next* tool call after `close`
   blocks forever trying to acquire it. Need to check if the model's *next*
   action (which never appeared) was queued and is itself blocked acquiring a
   session lock that `close()` poisoned or never released.
4. Something in the worker's main loop awaiting a tokio task that never
   completes (e.g. `tauri::async_runtime::spawn` work for the close bridge call
   racing with the wry close on the main thread in the desktop build).

### Status
Confirmed as a real hang (not just slow), root cause **not yet found** — needs
live debugging next session (attach to worker process, add tracing around the
close path, or reproduce with verbose `eprintln!` already present in
`backends.rs`/`browser_tools.rs` and tail the worker's stderr/log).

User has since **unloaded the model**, so the live repro is gone for now.

---

## Investigation notes (fill in as work progresses)

### Session 2 findings (2026-06-16, continued after model unload)

#### Bug #1 — confirmed scope, fix design

Read the full backend stack to confirm exactly how "single panel" state is
shared. The architecture is even more global than first thought:

- `crates/llama-chat-tools/src/browser_session/session_state.rs` holds THREE
  process-wide `static Mutex<...>` globals: `ACTIVE_URL`, `CACHED_HTML`,
  `CACHED_TEXT`. `current_session()` / `open_session()` read/write these
  directly — there is no per-tab keying anywhere in the data model, not just
  in the navigate/eval/close API surface noted before.
- `TauriHttpSession` (the concrete `BrowserSession` impl actually used in
  desktop/Tauri mode) is a thin wrapper: `navigate()` calls
  `notify_tauri_browser_navigate()` (drives the ONE shared `browser-panel`
  WebView via the 18091 HTTP bridge) then immediately does its own
  `do_fetch()` (re-reads `document.documentElement.outerHTML` from that same
  shared panel) and overwrites the global `CACHED_HTML`/`CACHED_TEXT`/
  `ACTIVE_URL`. So "navigate tab N" === "clobber the only session everyone
  reads from."
- Confirmed via `is_tauri_available()` / the 18091 bridge server
  (`src/mcp_ui/mod.rs`, `src/mcp_ui/browser_tools.rs`) that when running the
  desktop Tauri app (which is how this bug was reproduced), the Tauri path is
  used, not the wry fallback — `wry-browser` is compiled in
  (`default = ["cdp", "wry-browser"]` in `crates/llama-chat-tools/Cargo.toml`)
  but its singleton `WRY` static is never populated unless `is_tauri_available()`
  returns false, so it was very likely entirely inert this session.

**Fix design (maps to existing tasks #11/#13, not yet implemented):**
1. Replace the 3 global statics in `session_state.rs` with a
   `HashMap<String /* tab_id */, TabState { url, cached_html, cached_text }>`
   keyed by the model-supplied `tab_id`. `current_session(tab_id)` /
   `open_session(url, tab_id)` take the id explicitly.
2. Give each `tab_id` its own Tauri child WebView label (e.g.
   `browser-panel-{tab_id}`) instead of the hardcoded `"browser-panel"` label
   in `src/mcp_ui/browser_tools.rs::bridge_browser_navigate` — Tauri already
   supports multiple child webviews per window (`window.add_child` is called
   per-label), so this is additive, not a rearchitecture.
3. `eval_in_browser_panel`/`notify_tauri_browser_navigate`/
   `notify_tauri_browser_close` in `backends.rs` need a `tab_id` parameter
   threaded through to pick the right webview label/bridge target instead of
   the hardcoded `"browser-panel"` target string.
4. wry path (task #12, lower priority since it's likely inert in desktop
   testing): would need an analogous `HashMap<tab_id, WryHandle>` instead of
   the single `static WRY`. Only worth doing if web-mode (non-Tauri) browser
   testing is actually exercised — confirm with user before investing here.
5. `close_browser_tab(tab_id)` stops being a no-op: removes that tab's map
   entry and closes/destroys that tab's actual webview.

This is a real but bounded refactor — no unknowns left, just needs
implementation + the existing task #14 (build/smoke-test).

#### Bug #2 — ruled out the entire browser/tool-dispatch code path

Traced the full call chain from tool dispatch through to decode-resume
(used a sub-agent to read `llama-chat-engine` since it's outside the crate
I'd been focused on). Chain:

```
command_executor::single_exec::execute_single_call
  → tool_dispatch::run_native_tool_with_timeout   (tool_dispatch.rs:120-183)
      spawns a dedicated std::thread, calls dispatch_native_tool() on it,
      waits on the calling side with rx.recv_timeout(BROWSER_TOOL_TIMEOUT_SECS = 90s)
      ⇒ HARD BOUND: this cannot hang past 90s, model gets a timeout-error
        string back if it does.
  → llama_chat_tools::dispatch::dispatch_native_tool
  → browser_tools::handle_browser_tool("close", ...)
      → notify_tauri_browser_close()  (3s ureq timeout, errors swallowed)
      → TauriHttpSession::close() → notify_tauri_browser_close() AGAIN
        (redundant second call, same 3s bound)
      → wry_browser::close() (only matters if wry is active — see Bug #1
        notes, likely inert this session; non-blocking send_event either way)
  → back in single_exec.rs: "browser_close" is explicitly in the
    `skip_check` list (single_exec.rs:162-164) — the sub-agent
    quick-result-check decode pass is SKIPPED for this tool, so that's not
    it either.
  → command_executor::inject::inject_output_tokens — synchronous
    `context.decode()` loop, no locks/channels in this function itself.
```

**Conclusion: every component actually inside the browser-tool-call path is
provably bounded (≤ ~96s worst case: 90s timeout + 2×3s ureq calls), and the
observed hang was 10+ minutes with the worker at near-zero CPU.** This rules
out `crates/llama-chat-tools/src/browser_tools.rs`,
`browser_session/{backends,tauri_session,session_state}.rs`, and
`tool_dispatch.rs`'s timeout wrapper as the cause. If the close call itself
had hung, the model would have received a `"Error: Tool execution timed
out..."` string at the 90s mark and resumed — it did not.

Also ruled out **context/KV-cache exhaustion** as a contributing factor:
the live token counter read `~24.2K/173.6K` at the time of the freeze — only
~14% of context used, nowhere near the custom TurboQuant KV cache's
capacity limits (see `memory/turboquant-kv-cache.md`), so this is not a
near-full-context edge case in the custom cache code.

**Where the hang must actually be** (none confirmed, ranked by plausibility):
1. **`context.decode()` itself (FFI into llama.cpp/CUDA), called from
   `inject_output_tokens` to resume generation after the tool result is
   injected.** This is the only unbounded, un-timed-out call left in the
   path once tool dispatch returns. A near-zero-CPU hang here is consistent
   with a blocked GPU driver call (e.g. CUDA stream synchronize stuck) rather
   than a busy spin. Why it would correlate with *this specific* tool call is
   unknown — possibly coincidental (whatever batch/eval happened to run next
   hit an unrelated edge case) rather than caused by the close tool's content.
2. **Something in the worker's stdin/stdout IPC reader/writer
   (`crates/llama-chat-worker/src/worker/io_tasks.rs`)** — read this file
   this session: `stdout_reader_task` looks sound (dedicated blocking thread
   for `BufReader::lines()`, bounded `TokioMutex` locks, no obvious unbounded
   wait). Lower priority than #1 but not fully eliminated — haven't traced
   the PARENT-side consumer of the SSE/token stream (`commands::chat.rs`,
   `crates/llama-chat-web/src/worker_pool.rs`) in this session.
3. **wry event-loop thread wedged** (only relevant if wry was actually
   active, which seems unlikely this session per Bug #1 findings above) —
   deprioritized.

**Why static analysis can't go further:** there were no log/stderr captures
from the actual hung session to grep (checked `logs/*.log` and
`logs/backend_std{out,err}.log` — all stale, from February, not the
2026-06-16 session). The worker's stderr is `Stdio::inherit()`'d to the
parent (`process_manager.rs:130`), so next time this is reproduced, the
parent process's own console/terminal needs to be kept open and watched
live, or redirected to a file, to capture the `eprintln!` trail already
present in `backends.rs`/`browser_tools.rs`/`io_tasks.rs`.

**Recommended fix for this session (since root cause needs a live repro to
confirm):**
1. Add `eprintln!` tracing immediately before/after the `context.decode()`
   call in `inject_output_tokens` (`command_executor/inject.rs`) — if it logs
   "before" but never "after" on the next repro, that's the smoking gun
   confirmed.
2. Add a coarse watchdog: if no token/status update is sent for >120s during
   an active generation, surface a "generation appears stuck" notice to the
   conversation (mirrors the existing crash-recovery persisted-notice
   pattern in `io_tasks.rs::persist_crash_notice`) instead of hanging the UI
   forever with no signal. This bounds the *impact* even if the root cause
   stays elusive for now.
3. Capture the worker's stderr to a rotating log file (not just
   `Stdio::inherit()`) so a future hang's `eprintln!` trail survives even if
   nobody was watching the console live when it happened.

### Status
- Bug #1: **IMPLEMENTED AND COMPILING** (2026-06-16, session 3).
  5 files changed, 2 binaries cargo-checked clean (llama_chat_web, llama_chat_app):
  - `crates/llama-chat-tools/src/browser_session/session_state.rs` — replaced 3 global
    statics with a `Mutex<Option<HashMap<String,TabState>>>`, new helpers:
    `store_tab_state(tab_id,url,html,text)`, `remove_session(tab_id)`, all
    existing functions now take `tab_id: &str`.
  - `crates/llama-chat-tools/src/browser_session/backends.rs` — added
    `DEFAULT_TAB_ID="main"` const, `tab_label(tab_id)->String` (maps "main"→
    "browser-panel", else "browser-panel-{safe}"), rewrote `navigate_browser_tab`/
    `eval_in_browser_tab`/`close_browser_tab` to use per-tab webview target;
    old 0-arg functions are now thin wrappers calling the new tab-aware functions.
  - `crates/llama-chat-tools/src/browser_session/tauri_session.rs` — added
    `tab_id: String` field, `open(tab_id,url)` takes tab_id, every
    `eval_in_browser_panel`/`notify_tauri_browser_*` call replaced with tab-aware
    variant; `close()` now calls `close_browser_tab`+`remove_session`.
  - `crates/llama-chat-tools/src/browser_tools.rs` — extracts `tab_id` from tool
    `args` (defaults to `DEFAULT_TAB_ID="main"`), threads through all dispatch
    branches (`navigate`, `click`, `go_back`, `screenshot`, `close`, etc.).
  - `src/mcp_ui/browser_tools.rs` — `bridge_browser_navigate` reads `target` field
    from POST body (defaults "browser-panel"), uses it as webview label for both
    lookup and creation; `bridge_browser_close` now takes a body, reads `target`,
    hides panel for "browser-panel" (legacy UX) and destroys the webview for other
    per-tab labels.
  Needs smoke-test (task #14): load 35B model, run "5 HN tabs" prompt, verify
  each tab's text is distinct.

- Bug #2: root cause NOT confirmed; instrumentation **added to DB** (2026-06-16).
  `crates/llama-chat-engine/src/command_executor/inject.rs` now calls
  `log_event(conv_id, "inject_start"|"decode_token"|"inject_done"|"inject_error",…)`
  at injection boundaries and every 50 tokens (+ first/last token).
  Since `log_event` is synchronous (std::mpsc channel send to a background writer
  thread), this is zero-cost on the hot path. Events are stored in the `logs` DB
  table under `level="event:inject_*"`. On the next hang repro, the per-conversation
  log will show the last `decode_token` that committed — that's the exact token
  inside which `context.decode()` hung.
  Still needed: a UI-level "generation stuck >120s" notice (task #15 remains open).

---

## Pipeline comparison backlog (opencode-inspired)

Derived from a side-by-side comparison of llama_cpp_rs_chat vs opencode pipelines.

- [x] Typed error persistence — persist errors as final assistant message row in DB so they survive reload *(done)*
- [x] Automatic retry for remote providers — wrap ureq SSE in `openai_compat/generate/mod.rs` with 3-attempt exponential backoff, retry on 429/5xx only *(done)*
- [ ] Doom-loop confirmation prompt — if last 3 tool calls are identical (same name + args), pause and send a WS status message asking user to confirm before continuing
- [ ] Reasoning token separation — detect `<think>…</think>` at engine level, emit with a `reasoning` flag on `TokenData`, front-end renders in a collapsible block
- [ ] Message parts table — add `message_parts` table (or JSON parts column) persisting `{ type, content, tool_name?, tool_args?, tool_result? }` per turn; enables re-renderable UI and analytics
- [ ] Cost tracking — add `total_cost_usd REAL` to `conversations` table; accumulate on each remote provider response using known per-token pricing
