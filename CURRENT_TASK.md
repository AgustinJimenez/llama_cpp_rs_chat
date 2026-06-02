# Current Task

## Tauri v2 Plugin Integration

### High Priority — All Done ✅

| Plugin | Notes |
|---|---|
| `tauri-plugin-updater` | Initialized in `main.rs` |
| `tauri-plugin-single-instance` | Focuses window + forwards deep links |
| `tauri-plugin-window-state` | POSITION/SIZE/MAXIMIZED/VISIBLE flags |
| `tauri-plugin-notification` | Initialized (not yet wired to inference events) |
| `tauri-plugin-global-shortcut` | `CmdOrCtrl+Shift+Space` → show/focus window |
| keepawake crate | Prevents OS sleep during inference |
| System Monitor fix | CPU/RAM/VRAM all fixed — stdin null + real nvidia-smi memory.used |

### Medium Priority — Pending

| Plugin | What it adds |
|---|---|
| `tauri-plugin-notification` wiring | Notify user when inference / agent run completes |
| `tauri-plugin-clipboard` (extended) | Read images from clipboard → vision pipeline |
| `tauri-plugin-keyring` | Store API keys in OS Credential Manager |
| `tauri-plugin-pty` | Real PTY/terminal instead of `execute_command` |
| `tauri-plugin-store` | Lightweight key-value settings complement to SQLite |

---

## Other Queued UI / Backend Work

From memory backlog:

| Item | Notes |
|---|---|
| **Agent system_prompt in engine** | Wire `load_effective_config()` so generation reads assigned agent's system_prompt per conversation (Step 4 of agent system) |
| **UI: scroll on edit** | After editing a user message and submitting, scroll chat to bottom |
| **UI: context breakdown** | Split "Active messages" into: messages / raw tool output / summary tool output |
| **UI: sidebar flicker** | Sidebar shows loading label over conversation list on new conversation — fix with overlay spinner |
| **UI: empty thinking tags** | Qwen3 emits `<think></think>` between tool calls — skip rendering empty/whitespace-only thinking blocks |
| **Orphan server processes** | Remote providers run servers with `background: false`, processes get orphaned after timeout |
| **Unify stats components** | Local vs remote providers show stats differently |
