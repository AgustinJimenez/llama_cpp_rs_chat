# Desktop TODO

## Next Priorities

1. Maintain a repeatable desktop regression harness.
   - Keep at least one GPU-app smoke path (Blender).
   - Add one native-widget smoke path (for example Notepad or Explorer).
   - Run these after desktop-tool changes that affect input, targeting, OCR, or MCP execution.

2. Improve window identity propagation.
   - Prefer PID/HWND continuity across multi-step workflows instead of title re-matching where possible.

3. Improve OCR efficiency and targeting.
   - Better region cropping.
   - Better cache/reuse for repeated OCR on nearby areas.
   - More explicit OCR-vs-UIA routing for GPU-rendered apps.

## Implemented In This Pass

- Added a maintained Blender MCP smoke-test entry point via `cargo run --bin mcp_desktop_smoke -- blender`.
- Desktop tool outputs now include a compact `[desktop_result]` footer.
- Desktop actions now emit JSONL traces for inspection.
