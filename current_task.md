# Current Task

## Goal

Make desktop automation safer and more reliable by replacing timeout-only behavior with cooperative cancellation for desktop tool execution.

## Why

The app is intended to let agents operate a PC. In that context, a desktop action that "times out" but keeps running in the background is a correctness issue, not just a performance issue.

Current state:

- MCP desktop execution is serialized, so actions no longer overlap.
- But blocking desktop work can still continue after timeout.
- This can desynchronize agent state from actual machine state.

## Plan

1. Introduce a desktop execution controller for the MCP desktop server.
2. Add per-call cancellation context and timeout handling that signals cancellation explicitly.
3. Refactor thread-based helper execution in desktop tools to support cooperative cancellation instead of detached timeout-only waiting.
4. Update polling/wait-style desktop tools to check cancellation regularly.
5. Return clearer timeout/cancelled errors from the MCP tool path.
6. Run targeted verification:
   - desktop tool dispatch test
   - MCP desktop tool test path
   - real Blender smoke validation if needed

## Expected Outcome

- Timed out desktop tool calls stop making progress as soon as they observe cancellation.
- Later desktop actions can continue from a known state.
- Desktop automation becomes safer for long-running agent workflows.

## Status

Implemented.

Completed:

- Added per-call desktop cancellation context in the MCP desktop server path.
- Propagated cancellation context into desktop tool worker threads.
- Updated polling and wait-style tools to observe cancellation regularly.
- Improved timeout reporting so timeout now requests cancellation explicitly.
- Added Windows UI Automation client timeouts via IUIAutomation2.
- Added WinRT OCR async-operation cancellation/status handling for Windows OCR paths.
- Re-ran targeted desktop MCP tests successfully.

Remaining limitation:

- A desktop call that is blocked inside a non-cancellable synchronous OS/API call still cannot be forcibly preempted mid-call. The major Windows hotspots are now bounded or cancel-aware, but true thread preemption is still not available.
