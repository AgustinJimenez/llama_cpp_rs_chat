# Current Task

## Tauri v2 Plugin Integration

Research: identified 30 official + 50+ community Tauri v2 plugins. Below is the full
status of high-priority and medium-priority plugins for this project.

---

### High Priority

| Plugin | Status | Notes |
|---|---|---|
| `tauri-plugin-updater` | ✅ Done | Initialized in `main.rs` |
| `tauri-plugin-single-instance` | ✅ Done | Initialized in `main.rs`, focuses window + forwards deep links |
| `tauri-plugin-window-state` | ✅ Done | Initialized with POSITION/SIZE/MAXIMIZED/VISIBLE flags |
| `tauri-plugin-notification` | ✅ Done | Initialized in `main.rs` (not yet wired to inference events) |
| `tauri-plugin-global-shortcut` | ✅ Done | `CmdOrCtrl+Shift+Space` → show/focus main window |
| nosleep / keepawake | ✅ Done | `keepawake` crate in `generate_stream`; guard lives for inference duration |
| `tauri-plugin-system-info` | ⬜ TODO | Fix CPU 0% / RAM 0GB bug in System Monitor panel |

### Medium Priority

| Plugin | Status | Notes |
|---|---|---|
| `tauri-plugin-clipboard` (extended) | ⬜ TODO | Read images from clipboard → vision pipeline |
| `tauri-plugin-keyring` | ⬜ TODO | Store API keys in OS Credential Manager |
| `tauri-plugin-pty` | ⬜ TODO | Real PTY/terminal instead of execute_command |
| `tauri-plugin-store` | ⬜ TODO | Lightweight key-value settings complement to SQLite |

---

## Active Implementation: High Priority Plugins

### 1. `tauri-plugin-global-shortcut`
- Add to `Cargo.toml`
- Register in `main.rs`: default hotkey `CmdOrCtrl+Shift+Space` → show/focus main window
- Make hotkey configurable via app settings (stored in config)

### 2. nosleep / keepawake
- Use `keepawake` Rust crate (cross-platform: Windows SetThreadExecutionState, macOS IOPMAssertion)
- Activate when inference starts (worker receives generate request)
- Release when inference ends or is cancelled

### 3. `tauri-plugin-system-info`
- Investigate if it can replace current `get_system_usage` to fix CPU 0% / RAM 0GB
