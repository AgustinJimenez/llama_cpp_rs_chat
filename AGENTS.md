AGENTS GUIDE

This is the short canonical reference for agents (Claude Code, OpenAI Agents, etc.) working in this repo.

What this is: Local LLM chat app (React + Rust/llama.cpp). Core flow: load GGUF model → auto-calculate GPU layers from available VRAM → extract Jinja2 chat template + model metadata (general.name) from GGUF → resolve model-specific tool format and sampler presets → format prompts via Jinja template → generate tokens with configurable sampler chain (11 types) → stream over WebSocket → detect tool call patterns in output (`<tool_call>`, `[TOOL_CALLS]`, `SYSTEM.EXEC`, etc.) → execute native tools (file I/O, shell, web search/fetch, Python) → inject results back into context for multi-turn agentic loops. KV cache reused between conversation turns for speed. Conversations and config persisted in SQLite. Model runs in an out-of-process worker (child process with JSON Lines IPC) — kill worker to reclaim all VRAM instantly. Runs as web app (Vite + Rust backend) or Tauri desktop app.

Default development (prefer Tauri commands — they handle both frontend and backend together):
- Desktop app (preferred): "npm run dev:auto:desktop" (automatic GPU detection) or "npm run tauri:dev" (CPU-only)
- Web app (fallback): "npm run dev:auto" (automatic GPU detection) or "npm run dev:cuda" (manual CUDA)
- CRITICAL: "npm run dev" and "npm run dev:web" are CPU-ONLY — they do NOT pass --features cuda. Model will run at ~5 tok/s instead of ~1000 tok/s. ALWAYS use "npm run dev:cuda" or "npm run dev:auto" for GPU acceleration.

Web app runs Vite on port 4000 with Rust backend on port 8000. Access via http://localhost:4000. Desktop app opens native window. When building/running, prefer Tauri commands over running backend and Vite separately.

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

Tool calling: tool schema is exposed via /api/tools/available and execution via /api/tools/execute. Models see available tools injected into prompts. Safety limit MAX_TOOL_ITERATIONS = 5 on the frontend agent loop. Native tools (src/web/native_tools.rs): web_search (DuckDuckGo Instant Answer API with ureq HTTP fallback), web_fetch (headless Chrome via src/web/browser.rs for JS-rendered pages, falls back to ureq for plain HTTP), read_file, write_file, execute_python, execute_command, list_directory. The Chrome browser is a lazy singleton with 5-minute idle timeout to free memory.
Brave web_search: users can store their Brave API key in the app settings (persisted in SQLite config as plaintext). The backend will also fall back to `BRAVE_SEARCH_API_KEY` if set.

Browser automation: use the Chrome DevTools MCP (chrome-devtools-mcp) for browser testing, NOT Playwright or the Claude Chrome extension. Install with: "claude mcp add chrome-devtools --scope user npx chrome-devtools-mcp@latest". Use this to interact with the UI at http://localhost:4000 for testing models, chat, and features.

Common gotchas: use port 4000 for the UI (not 8000), keep backend running for Playwright tests, use Chrome DevTools MCP for browser automation (not Claude Chrome extension), must kill running llama_chat_web.exe before rebuilding on Windows (Access denied — now spawns 2 processes, kill both), prefer existing modules rather than duplicating code when editing web routes or chat logic. Adding untracked directories (vendor/, node_modules/) inside the project invalidates cargo fingerprints → forces full CUDA rebuild.

Shell execution (Windows): `command.rs` uses direct execution first (`Command::new`), PowerShell fallback for builtins. `needs_shell()` detects operators (|, >, &&, ;) → routes through PowerShell. `$` escaped to backtick-$ for PowerShell. CRITICAL: `stdin(Stdio::null())` on ALL child processes — without this, children inherit the worker's IPC stdin pipe and MSYS2 tools (wc, grep, etc.) hang forever. Streaming command output: `execute_command_streaming()` uses `cmd /C "... 2>&1"` + byte-level pipe reading for live output (progress bars, cargo/npm output). Callback sends each line via token_sender → IPC → WebSocket.

Architecture:

GPU layer safety: NEVER set ngl (n_gpu_layers) higher than the model's actual block_count. ngl > n_layers offloads the output/embedding layer to GPU, which causes llama_decode to hang on Qwen3.5 (hybrid MoE+Mamba2/DeltaNet) with large context (262K). The vram_calculator reads GGUF block_count and caps automatically. model_manager.rs also caps explicitly-requested gpu_layers. Use ngl = n_layers (e.g., 40 for Qwen3.5's 40 layers), NOT 99.

Backend (src/web/): chat/ is the inference pipeline (generation.rs token loop with conditional KV cache GPU offload, templates.rs prompt formatting, command_executor.rs tool execution, tool_tags.rs per-model tag config, stop_conditions.rs). routes/ has HTTP/WebSocket handlers (chat, config, model, conversations, tools, files, health, logs, system, frontend_logs, static_files). database/ is SQLite persistence (conversations, messages, config, migration). models.rs defines shared types (SamplerConfig, SharedLlamaState, ChatRequest/Response). config.rs loads assets/config.json and resolves system prompts. model_manager.rs handles model loading/unloading with VRAM-based GPU layer calculation. websocket.rs handles WebSocket streaming. gguf_utils.rs extracts model metadata for auto-configuration. vram_calculator.rs computes optimal GPU layer count.

Chat pipeline: WebSocket message -> load config + model -> resolve system prompt (agentic/custom/model-default) -> format with chat template (ChatML/Mistral/Llama3/Gemma) using model-specific tool tags -> tokenize -> create context with KV cache on GPU if gpu_layers > 0 -> generate tokens in loop -> check stop conditions -> detect and execute commands (regex on tool tags) -> inject output back into context -> stream tokens to frontend -> log to SQLite.

Key types: SharedLlamaState (Arc<Mutex<Option<LlamaModelState>>>) wraps the loaded model. ConversationLogger writes to SQLite per-conversation. ToolTags (exec_open/close, output_open/close) are per-model (Qwen uses <tool_call>/<\/tool_call>, Mistral uses [TOOL_CALLS]/[/TOOL_CALLS], GLM uses <tool_call>/<|end_of_box|>, Harmony uses <|start|>tool<|message|>/<|end|>, default uses SYSTEM.EXEC). SamplerConfig holds all inference params plus model_path and model_history.

Dynamic tool_tags pipeline: Tool tags are now resolved dynamically from GGUF model metadata + presets and flow through the entire stack without hardcoded model-specific branches. Backend: `tool_tags.rs` resolves tags via `get_tool_tags_for_model()` (exact match → fuzzy match → default), then `build_model_exec_regex()` builds detection regex from the resolved tags. Command detection in `command_executor.rs` uses FORMAT_PRIORITY (6 detectors): model_specific → exec → llama3 → harmony → mistral_bracket → mistral_json. Frontend: `toolFormatUtils.ts` resolves tags client-side, `toolSpanCollectors.ts` builds collectors dynamically from resolved tags. Adding new model tool tag support: update `tool_tags.rs` MODEL_TAG_MAP (backend) and `modelPresets.ts` MODEL_TOOL_TAGS (frontend) — see SYNC comments.

Common pitfall: Some models use DIFFERENT special tokens for open vs close tags. Example: GLM-4 opens with `<tool_call>` (token 151352) but closes with `<|end_of_box|>` (different token), NOT `</tool_call>` (token 151353). If the close tag is wrong, the tool call regex never matches and the model hallucinates fake tool responses.

Harmony model support (gpt-oss-20b): Detected via `<|start|>` + `<|end|>` + `<|channel|>` in the Jinja2 template. Uses channel-based turn structure: `<|start|>assistant<|channel|>analysis<|message|>` for reasoning, `to=tool_name code<|message|>{JSON}<|call|>` for tool calls, `<|start|>tool<|message|>...<|end|>` for tool output, and `<|channel|>final<|message|>` for the user-facing response. Frontend parser in useMessageParsing.ts (parseHarmonyContent) extracts ordered segments preserving chronological flow.

Frontend (src/): Atomic design — atoms/ (Button, Dialog, etc.), molecules/ (MessageInput, ThinkingBlock, CommandExecBlock, ToolCallBlock), organisms/ (ModelSelector, SettingsModal, Sidebar), templates/ (ChatInputArea, MessagesArea). Message widgets: ThinkingBlock (collapsible reasoning with streaming support — detects unclosed `<think>` tags during streaming), CommandExecBlock (tool execution with animated "Executing Tool..." state when output is null, collapsed details with CLI-style command summary when complete), ToolCallBlock (JSON tool calls). Key hooks: useChat (messaging orchestration), useModel (model lifecycle), useMessageParsing (extracts thinking/commands/tool calls from message content, handles Harmony format), useToolExecution (tool call parsing with MAX_TOOL_ITERATIONS=20), useSettings (sampler config). Utils: chatTransport (HTTP/WS abstraction), toolParser (multi-format tool call extraction).

Model auto-configuration: When loading a GGUF model, gguf_utils.rs extracts general.sampling.* keys for optimal parameters. Fallback presets in src/config/modelPresets.ts keyed by general.name. Priority: GGUF embedded params -> preset lookup -> defaults. All model file paths, recommended configs, tool formats, and test results are documented in `docs/MODEL_CONFIGURATIONS.md`. Model GGUF files live under `E:/.lmstudio/` (subdirs: lmstudio-community/, Mungert/, etc.).

Out-of-process worker: The model runs in a child process (`llama_chat_web.exe --worker`) spawned by the web server. Communication is JSON Lines over stdin/stdout pipes. Key files: `src/web/worker/` — `ipc_types.rs` (protocol types), `worker_main.rs` (child entry point, 3-thread design), `worker_bridge.rs` (server-side `WorkerBridge` abstraction replacing `SharedLlamaState + GenerationQueue`), `process_manager.rs` (spawn/kill/restart). `SharedWorkerBridge = Arc<WorkerBridge>` is passed to all route handlers. Force-unload (`POST /api/model/hard-unload`) kills the child process so the OS reclaims ALL VRAM/RAM, then auto-restarts a fresh worker. The worker's `run_generation` uses a single-threaded tokio runtime — token forwarding uses a real OS thread (not tokio::spawn) because the generation loop has no yield points.

Tools (tools/ directory): Standalone Rust utilities, each with their own Cargo.toml. "tools/ensure-cmake" auto-downloads CMake if missing (also a library used by start-dev). "tools/start-dev" kills old processes and launches backend + Vite. GPU backend via `--gpu cuda|vulkan|cpu` (default cpu). Quick commands: `npm run start:cuda`, `npm run start:vulkan`, `npm run start:cpu`, or `npm start` (run existing binary). "tools/print-metadata" inspects GGUF model files (`npm run inspect-gguf <path-to-gguf>`).

llama-cpp-rs submodule: Embedded as git submodule at `deps/llama-cpp-rs` (nested: `deps/llama-cpp-rs/llama-cpp-sys-2/llama.cpp`). Origin: `https://github.com/utilityai/llama-cpp-rs.git`. Cargo.toml uses path deps: `path = "deps/llama-cpp-rs/llama-cpp-2"` and `llama-cpp-sys-2`. To update llama.cpp: `cd deps/llama-cpp-rs/llama-cpp-sys-2/llama.cpp && git pull`. To pull upstream Rust binding fixes: `cd deps/llama-cpp-rs && git pull`. Vision/mtmd disabled by default — gated behind `#[cfg(feature = "vision")]`. ALWAYS use `npm run cargo -- build ...` for manual cargo builds (not bare `cargo build`) — ensures cmake is available. Full CUDA rebuild ~27 min; incremental Rust-only ~20s. NEVER test with debug build — prompt eval 5+ min in debug vs <2s in release.

Linting: "cargo clippy --bin llama_chat_web --features cuda" for Rust. "npm run lint" for frontend ESLint. Both should report zero warnings on clean code.

Performance: Token pipeline optimized to 149+ tok/s (from 25.6 baseline) on RTX 4090. Key optimizations: KV cache reuse between turns (InferenceCache stores LlamaContext<'static> via transmute), system prompt caching, token prefix comparison to skip re-evaluating cached tokens, cancellation via CancellationFlag (Arc<AtomicBool>) checked every 4 tokens. CRITICAL: context_size MUST fit in VRAM — CUDA VMM silently pages to RAM if oversubscribed → 70ms/tok instead of 5ms.

Sampler system: 11 sampler types in `create_sampler()` in `generation.rs`. Chain ordering: penalties → DRY → top_n_sigma → filtering samplers → terminal (dist/greedy). DRY sampler requires &LlamaModel reference. LlamaSampler::temp() alone crashes — must chain with terminal sampler.

WebSocket /ws/status: Persistent health-check WebSocket with 20s ping keep-alive. Frontend ConnectionProvider (ConnectionContext.tsx) reconnects with exponential backoff (500ms → 5s). Red ConnectionBanner appears when server disconnects, disappears on reconnect. Skipped in Tauri mode.

System monitor: SystemResourcesContext polls at 10s background rate, switches to 3s when sidebar is open (setMonitorActive). Graphs render instantly on sidebar open because data is pre-populated. SystemUsage is a pure rendering consumer.

Model testing results (RTX 4090 24GB, 6-test agent suite): PASS 6/6: Devstral-Small-2-2512, Devstral-Small-2507, Magistral-Small-2509, Qwen3-Coder-30B-A3B-1M, Ministral-3-14B-Reasoning. NEAR-PASS 5.5/6: GLM-4.7-Flash. Config is critical — wrong temp/context can drop a 6/6 model to 1/6. Models <14B generally fail. See `docs/MODEL_CONFIGURATIONS.md`.

TTS Notifications: Claude Code hooks in `.claude/hooks/` trigger TTS on events. Unified dispatcher `hook.py` handles session/submit/notification/stop events, selects engine via TTS_ENGINE variable. Current engine: "homer" (Qwen3-TTS 1.7B voice clone with Homer Simpson reference audio, Spanish LATAM). Engine scripts in `.claude/hooks/engines/run_homer.py` using faster-qwen3-tts (CUDA graphs, GPU-only). Venv at `E:/repo/faster-qwen3-tts/.venv/`. Available engines: "kitten" (KittenTTS CPU English), "homer" (faster-qwen3-tts GPU Spanish), "f5", "chatterbox", "cosyvoice". The stop hook reads `last_assistant_message`, strips markdown, and speaks it aloud.
