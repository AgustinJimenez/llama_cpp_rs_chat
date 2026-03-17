# Possible Future Implementations

## Computer Use Enhancements
- **Window listing tool** — list open windows with titles, positions, sizes so the LLM knows what's on screen without screenshotting
- **Clipboard read/write** — let the LLM copy/paste programmatically
- **Mouse drag** — click-and-drag for resizing windows, selecting text, etc.
- **OCR tool** — extract text from screenshots without needing a vision model (Windows OCR API)

## Chat & UX
- **Voice input** — speech-to-text microphone button (TTS already exists on the hooks side)
- **Conversation branching** — DONE: edit message + regenerate button implemented
- **Long-term memory** — persistent notes the LLM stores across conversations (like "user prefers Python over JS")
- **Multi-conversation** — run parallel chats with the same or different models
- **Conversation export** — export as Markdown/JSON/plain text for sharing
- **Conversation search** — full-text search across all chats (SQLite FTS5)
- **Dark/Light theme toggle** — currently dark only

## Model & Inference
- **Structured output / JSON mode** — DONE: GBNF lazy grammar for tool call JSON constraints
- **Speculative decoding** — use a small draft model (0.5B-1B) to speed up generation 2-3x. llama.cpp supports it natively. Requires user to select a draft model in settings.
- **LoRA hot-swap** — load/unload LoRA adapters without reloading the base model

## Tools & Integration
- **Code interpreter** — sandboxed Python/JS execution with captured output + plots (like Jupyter)
- **RAG / document ingestion** — chunk PDFs/docs, embed locally, retrieve relevant context
- **Image generation** — integrate Stable Diffusion or FLUX locally

---

## Database Restructuring (v0.3.0)

### Problem
Currently, system prompt, tool definitions, and messages are all mixed together when building the prompt. This makes it impossible to accurately calculate how much context is available for conversation vs overhead. It also blocks mid-task compaction and proper token budgeting.

### Current Structure
```
conversations
  - id, created_at, title, system_prompt

messages
  - id, conversation_id (FK), role, content, timestamp, sequence_order
  - compacted (bool), timing columns

conversation_config
  - per-conversation settings override
```

### Proposed Structure
```
conversations
  - id, created_at, title

conversation_context (NEW - 1:1 with conversation)
  - conversation_id (FK)
  - system_prompt_text
  - system_prompt_tokens (cached count)
  - tool_definitions_json
  - tool_definitions_tokens (cached count)
  - last_updated_at

messages (existing, enhanced)
  - id, conversation_id (FK), role, content, timestamp, sequence_order
  - compacted: bool
  - compaction_group_id: nullable FK to compaction_groups
  - token_count: cached token count per message

compaction_groups (NEW)
  - id, conversation_id (FK)
  - summary_text
  - covers_from_sequence, covers_to_sequence
  - token_count_before (total tokens of compacted messages)
  - token_count_after (tokens of summary)
  - trigger: "threshold" | "mid_task" | "user_requested"
  - created_at
```

### Benefits
- Accurate token budgeting: `available = context_size - system_tokens - tool_tokens`
- System prompt changes don't require re-tokenizing everything
- Tool definitions cached and reused across turns
- Compaction groups enable UI to show expandable sections per group
- Per-message token counts enable precise context management

---

## Mid-Task Incremental Compaction

### Problem
When the model is in a tool-calling loop (e.g., installing packages, debugging errors), each failed attempt generates hundreds of tokens. The model re-reads all previous failed attempts on every turn, wasting context.

### Solution
After each tool result, detect retry patterns and offer incremental compaction:

1. Track consecutive tool calls to the same tool with similar arguments
2. After N failed attempts (e.g., 3), ask the model: "Should we summarize the previous attempts?"
3. If yes, create a `compaction_group` covering those messages
4. Replace them in the prompt with a mini-summary: "Tried 3 approaches to install X, all failed due to Y"
5. Original messages preserved in DB for user viewing

### Detection Heuristics
- Same tool called 3+ times in a row
- Tool output contains error/failure patterns
- Accumulated tool output exceeds N tokens
- Model explicitly requests compaction via a special tool

### UI
- Collapsible block: "3 failed attempts summarized" with expand arrow
- Shows summary text when collapsed
- Full original messages when expanded
- Visual indicator distinguishing mid-task compaction from threshold compaction

---

## Context Compaction (DONE - basic version)

### Current Implementation
- `compacted` column on messages table
- Auto-triggers at 70% of available context (context_size - 1200 overhead)
- Keeps last 6 messages, summarizes the rest via LLM sub-agent (4K context, 0.3 temp)
- Summary stored as system message in DB
- UI shows collapsible "[Conversation summary]" divider with archive icon
- Original messages preserved (not deleted)
- Multiple compaction cycles supported (chained summaries)

### Known Limitations
- Uses char/4 token estimate instead of actual token counts
- 1200 token overhead is a fixed estimate (should come from conversation_context table)
- Summary is a system message, not a separate compaction_groups entry
- Model sometimes doesn't recognize the summary as past conversation context

### Future Improvements
- Use actual token counts from per-message `token_count` column
- Separate `compaction_groups` table for proper tracking
- Integrate with mid-task compaction
- Improve summary prompt so model recognizes it as prior conversation

---

## Desktop App Polish

### Done
- System tray (minimize to tray, right-click menu)
- App menu (File/Edit with keyboard shortcuts)
- Window state persistence (position, size, maximized — no fullscreen)
- CSP security policy
- Auto-updater plugin (tauri-plugin-updater) — needs signing keys
- Deep links (llamachat:// protocol)
- Hidden subprocess console windows (CREATE_NO_WINDOW)
- Model loading progress bar
- Mmproj vision auto-detection
- App data directory (%APPDATA%/com.llamachat.desktop/)

### TODO
- GitHub Actions CI/CD for auto-building installers on release
- Code signing for Windows and macOS
- Version bump (currently 0.1.0)
- Splash screen during startup
- Drag-drop .gguf files to load model

---

## Backend Pipeline (DONE items)

- **GBNF lazy grammar** — constrains JSON tool calls when model outputs `{"name"` trigger
- **Parallel tool execution** — native tools run concurrently via `std::thread::scope()`
- **Model loading progress** — `with_progress_callback` wired to llama-cpp-rs fork
- **Cancel generation** — kills stuck subprocesses, waits for thread cleanup on new message
- **`generating` status field** — frontend can track actual backend work state
- **Friendly error toasts** — maps raw backend errors to user-friendly messages
- **Regenerate button** — on last assistant message, truncates + re-sends
