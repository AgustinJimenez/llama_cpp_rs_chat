# Current Task

## Goal

Improve the desktop tools stack further with structured result reporting, persistent action tracing, and better regression support.

## Why

The app is intended to let agents operate a PC. After fixing the major timeout/cancellation issues, the next gaps are:

- tool outcomes are mostly free-form text rather than structured signals
- desktop failures are hard to inspect after the fact
- regression coverage exists, but desktop reliability still depends too much on ad hoc smoke checks

Current state:

- MCP desktop execution is serialized.
- The main Windows UI Automation and OCR hotspots are bounded or cancel-aware.
- Desktop results still lack a unified machine-readable status/tracing layer.

## Plan

1. Add a dispatcher-level structured desktop result summary:
   - status (`completed`, `cancelled`, `timed_out`, `error`)
   - duration
   - image count
   - tool name
2. Append a compact machine-readable footer to desktop tool outputs so agents can parse outcomes consistently.
3. Write persistent desktop action traces as JSONL for post-failure inspection.
4. Add targeted tests for status classification and trace serialization.
5. Run targeted verification:
   - desktop tool dispatch test
   - desktop result/tracing unit tests
   - MCP desktop tool compile path

## Expected Outcome

- Desktop tools produce more consistent machine-readable outcomes.
- Failures and timeouts leave a trace that can be inspected after the fact.
- Future desktop regressions are easier to detect and diagnose.

## Status

Implemented.

Completed in the previous pass:

- per-call desktop cancellation context in the MCP desktop server path
- propagation of cancellation context into desktop tool worker threads
- polling/wait-style cancellation checks
- clearer timeout reporting in the MCP path
- Windows UI Automation client timeouts via IUIAutomation2
- WinRT OCR async-operation cancellation/status handling for Windows OCR paths

Completed in this pass:

- dispatcher-level desktop result status classification
- compact machine-readable `[desktop_result]` footer on desktop tool outputs
- persistent desktop action trace logging as JSONL
- unit tests for desktop result classification and trace-safe argument summarization
- re-ran targeted desktop MCP tests successfully

Remaining limitation from the previous pass:

- A desktop call that is blocked inside a non-cancellable synchronous OS/API call still cannot be forcibly preempted mid-call.

Still not implemented in this pass:

- broader real-app regression harness beyond targeted smoke tests
- OCR regioning/cache improvements
- stronger PID/HWND-forwarding across more multi-step workflows
