# Desktop Plan

## Goal

Make desktop tools reliable enough for long-running agent workflows by improving identity continuity, regression coverage, perception efficiency, and post-action confidence.

## Phase 1: Reliability Baseline

1. Maintain repeatable desktop regression coverage.
   - Keep one GPU-app smoke path (`blender`).
   - Keep one native-widget smoke path (`notepad`).
   - Run these after changes to input, targeting, OCR, or MCP execution.

2. Improve window identity propagation.
   - Prefer PID continuity over title matching once a target window is known.
   - Expand desktop tools to accept `pid` anywhere title-only targeting is still the default.

3. Keep structured result/tracing support in place.
   - Preserve `[desktop_result]` machine-readable footers.
   - Preserve JSONL desktop traces for debugging real automation failures.

## Phase 2: Perception And Verification

1. Improve OCR efficiency and targeting.
   - Better region cropping.
   - Better cache/reuse for repeated OCR on nearby areas.
   - More explicit OCR-vs-UIA routing for GPU-rendered apps.

2. Add post-action verification options.
   - Allow clicks, typing, and drags to optionally verify a visible state change before continuing.
   - Surface verification failures as structured result states instead of plain-text surprises.

3. Improve coordinate safety.
   - Tighten monitor/DPI normalization.
   - Handle partially off-screen windows and unusual monitor layouts more defensibly.

## Phase 3: Deeper Hardening

1. Broaden HWND/process identity handoff across higher-level workflows.
   - Avoid re-matching by title inside compound tools once a process/window has already been established.

2. Expand maintained real-app coverage.
   - Add at least one browser/native-app path beyond Notepad.
   - Keep smoke scenarios stable enough to run after risky desktop changes.

3. Improve trace detail.
   - Attach matched-window metadata, cancellation causes, and screenshot linkage to action traces.

## Implemented

- Added a maintained Blender MCP smoke-test entry point via `cargo run --bin mcp_desktop_smoke -- blender`.
- Added a maintained Notepad MCP smoke-test entry point via `cargo run --bin mcp_desktop_smoke -- notepad`.
- Desktop tool outputs now include a compact `[desktop_result]` footer.
- Desktop actions now emit JSONL traces for inspection.
- Window management tools now accept PID-based targeting across more workflows so agents can carry forward a known window identity.
- Added opt-in screen-change verification for high-risk input tools so agents can ask actions to confirm a visible state change before continuing.
- OCR screen and OCR text-finding now share tighter region/window/pid targeting so perception can avoid full-screen scans when the target area is already known.
- Verification now supports smaller target regions, and the maintained Notepad smoke exercises OCR against a live native app window before save.
- OCR now reuses recent results for unchanged target regions, reducing repeated-read cost in stable windows and regions.
- Compound tools now route more explicitly between UI Automation and OCR fallback so GPU-rendered apps stop hard-failing when a vision-based path is available.
