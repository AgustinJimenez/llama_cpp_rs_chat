# Current Tasks — App Improvements

## Status: 4 of 6 tasks completed

### ~~1. Conversation Search~~ ✅ DONE
Implemented: search input in sidebar, instant client-side title filtering.

### ~~3. Faster Streaming Cancel~~ ✅ DONE  
Fixed: cancel check every token instead of every 4 (4x faster).

### ~~4. Mobile Responsive UI~~ ✅ ALREADY DONE
Already implemented: sidebar overlay, hamburger menu, responsive layout.

### ~~7. File Drag-and-Drop~~ ✅ ALREADY DONE
Already implemented: images, text/code, PDFs/DOCX, drag overlay UI.

---

## Remaining Tasks

### 2. Message Editing & Conversation Forking (Priority: HIGH, Effort: Large)

### 1. Conversation Search (Priority: HIGH, Effort: Small)
**Goal:** Add a search input to the sidebar that filters conversations by title and content.

**Implementation:**
- Add a search input at the top of the sidebar (below "Conversations" header)
- Filter conversations client-side by title match (instant)
- For content search: add a `/api/conversations/search?q=term` endpoint that searches message content in SQLite (`SELECT DISTINCT conversation_id FROM messages WHERE content LIKE '%term%'`)
- Show matching conversations highlighted, with snippet of matching text
- Debounce input (300ms) to avoid excessive API calls

**Files to modify:**
- `src/components/organisms/Sidebar.tsx` — add search input, filter logic
- `src/web/routes/conversation.rs` — add search endpoint
- `src/web/database/conversation.rs` — add search query

---

### 2. Message Editing & Conversation Forking (Priority: HIGH, Effort: Large)
**Goal:** Edit a previous message and regenerate the conversation from that point forward.

**Current state:** Basic edit exists (`onEditMessage` in MessageBubble) but doesn't truncate/regenerate.

**Implementation:**
- When user edits a message, truncate all messages after the edited one
- Delete truncated messages from DB
- Re-send the edited message as the new prompt
- The model regenerates from the edited point
- Optional: keep a "branch history" so users can switch between branches

**Files to modify:**
- `src/hooks/useChat.ts` — add `editAndRegenerate(messageIndex, newContent)` function
- `src/web/routes/conversation.rs` — add `/api/conversations/:id/truncate-after` endpoint
- `src/web/database/conversation.rs` — add `delete_messages_after(conversation_id, sequence_order)`
- `src/components/organisms/MessageBubble.tsx` — update edit handler to call regenerate

**Edge cases:**
- What if the edited message is a tool call response? Skip it.
- What about compacted conversations? Can't edit before compaction point.
- System prompt changes between original and edit? Use current config.

---

### 3. Faster Streaming Cancel (Priority: HIGH, Effort: Investigation)
**Goal:** Cancel generation should stop output within <200ms, not seconds.

**Current state:** Cancel sets a flag, but the model keeps generating until the next token check. The worker process might buffer tokens.

**Investigation needed:**
- Check `src/web/chat/generation.rs` — where is the cancel flag checked in the decode loop?
- Check `src/web/worker/worker_bridge.rs` — how does cancel propagate from HTTP to worker process?
- Check if `llama_decode()` blocks for long periods (large batch size = slow cancel)
- Possible fix: reduce batch size during generation, check cancel flag every N tokens
- Possible fix: use `llama_abort()` C API if available (interrupts decode mid-batch)

**Files to investigate:**
- `src/web/chat/generation.rs` — main generation loop, cancel flag check
- `src/web/worker/worker_bridge.rs` — cancel IPC
- `src/web/worker/worker_main.rs` — worker-side cancel handling
- `src/web/routes/chat.rs` — `/api/chat/cancel` endpoint

---

### 4. Mobile Responsive UI (Priority: MEDIUM, Effort: Medium)
**Goal:** App should be usable on phones/tablets. Sidebar should collapse, input should be full-width.

**Implementation:**
- Sidebar: hide by default on mobile (< 768px), show as overlay when toggled
- Header: hamburger menu to toggle sidebar on mobile
- Message bubbles: full-width on mobile (no max-width constraint)
- Input: full-width, larger touch targets
- Model config modal: stack vertically on mobile instead of grid
- Tool call widgets: collapse by default on mobile (save space)

**Files to modify:**
- `src/App.tsx` — responsive sidebar toggle state
- `src/components/organisms/Sidebar.tsx` — mobile overlay mode
- `src/components/organisms/ChatHeader.tsx` — hamburger menu
- `src/components/organisms/MessageBubble.tsx` — responsive widths
- `src/components/molecules/MessageInput.tsx` — mobile sizing
- `src/index.css` — media queries, mobile-specific styles

**Breakpoints:**
- `< 640px` (sm): phone — sidebar hidden, full-width everything
- `640-1024px` (md): tablet — sidebar overlay, adjusted spacing
- `> 1024px` (lg): desktop — current layout

---

### 7. File Drag-and-Drop for Context (Priority: MEDIUM, Effort: Medium)
**Goal:** Drop any file into the chat and the agent reads it automatically.

**Current state:** Image paste/drop works (base64 → vision pipeline). Need to extend for text files, code, PDFs, etc.

**Implementation:**
- Extend the drop zone in MessageInput to accept all file types
- For text files (.txt, .md, .rs, .py, .js, etc.): read content, inject as user message
- For code files: syntax-detect language, wrap in code block
- For PDFs: use existing `/api/file/extract-text` endpoint
- For images: existing flow (base64 → vision pipeline)
- Show file preview chip in the input area before sending

**Files to modify:**
- `src/components/molecules/MessageInput.tsx` — extend drop handler, file type detection
- `src/hooks/useChat.ts` — handle file content in message
- Already have: `/api/file/extract-text` for PDF/DOCX/XLSX

**Supported formats:**
- Text: .txt, .md, .csv, .json, .xml, .yaml, .toml, .log
- Code: .rs, .py, .js, .ts, .tsx, .html, .css, .java, .c, .cpp, .go, .rb, .php, .sql, .sh
- Documents: .pdf, .docx, .pptx, .xlsx (via extract-text API)
- Images: .png, .jpg, .gif, .webp (existing vision pipeline)

---

### 9. Auto-Update (Priority: LOW, Effort: Large)
**Goal:** App checks for updates and offers to install them. Tauri updater plugin is already configured.

**Prerequisites:**
- GitHub Releases with proper tauri update manifest
- Code signing (optional but recommended for Windows)
- Update server URL in tauri.conf.json

**Implementation:**
- Set up GitHub Actions CI/CD for building + releasing
- Configure `tauri.conf.json` updater section with GitHub endpoint
- Add "Check for Updates" button in Settings
- Show notification when update is available

**Files to modify:**
- `tauri.conf.json` — updater endpoint configuration
- `.github/workflows/release.yml` (new) — CI/CD build pipeline
- `src/components/organisms/AppSettingsModal.tsx` — update check button

---

## Completed This Session

- ✅ Gemma 4 model support (tool calling, presets, llama.cpp update)
- ✅ Screenshots displayed in chat (persisted as files, served via API)
- ✅ Mermaid diagrams + Chart.js charts in chat (with 3-dot menu, expand, export)
- ✅ Image lightbox with download
- ✅ Light/dark theme toggle with semantic tokens (all hardcoded colors fixed)
- ✅ Config sidebar fix (shows actual loaded model values)
- ✅ Polling re-render fix (no more menu/scroll reset every 5s)
- ✅ Web search image results (Brave thumbnails + DDG icons)
- ✅ OCR engine stack: Tesseract (~95%) → ocrs (~70%) → native (WinRT/Vision)
- ✅ ensure-tesseract auto-download tool (npm postinstall)
- ✅ ocrs Rust-native OCR bundled (BeamSearch + 2x upscale)
- ✅ PaddleOCR-VL + GLM-OCR VLM OCR testing (not suitable for UI screenshots)
- ✅ VLM OCR subprocess mode (--vlm-ocr flag)
- ✅ GLM (Zhipu AI) + Kimi (Moonshot) cloud providers added
- ✅ Windows installer built (Tauri NSIS 550MB + MSI 570MB)
- ✅ AGENTS.md: documented 3 tool systems, OCR engines, macOS specifics
- ✅ ocr_screen added to core tools (always in system prompt)
