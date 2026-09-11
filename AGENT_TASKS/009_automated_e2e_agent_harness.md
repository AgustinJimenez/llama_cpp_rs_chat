# 009 — Automated end-to-end agent harness

Status: INVESTIGATION — planning started 2026-09-11, nothing designed or implemented yet
Source: comparative review of `E:\repo\Atomic-Chat` (HEAD `64758ea`)

## Goal

Turn "load an agent and give it a real task, then read the output carefully" into something
that runs unattended and fails loudly, instead of something a human does by hand each time.

## Why this surfaced

Verifying the EOS-probe fix ([[002_malformed_response_detection_and_agent_feedback]])
required a full agentic run — a Laravel Products CRUD task — followed by manual analysis:
reassembling 60 KB of SSE tokens, counting `<think>`/`</think>` pairs, balancing
`<tool_call>` against `<tool_response>`, grepping stderr for `EOS_PROBE` and
`llama_decode: failed to decode`, then linting the 20 generated PHP files.

Every one of those checks is mechanical and every one of them was done by hand. The useful
assertions that fell out of it are exactly the assertions a harness should own:

| Assertion | Why it matters |
|---|---|
| `tool_call` count == `tool_response` count | an orphaned call means a tool silently never ran |
| `</think>` count == `<think>` count + 1 | +1 because the first block is prefilled; a different delta means leaked/dropped tags |
| no `EOS_PROBE … incomplete` in stderr | the exact regression 002 fixed |
| no `failed to decode` / `inconsistent sequence positions` | the latent M-RoPE defect |
| `finish_reason == "stop"` | not `watchdog`, not truncation |
| generated artifacts pass their own linter | proves the agent produced usable output, not plausible text |

## Verified in the reference project

`autoqa/` is a Python harness (`main.py`, `test_runner.py`, `screen_recorder.py`,
`reportportal_handler.py`, `checklist.md`, `tests/`). Its README claims:

- starts/stops the app under test automatically, plus a background "computer server"
- **screen recording** of each run for debugging
- **turn monitoring** — *"Prevents infinite loops with configurable turn limits"*
- test discovery by scanning a directory
- optional ReportPortal upload, cross-platform

The turn-limit idea is the one that transfers most directly: an agent harness whose failure
mode is "runs forever" is worse than no harness.

## What does NOT transfer

Their harness drives the **GUI** (screen recording, Windows Sandbox, a computer-control
server). That is heavy, flaky, and aimed at release QA. Ours should start at the **API**
layer — `POST /api/chat/stream` with an `agent_id` — which is where today's verification
already happened, is deterministic, needs no display, and exercises the same backend the
desktop app uses.

GUI-level testing is a separate, later question; we already have Chrome DevTools MCP for
ad-hoc UI checks.

## Not yet verified

- Whether runs are reproducible enough to assert on *content*. Sampling is stochastic
  (`temperature 1.0` after the preset fix), so assertions must be **structural**
  (tag balance, tool-call balance, no error markers, artifact lints) rather than
  "the model said X". Needs a few repeat runs to confirm the structural assertions are
  stable across seeds.
- Cost per run. Today's Laravel task was 14,097 generated tokens / ~5 min wall clock on a
  9B. A suite of these is minutes-to-tens-of-minutes, so it is a pre-release / nightly
  gate, not something to run on every commit.
- Where it lives: a Rust integration test in `crates/tests/`, or a standalone script.
  `crates/tests/` already exists and holds live-inference tests, so it is the obvious home,
  but those need a loaded model and are presumably not run in CI today — worth checking.

## Concrete next steps

- [ ] Re-run the Laravel task 3× on the same agent and diff the **structural** metrics only.
      If they are stable, the assertion set above is viable; if not, narrow it.
- [ ] Check how `crates/tests/` is currently invoked and whether it is wired into any CI.
- [ ] Draft one scenario end-to-end (prompt → assertions → pass/fail) before building any
      runner. One good scenario beats a framework with none.
- [ ] Add a hard turn/token ceiling from the start.

## Related

[[002_malformed_response_detection_and_agent_feedback]] — the structural assertions here
overlap heavily with the defect taxonomy proposed there. If both are built, they should
share one validator rather than growing two.
