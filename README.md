# LLaMA Chat

A local AI chat application built with Rust and React. Runs GGUF models via llama.cpp with GPU acceleration (CUDA/Metal). Available as a web app, desktop app (Tauri), or CLI.

## Features

- **Local inference** powered by llama-cpp-2 with CUDA and Metal GPU acceleration
- **Web and desktop** modes (Tauri) from the same codebase
- **Tool execution** — models can run shell commands with safety limits
- **Auto-configuration** — extracts optimal sampling parameters from GGUF metadata
- **Multiple samplers** — Greedy, Temperature, TopP, TopK, Mirostat, and chain variants
- **Conversation history** stored in SQLite
- **Docker support** — CPU and CUDA images included

## Quick Start

```bash
npm install
npm run dev:auto        # auto-detects GPU (CUDA/Metal/CPU)
```

Opens at **http://localhost:4000**. The backend runs on port 8000.

CMake is required to build llama.cpp. If it's not installed, the build toolchain downloads a portable copy automatically — no manual install needed.

### Manual GPU selection

```bash
npm run dev:cuda        # NVIDIA GPU (CUDA)
npm run dev:metal       # Apple GPU (Metal)
npm run dev             # CPU only
```

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
│   │   ├── gguf_utils.rs       # GGUF metadata extraction
│   │   ├── vram_calculator.rs  # GPU layer auto-calculation
│   │   └── websocket.rs        # WebSocket streaming
│   ├── components/             # Frontend (React)
│   │   ├── atoms/              # Button, Dialog, etc.
│   │   ├── molecules/          # MessageInput, ToolCallBlock
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
- **Port 4000 in use**: The Vite dev server uses port 4000. Kill any existing process or change the port in package.json.
- **Model won't load**: Check that the `.gguf` file path is correct and the file isn't corrupted. Try a smaller model first.

---

Built with Rust, React, Tauri, and llama.cpp
