AGENTS GUIDE

This is the short canonical reference for agents (Claude Code, OpenAI Agents, etc.) working in this repo.

Default development:
- Web app: "npm run dev:auto" (automatic GPU detection) or "npm run dev" (CPU-only)
- Desktop app: "npm run dev:auto:desktop" (automatic GPU detection) or "npm run tauri:dev" (CPU-only)

Web app runs Vite on port 4000 with Rust backend on port 8000. Access via http://localhost:4000. Desktop app opens native window.

GPU acceleration: "npm run dev:auto" (web) or "npm run dev:auto:desktop" automatically detect the best setup (Metal on macOS, CUDA on Windows, CPU fallback). Manual options: "npm run dev:metal"/"npm run tauri:dev:metal", "npm run dev:cuda"/"npm run tauri:dev:cuda".

Frontend alternatives: "npm run build" for production build. If the backend is already running, start Vite only with "npx vite --host --port 4000".

Backend alternatives: "cargo run --bin llama_chat_web" if Vite is already running. Rust builds: "cargo build --lib" and "cargo build --bin llama_chat_web".

Desktop app: "cargo tauri dev" for hot reload, "cargo tauri build" for production desktop.

CMake: required by llama-cpp-sys-2. All npm scripts route through tools/ensure-cmake, a standalone Rust tool that auto-downloads a portable CMake if it's not on PATH. This solves the chicken-and-egg problem where llama-cpp-sys-2's build script needs cmake before our build.rs runs. The ensure-cmake tool checks: (1) "cmake" on PATH, (2) well-known install locations (C:\Program Files\CMake, /usr/local/bin, /opt/homebrew/bin), (3) cached download in target/cmake/, (4) fresh download from GitHub releases. When cmake is found at an absolute path not on PATH, it injects the directory into the child process's PATH.

Docker:
- CPU: "docker build -f Dockerfile.test-cmake -t llama-cpu ." then "docker run -p 8000:8000 -v /path/to/models:/app/models -v ./assets:/app/assets llama-cpu"
- CUDA: "docker build -f Dockerfile.cuda -t llama-cuda ." then "docker run --gpus all -p 8000:8000 -v /path/to/models:/app/models -v ./assets:/app/assets llama-cuda"
- Mount models to /app/models (the browse endpoint only allows /app/models and /app paths)
- CUDA requires NVIDIA Container Toolkit on the host

Testing: "npm test" (Playwright E2E; backend must be running on 8000). UI/headed/debug variants: "npm run test:ui", "npm run test:headed", "npm run test:debug", "npm run test:docker". Unit tests: "cargo test". Single browser: "npx playwright test --project=chromium".

Mock mode for tests: build or run with the "mock" feature. Example: "cargo build --features mock --bin llama_chat_web" or "TEST_MODE=true cargo run --bin llama_chat_web".

Tool calling: tool schema is exposed via /api/tools/available and execution via /api/tools/execute. Models see available tools injected into prompts. Safety limit MAX_TOOL_ITERATIONS = 5 on the frontend agent loop.

Browser automation: use the Chrome DevTools MCP (chrome-devtools-mcp) for browser testing, NOT the Claude Chrome extension. Install with: "claude mcp add chrome-devtools --scope user npx chrome-devtools-mcp@latest". Use this to interact with the UI at http://localhost:4000 for testing models, chat, and features.

Common gotchas: use port 4000 for the UI (not 8000), keep backend running for Playwright tests, use Chrome DevTools MCP for browser automation (not Claude Chrome extension), must kill running llama_chat_web.exe before rebuilding on Windows (Access denied), prefer existing modules rather than duplicating code when editing web routes or chat logic.

Architecture:

Backend (src/web/): chat/ is the inference pipeline (generation.rs token loop with conditional KV cache GPU offload, templates.rs prompt formatting, command_executor.rs tool execution, tool_tags.rs per-model tag config, stop_conditions.rs). routes/ has HTTP/WebSocket handlers (chat, config, model, conversations, tools, files, health, logs, system, frontend_logs, static_files). database/ is SQLite persistence (conversations, messages, config, migration). models.rs defines shared types (SamplerConfig, SharedLlamaState, ChatRequest/Response). config.rs loads assets/config.json and resolves system prompts. model_manager.rs handles model loading/unloading with VRAM-based GPU layer calculation. websocket.rs handles WebSocket streaming. gguf_utils.rs extracts model metadata for auto-configuration. vram_calculator.rs computes optimal GPU layer count.

Chat pipeline: WebSocket message -> load config + model -> resolve system prompt (agentic/custom/model-default) -> format with chat template (ChatML/Mistral/Llama3/Gemma) using model-specific tool tags -> tokenize -> create context with KV cache on GPU if gpu_layers > 0 -> generate tokens in loop -> check stop conditions -> detect and execute commands (regex on tool tags) -> inject output back into context -> stream tokens to frontend -> log to SQLite.

Key types: SharedLlamaState (Arc<Mutex<Option<LlamaModelState>>>) wraps the loaded model. ConversationLogger writes to SQLite per-conversation. ToolTags (exec_open/close, output_open/close) are per-model (Qwen uses <tool_call>, Mistral uses [TOOL_CALLS], default uses SYSTEM.EXEC). SamplerConfig holds all inference params plus model_path and model_history.

Frontend (src/): Atomic design — atoms/ (Button, Dialog, etc.), molecules/ (MessageInput, ToolCallBlock, CommandExecBlock), organisms/ (ModelSelector, SettingsModal, Sidebar), templates/ (ChatInputArea, MessagesArea). Key hooks: useChat (messaging orchestration), useModel (model lifecycle), useToolExecution (tool call parsing with MAX_TOOL_ITERATIONS=20), useSettings (sampler config). Utils: chatTransport (HTTP/WS abstraction), toolParser (multi-format tool call extraction).

Model auto-configuration: When loading a GGUF model, gguf_utils.rs extracts general.sampling.* keys for optimal parameters. Fallback presets in src/config/modelPresets.ts keyed by general.name. Priority: GGUF embedded params -> preset lookup -> defaults.

Out-of-process worker: The model runs in a child process (`llama_chat_web.exe --worker`) spawned by the web server. Communication is JSON Lines over stdin/stdout pipes. Key files: `src/web/worker/` — `ipc_types.rs` (protocol types), `worker_main.rs` (child entry point, 3-thread design), `worker_bridge.rs` (server-side `WorkerBridge` abstraction replacing `SharedLlamaState + GenerationQueue`), `process_manager.rs` (spawn/kill/restart). `SharedWorkerBridge = Arc<WorkerBridge>` is passed to all route handlers. Force-unload (`POST /api/model/hard-unload`) kills the child process so the OS reclaims ALL VRAM/RAM, then auto-restarts a fresh worker. The worker's `run_generation` uses a single-threaded tokio runtime — token forwarding uses a real OS thread (not tokio::spawn) because the generation loop has no yield points.

Tools (tools/ directory): Standalone Rust utilities, each with their own Cargo.toml. "tools/ensure-cmake" auto-downloads CMake if missing (also a library used by start-dev). "tools/start-dev" kills old processes and launches backend + Vite. GPU backend via `--gpu cuda|vulkan|cpu` (default cpu). Quick commands: `npm run start:cuda`, `npm run start:vulkan`, `npm run start:cpu`, or `npm start` (run existing binary). "tools/print-metadata" inspects GGUF model files (`npm run inspect-gguf <path-to-gguf>`).

Linting: "cargo clippy --bin llama_chat_web --features cuda" for Rust. "npm run lint" for frontend ESLint. Both should report zero warnings on clean code.

TTS Notifications (KittenTTS): The user has a KittenTTS installation at /Users/agusj/repo/KittenTTS/ with a Claude Code hook that speaks notifications aloud. Setup: (1) Ensure the venv exists: "cd /Users/agusj/repo/KittenTTS && python3 -m venv .venv && .venv/bin/pip install -e . soundfile". (2) The hook script is /Users/agusj/repo/KittenTTS/claude_hook.sh — it reads JSON from stdin, extracts the message field, and passes it to play.py. (3) The hook is registered in ~/.claude/settings.json under hooks.Notification. To test: echo '{"message":"Hello"}' | /Users/agusj/repo/KittenTTS/claude_hook.sh. Voices: Bella, Jasper, Luna, Bruno, Rosie, Hugo, Kiki, Leo (default: Kiki).
