AGENTS GUIDE

This is the short canonical reference for agents (Claude Code, OpenAI Agents, etc.) working in this repo. Use this to remember key development steps across sessions.

Default development: 
- Web app: "npm run dev:auto" (automatic GPU detection) or "npm run dev" (CPU-only)
- Desktop app: "npm run dev:auto:desktop" (automatic GPU detection) or "npm run tauri:dev" (CPU-only)

Web app runs Vite on port 4000 with Rust backend on port 8000. Access via http://localhost:4000. Desktop app opens native window.

GPU acceleration: "npm run dev:auto" (web) or "npm run dev:auto:desktop" automatically detect the best setup (Metal on macOS, CUDA on Windows, CPU fallback). Manual options: "npm run dev:metal"/"npm run tauri:dev:metal", "npm run dev:cuda"/"npm run tauri:dev:cuda".

Frontend alternatives: "npm run build" for production build. If the backend is already running, start Vite only with "npx vite --host --port 4000".

Backend alternatives: "cargo run --bin llama_chat_web" if Vite is already running. Rust builds: "cargo build --lib" and "cargo build --bin llama_chat_web".

Desktop app: "cargo tauri dev" for hot reload, "cargo tauri build" for production desktop.

Testing: "npm test" (Playwright E2E; backend must be running on 8000). UI/headed/debug variants: "npm run test:ui", "npm run test:headed", "npm run test:debug", "npm run test:docker". Unit tests: "cargo test". Single browser: "npx playwright test --project=chromium".

Mock mode for tests: build or run with the "mock" feature. Example: "cargo build --features mock --bin llama_chat_web" or "TEST_MODE=true cargo run --bin llama_chat_web".

CMake: required for building llama.cpp. If not installed, the build.rs will attempt to download a portable copy to target/cmake/. For Docker/CI builds without cmake, download it first and set CMAKE env var (see Dockerfile.test-cmake). Manual install: "winget install Kitware.CMake" (Windows), "brew install cmake" (macOS), "sudo apt install cmake" (Linux).

Tool calling: tool schema is exposed via /api/tools/available and execution via /api/tools/execute. Models see available tools injected into prompts. Safety limit MAX_TOOL_ITERATIONS = 5 on the frontend agent loop.

Browser automation: use the Chrome DevTools MCP (chrome-devtools-mcp) for browser testing, NOT the Claude Chrome extension. Install with: "claude mcp add chrome-devtools --scope user npx chrome-devtools-mcp@latest". Use this to interact with the UI at http://localhost:4000 for testing models, chat, and features.

Common gotchas to remember: use port 4000 for the UI (not 8000), keep backend running for Playwright tests, use Chrome DevTools MCP for browser automation (not Claude Chrome extension), and prefer existing modules rather than duplicating code when editing web routes or chat logic.

Architecture:

Backend (src/web/): chat/ is the inference pipeline (generation.rs token loop, templates.rs prompt formatting, command_executor.rs tool execution, tool_tags.rs per-model tag config, stop_conditions.rs). routes/ has HTTP/WebSocket handlers (chat, config, model, conversations, tools, files, health, logs). database/ is SQLite persistence (conversations, messages, config). models.rs defines shared types (SamplerConfig, SharedLlamaState, ChatRequest/Response). config.rs loads assets/config.json and resolves system prompts. model_manager.rs handles model loading/unloading. websocket.rs handles WebSocket streaming.

Chat pipeline: WebSocket message → load config + model → resolve system prompt (agentic/custom/model-default) → format with chat template (ChatML/Mistral/Llama3/Gemma) using model-specific tool tags → tokenize → generate tokens in loop → check stop conditions → detect and execute commands (regex on tool tags) → inject output back into context → stream tokens to frontend → log to SQLite.

Key types: SharedLlamaState (Arc<Mutex<Option<LlamaModelState>>>) wraps the loaded model. ConversationLogger writes to SQLite per-conversation. ToolTags (exec_open/close, output_open/close) are per-model (Qwen uses <tool_call>, Mistral uses [TOOL_CALLS], default uses SYSTEM.EXEC). SamplerConfig holds all inference params plus model_path and model_history.

Frontend (src/): Atomic design — atoms/ (Button, Dialog, etc.), molecules/ (MessageInput, ToolCallBlock, CommandExecBlock), organisms/ (ModelSelector, SettingsModal, Sidebar), templates/ (ChatInputArea, MessagesArea). Key hooks: useChat (messaging orchestration), useModel (model lifecycle), useToolExecution (tool call parsing with MAX_TOOL_ITERATIONS=20), useSettings (sampler config). Utils: chatTransport (HTTP/WS abstraction), toolParser (multi-format tool call extraction).

Linting: "cargo clippy --bin llama_chat_web --features cuda" for Rust. "npm run lint" for frontend ESLint. Both should report zero warnings on clean code.
