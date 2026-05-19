# Installation

## Prerequisites

| Requirement | Version | Notes |
|-------------|---------|-------|
| Rust | stable | `rustup update stable` |
| Node.js | 18+ | for the React frontend |
| CMake | 3.15+ | for building llama.cpp |
| Git | any | submodules required |

**For GPU (CUDA):** CUDA Toolkit 12.x + NVIDIA driver 525+

**For GPU (Metal):** macOS 12+ with Apple Silicon or AMD GPU

### Windows additional requirements
- Visual Studio 2022 with "Desktop development with C++" workload
- For the desktop app: WebView2 runtime (pre-installed on Windows 11)

---

## Install from source

```bash
git clone --recurse-submodules <repo-url>
cd llama_cpp_rs_chat
npm install
```

CMake is downloaded automatically on first build if not found on PATH.

---

## Build modes

### Desktop app (recommended)

Builds the Tauri desktop application. Does **not** bundle CUDA — the user installs CUDA separately and the app detects it at runtime.

```bash
npm run tauri:build
```

The installer is placed in `target/release/bundle/`.

### Web server (headless)

Runs as a plain HTTP server — useful for servers, Docker, or when Tauri is not needed.

**CPU only:**
```bash
cargo build --bin llama_chat_web --release
```

**CUDA (Windows/Linux):**
```bash
cargo build --bin llama_chat_web --features cuda,vision --release
```

**Metal (macOS):**
```bash
cargo build --bin llama_chat_web --features metal,vision --release
```

---

## Development

```bash
# Desktop app with auto GPU detection
npm run dev:auto:desktop

# Web server + Vite frontend
npm run dev:auto
```

Frontend runs on **http://localhost:14000**, backend on **http://localhost:18080**.

---

## Docker

A CPU-only Docker image for testing:

```bash
docker build -f Dockerfile.test-cmake -t llama-chat-cpu .
docker run -p 14000:14000 -p 18080:18080 -v /path/to/models:/app/models llama-chat-cpu
```

A CUDA image:
```bash
docker build -f Dockerfile.cuda -t llama-chat-cuda .
docker run --gpus all -p 14000:14000 -p 18080:18080 -v /path/to/models:/app/models llama-chat-cuda
```

---

## Verify the build

1. Start the server: `npm run dev:auto`
2. Open http://localhost:14000
3. Click **Load Model** and select a `.gguf` file
4. Send a test message
