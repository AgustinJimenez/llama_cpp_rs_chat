# 015 — Two processes can bind port 18080 and the loser silently gets no traffic

Status: CONFIRMED 2026-09-11 — not yet fixed
Found: 2026-09-11, while debugging [[012_eos_probe_prompt_leaks_into_visible_message]]

## Symptom

The desktop app and the web server can both be running, both "listening" on 18080, with
**no error from either**. Every `localhost` request goes to one of them, and there is no
indication which.

Observed listeners during the incident:

```
PID 29588  llama_chat_app.exe   local=127.0.0.1   started 11:21:46
PID  4588  llama_chat_web.exe   local=0.0.0.0     started 16:48:06
```

## Mechanism

- Desktop app binds **`127.0.0.1:18080`** — `src/main.rs:172`
- Web server binds **`0.0.0.0:18080`** — `src/server.rs:144`

These are different addresses, so neither `bind()` fails. Windows routes a connection to the
**most specific** matching socket, so `127.0.0.1` traffic always reaches the desktop app and
the web server receives nothing locally while appearing perfectly healthy.

## Why this is worth fixing rather than just knowing about

It cost most of a debugging session. From the moment the desktop app was launched, every API
request during the [[012_eos_probe_prompt_leaks_into_visible_message]] and
[[013_multibyte_utf8_broken_at_token_boundaries]] investigations was answered by a binary
several hours old. That produced a long chain of false conclusions:

- two fixes "verified present in the binary" that "behaved like the old code"
- a retraction that blamed the build/deploy model, which was innocent
- the agent list appearing to change identity (the app uses its own DB)
- `logs/worker.stderr.log` containing only startup lines — the app's worker was doing the
  work and writing elsewhere
- `[EOS_PROBE]` never appearing, which was then used as evidence that the probe never ran

None of that was detectable from inside the app, because from the client's point of view the
API answered normally the whole time.

## Fix direction

1. **Make the collision loud.** Before binding, probe `127.0.0.1:18080`; if something already
   answers `/api/info`, refuse to start with a message naming the other process
   (`running_binary.pid` is now available for exactly this) rather than binding a wildcard
   socket that will never be reached.
2. **Make the port configurable** — `--port` / `LLAMA_CHAT_PORT`. It is currently hardcoded
   in at least `src/server.rs:144`, `src/main.rs:172` and
   `crates/llama-chat-web/src/routes/remote.rs:13`, so running a dev server alongside the
   installed app is impossible today.
3. Consider binding the desktop app to an ephemeral port and telling the webview about it,
   since nothing external needs to reach it.

## Already done

`/api/info` now returns a `running_binary` block (`build_id`, `exe_path`, `exe_size`,
`exe_mtime_secs`, `pid`) read from `current_exe()` by the answering process. That makes the
situation diagnosable in one request, and **asserting it before an experiment is now the
standing rule** — it is what finally exposed this.

## Verification

- Start the desktop app, then the web server; assert the web server **refuses to start** with
  a message identifying the running instance.
- With `--port`/env set, assert both can run and each `/api/info` reports its own `pid`.
