# Desktop Plan

## Goal

Make desktop tools reliable enough for long-running agent workflows by improving identity continuity, regression coverage, perception efficiency, and post-action confidence.

## Phase 1: Reliability Baseline — DONE

All items implemented.

1. ~~Maintain repeatable desktop regression coverage.~~ Done: Blender + Notepad smoke tests.
2. ~~Improve window identity propagation.~~ Done: PID targeting across list_windows, focus_window, kill_process, and compound tools.
3. ~~Keep structured result/tracing support in place.~~ Done: `[desktop_result]` footers + JSONL traces.

## Phase 2: Perception And Verification — DONE

All items implemented.

1. ~~Improve OCR efficiency and targeting.~~ Done: region/window/pid targeting, OCR result caching for unchanged regions, explicit UIA-vs-OCR routing, confidence scores, language parameter.
2. ~~Add post-action verification options.~~ Done: opt-in screen-change verification, retry logic on input tools, assert_text in action sequences, screenshot diff with highlighted region output.
3. ~~Improve coordinate safety.~~ Done: DPI-aware scroll_screen, snap_to_screen coordinate clamping, monitor boundary validation.

## Phase 3: Deeper Hardening — PARTIAL

1. ~~Broaden HWND/process identity handoff.~~ Done: PID filter on list_windows/focus_window, fullscreen detection, graceful kill with WM_CLOSE.
2. Expand maintained real-app coverage. **TODO**
   - Add at least one browser/native-app path beyond Notepad.
   - Keep smoke scenarios stable enough to run after risky desktop changes.
3. Improve trace detail. **TODO**
   - Attach matched-window metadata, cancellation causes, and screenshot linkage to action traces.

## Round 4: Schema, MCP, Audio, Clipboard, Window Layouts — DONE

- [x] **Schema-implementation sync** — Updated 14 tool schemas in jinja_templates.rs: timeout_ms, steps, retry, snap_to_screen, exclude_types, language, highlight, force, grace_ms, pid, mode, type.
- [x] **MCP image format control** — screenshot_format (jpeg/png), screenshot_quality (1-100), screenshot_max_width (320-3840) in mcp_server.rs.
- [x] **Audio controls** — get_system_volume, set_system_volume, set_system_mute. Windows: PowerShell COM (IAudioEndpointVolume), macOS: osascript, Linux: amixer.
- [x] **Clipboard enhancements** — clear_clipboard (EmptyClipboard/arboard), clipboard_file_paths (CF_HDROP/uri-list), clipboard_html (CF_HTML/xclip).
- [x] **Window layout save/restore** — save_window_layout / restore_window_layout with JSON serialization.

## Round 5: Video, Process Monitoring, Notifications, Dialog Handler — DONE

- [x] **Screen recording** — start_screen_recording / stop_screen_recording via ffmpeg (platform-specific capture flags). capture_gif pure Rust with median-cut quantization + LZW encoder.
- [x] **Process monitoring** — wait_for_process_exit (poll with cancellation), get_process_tree (PowerShell/ps recursive), get_system_metrics (sysinfo crate: CPU/mem/disk).
- [x] **Notification handling** — wait_for_notification (OCR-based detection of notification region), dismiss_all_notifications (Win: notification center, Linux: dunstctl/makoctl).
- [x] **Dialog auto-handler** — dialog_handler_start/stop: background thread with button_map matching, auto-click via UI Automation.
- Skipped: File drag-and-drop (COM/OLE too complex, AppleScript unreliable).

## Round 6: Advanced (future)

- [ ] **Browser/DOM integration** — JavaScript injection via CDP, DOM extraction, page content reading.
- [ ] **Cross-platform accessibility tree** — macOS AXUIElement, Linux AT-SPI2 for full a11y tree support.
- [ ] **IME support** — CJK input method composition for type_text.
- [ ] **Image template fuzzy matching** — rotation/scale-tolerant find_image_on_screen.
- [ ] **Form autocomplete handling** — type partial text, wait for dropdown, select match.
- [ ] **Text-to-speech** — speak_text via SAPI/say/espeak.

## Deferred

- [ ] Add a browser-based smoke test (e.g. Chrome/Edge opening a page, clicking a link, verifying text).
- [ ] Attach richer metadata to JSONL traces: matched window HWND/PID, cancellation cause, screenshot path per action.

## Implemented

- Blender MCP smoke-test: `cargo run --bin mcp_desktop_smoke -- blender`.
- Notepad MCP smoke-test: `cargo run --bin mcp_desktop_smoke -- notepad`.
- `[desktop_result]` machine-readable footers on all desktop tool outputs.
- JSONL desktop action traces for debugging.
- PID-based window targeting across list_windows, focus_window, kill_process, and compound tools.
- Opt-in screen-change verification for input tools (click, type, drag).
- OCR region/window/pid targeting to avoid full-screen scans.
- OCR result caching for unchanged target regions.
- OCR confidence scores and language parameter (macOS Vision, tesseract).
- Explicit UIA-vs-OCR routing in compound tools; macOS/Linux OCR fallbacks for 6 compound tools.
- Verification supports smaller target regions; Notepad smoke exercises OCR before save.
- Serialized MCP desktop execution (one action at a time).
- Cooperative cancellation with per-call cancellation context.
- Windows IUIAutomation2 client timeouts and WinRT OCR cancellation.
- Platform-filtered tool exposure (unsupported tools hidden per-platform).
- Module split: ui_tools.rs → screenshot_tools, ocr_tools, ui_automation_tools, clipboard_tools, image_tools.
- spawn_with_timeout safety helper (23 call sites) + per-tool timeout_ms parameter.
- Script execution timeout (120s) for Blender/Unity/Maya/Godot/UE5.
- Arc screenshot cache with configurable threshold/TTL.
- Smooth mouse drag interpolation (steps parameter).
- Screenshot diff with red-highlighted changed region image.
- Form filling: dropdown/checkbox/radio support via control type detection.
- UI tree filtering: exclude_types, configurable max_depth, truncation warning.
- Action sequence: per-action retry, if_previous conditional, abort_on_failure.
- Coordinate snap-to-nearest for off-screen coords.
- Graceful process kill: WM_CLOSE → grace period → force kill.
- Modal dialog detection on macOS (osascript AXDialog) and Linux (xprop _NET_WM_STATE_MODAL).
- Clipboard image read/write on macOS/Linux via arboard.
- Scroll-to-text mode: OCR loop with configurable max_scrolls.
- Fullscreen window detection (monitor rect comparison).
- Keyboard layout warning for non-US-QWERTY layouts.
- Consistent tool_error() adoption across all 15 submodule files.
- Retry logic on type_text/press_key with configurable retry count.
- Mutex poison recovery on screenshot and overlay caches.
- DPI-aware and snap_to_screen on scroll_screen.
- Schema sync: 14 existing tool schemas updated with missing params.
- MCP image format control: jpeg/png, quality 1-100, max_width 320-3840.
- Audio tools: get_system_volume, set_system_volume, set_system_mute (Win/macOS/Linux).
- Clipboard: clear_clipboard, clipboard_file_paths (CF_HDROP), clipboard_html (CF_HTML).
- Window layout: save_window_layout/restore_window_layout (JSON snapshots).
- Process monitoring: wait_for_process_exit, get_process_tree, get_system_metrics.
- Notification tools: wait_for_notification (OCR-based), dismiss_all_notifications.
- Screen recording: start/stop_screen_recording (ffmpeg), capture_gif (pure Rust GIF encoder).
- Dialog auto-handler: dialog_handler_start/stop (background button auto-click).
- All 18 new tools registered in dispatch + DESKTOP_TOOL_NAMES + schemas (91 total tools).
