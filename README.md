# LLaMA Chat

A multi-provider AI chat application built with Rust and React. Run local GGUF models via llama.cpp, or connect to 15+ cloud providers (Groq, Gemini, Claude, Mistral, etc.) — all with agentic tool execution. Available as a web app, desktop app (Tauri), or CLI.

## Features

- **Multi-provider** — Local (llama.cpp), Claude Code, Codex CLI, and 13 OpenAI-compatible cloud providers with agentic tool loop
- **120+ native tools** — organized via tool catalog (24 core tools in prompt, 90+ desktop tools discoverable on demand via `list_tools`/`get_tool_details`)
- **Local inference** powered by llama-cpp-2 with CUDA and Metal GPU acceleration
- **Web and desktop** modes (Tauri) from the same codebase
- **Desktop automation (computer use)** — click, type, press keys, scroll, OCR, UI tree inspection via `enigo` crate
- **Vision support** — paste/drop images in chat, screenshot tool for screen capture (always compiled)
- **MCP (Model Context Protocol)** — connect external tool servers, auto-connect on startup
- **Web search and fetch** — Brave Search API, Google via headless Chrome, DuckDuckGo fallback
- **Custom providers** — add any OpenAI-compatible endpoint (vLLM, Ollama, LM Studio)
- **Jinja2 prompt templates** — renders native chat templates from GGUF metadata via minijinja
- **Auto-configuration** — extracts optimal sampling parameters from GGUF metadata
- **11 sampler types** — Greedy, Temperature, TopP, TopK, MinP, Mirostat, DRY, and chain variants
- **Conversation compaction** — map-reduce summarization when context fills up, auto-continue
- **Conversation history** stored in SQLite with auto-generated titles
- **Out-of-process worker** — model runs in child process; kill to reclaim all VRAM instantly
- **Docker support** — CPU and CUDA images included

## Quick Start

```bash
npm install
npm run dev:auto        # auto-detects GPU (CUDA/Metal/CPU)
```

Opens at **http://localhost:14000**. The backend runs on port 18080.

CMake is required to build llama.cpp. If it's not installed, the build toolchain downloads a portable copy automatically — no manual install needed.

### Manual GPU selection

```bash
npm run dev:cuda        # NVIDIA GPU (CUDA)
npm run dev:metal       # Apple GPU (Metal)
npm run dev             # CPU only (very slow — use dev:cuda or dev:auto for GPU)
```

> **Note:** `npm run dev` and `npm run dev:web` do **not** enable GPU acceleration. Always use `npm run dev:auto` or `npm run dev:cuda` for usable performance.

### Desktop app (Tauri)

```bash
npm run dev:auto:desktop   # auto-detect GPU
npm run tauri:dev:cuda     # or manual
cargo tauri build          # production build
```

## Docker

### CPU

```bash
docker build -f Dockerfile.test-cmake -t llama-cpu .
docker run -p 8000:8000 \
  -v /path/to/models:/app/models \
  -v ./assets:/app/assets \
  llama-cpu
```

### CUDA (NVIDIA GPU)

```bash
docker build -f Dockerfile.cuda -t llama-cuda .
docker run --gpus all -p 8000:8000 \
  -v /path/to/models:/app/models \
  -v ./assets:/app/assets \
  llama-cuda
```

Mount your models directory to `/app/models` and browse them in the UI.

## System Requirements

### Windows (Installer)

| Requirement | Required? | Notes |
|---|---|---|
| Windows 10+ (64-bit) | **Required** | |
| [Visual C++ Runtime 2022](https://aka.ms/vs/17/release/vc_redist.x64.exe) | **Required** | Most apps install this; you may already have it |
| NVIDIA GPU drivers | Optional | Enables CUDA acceleration. Install via [nvidia.com/drivers](https://www.nvidia.com/drivers) |
| Vulkan GPU drivers | Optional | Enables Vulkan acceleration (AMD/Intel/NVIDIA). Usually included with GPU drivers |

The app auto-detects available GPU backends at startup. Without GPU drivers, it runs on CPU.

### macOS

| Requirement | Required? | Notes |
|---|---|---|
| macOS 12+ (Monterey) | **Required** | |
| Xcode Command Line Tools | For building from source | `xcode-select --install` |

Metal GPU acceleration is built into macOS — no additional drivers needed.

### Linux

| Requirement | Required? | Notes |
|---|---|---|
| glibc 2.31+ | **Required** | Ubuntu 20.04+, Fedora 32+ |
| NVIDIA GPU drivers + CUDA 12.x | Optional | For CUDA acceleration |
| Vulkan drivers (mesa-vulkan) | Optional | For Vulkan acceleration (AMD/Intel) |

### Building from Source (all platforms)

| Requirement | Notes |
|---|---|
| Rust 1.80+ | `rustup` recommended |
| Node.js 18+ | For the frontend |
| CMake 3.14+ | Auto-downloaded by the build system if not installed |
| CUDA Toolkit 12.x | Only if building with `--features cuda` |
| Vulkan SDK | Only if building with `--features vulkan` |

## Configuration

Models are configured through the web UI (Settings). The app auto-configures sampling parameters from GGUF embedded metadata when available, with fallback presets for known models.

Key settings:
- **Model path** — select any `.gguf` file
- **Sampler type** — Greedy, Temperature, TopP, TopK, Mirostat, ChainFull, etc.
- **Context size** — defaults to model's trained context or 4096
- **System prompt** — customizable per conversation
- **GPU layers** — auto-calculated based on available VRAM

Configuration is stored in `assets/config.json` and SQLite (`assets/llama_chat.db`).

## Project Structure

```
llama_cpp_rs_chat/
├── src/
│   ├── web/                    # Backend
│   │   ├── chat/               # Inference pipeline
│   │   │   ├── generation.rs   # Token generation loop
│   │   │   ├── templates.rs    # Prompt formatting (ChatML/Mistral/Llama3/Gemma)
│   │   │   ├── tool_tags.rs    # Per-model tool call tags
│   │   │   ├── command_executor.rs  # Shell command execution
│   │   │   └── stop_conditions.rs   # EOS/stop token detection
│   │   ├── routes/             # HTTP/WebSocket handlers
│   │   ├── database/           # SQLite persistence
│   │   ├── model_manager.rs    # Model loading/unloading
│   │   ├── native_tools.rs     # Web search, web fetch, file I/O, screenshots
│   │   ├── desktop_tools.rs    # Mouse, keyboard, scroll simulation (computer use)
│   │   ├── mcp/                # MCP server connections and tool proxying
│   │   ├── browser.rs          # Headless Chrome singleton for JS-rendered pages
│   │   ├── worker/             # Out-of-process model worker (IPC over stdin/stdout)
│   │   ├── gguf_utils.rs       # GGUF metadata extraction
│   │   ├── vram_calculator.rs  # GPU layer auto-calculation
│   │   └── websocket.rs        # WebSocket streaming
│   ├── components/             # Frontend (React)
│   │   ├── atoms/              # Button, Dialog, etc.
│   │   ├── molecules/          # MessageInput, ThinkingBlock, CommandExecBlock
│   │   ├── organisms/          # ModelSelector, SettingsModal
│   │   └── templates/          # ChatInputArea, MessagesArea
│   ├── config/modelPresets.ts  # Fallback model presets
│   ├── main.rs                 # Tauri desktop entry
│   ├── main_web.rs             # Web server entry
│   └── lib.rs                  # Tauri commands
├── tools/ensure-cmake/         # Auto-downloads CMake if missing
├── assets/                     # Config, DB, conversations
├── Dockerfile.cuda             # CUDA Docker image
├── Dockerfile.test-cmake       # CPU Docker image
└── package.json
```

## Development

```bash
npm run dev:auto          # Web app with auto GPU detection
npm run dev:auto:desktop  # Desktop app with auto GPU detection
npm run build             # Production frontend build
cargo test                # Rust unit tests
npm test                  # Playwright E2E tests (backend must be running)
cargo clippy --bin llama_chat_web --features cuda  # Lint
npm run lint              # Frontend ESLint
```

### Testing

```bash
npm test                  # Run all E2E tests
npm run test:headed       # Run with browser visible
npm run test:debug        # Debug mode
npm run test:docker       # Docker-based test run
```

For mock mode (no real model needed): build with `--features mock` or set `TEST_MODE=true`.

## System Requirements

- **Rust** 1.70+
- **Node.js** 16+
- **CMake** — auto-downloaded if not installed
- **CUDA toolkit** (optional) — for NVIDIA GPU acceleration
- **Xcode Command Line Tools** (macOS) — for Metal acceleration

## Troubleshooting

- **CMake not found**: The build toolchain (`tools/ensure-cmake`) downloads it automatically. If that fails, install manually: `winget install Kitware.CMake` (Windows), `brew install cmake` (macOS), `sudo apt install cmake` (Linux).
- **CUDA build fails**: Ensure CUDA toolkit is installed and `nvcc` is on PATH.
- **Port 14000 in use**: The Vite dev server uses port 14000. Kill any existing process or change the port in package.json.
- **Model won't load**: Check that the `.gguf` file path is correct and the file isn't corrupted. Try a smaller model first.

---

Built with Rust, React, Tauri, and llama.cpp
