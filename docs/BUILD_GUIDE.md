# Build Guide

## Prerequisites

- **CMake** 3.15+ ([download](https://cmake.org/download/) or `winget install Kitware.CMake`)
- **Visual Studio 2022** with "Desktop development with C++" workload
- **CUDA Toolkit 12.x** (for GPU acceleration)

Verify installation:
```bash
cmake --version
nvcc --version    # CUDA only
where cl.exe      # Windows only
```

## Build Modes

### CUDA (GPU) — Windows

Run from **CMD or PowerShell** (not Git Bash — it can't set up VS environment variables):

```cmd
cargo build --bin llama_chat_web --features cuda
```

Or use the batch script:
```cmd
build_cuda.bat
```

If the compiler isn't found, open **"x64 Native Tools Command Prompt for VS 2022"** from the Start Menu instead.

### Metal (GPU) — macOS

```bash
cargo build --bin llama_chat_web --features metal
```

### CPU Only (any platform)

```bash
cargo build --bin llama_chat_web
```

No CUDA or Visual Studio needed. Slower inference (~2-5 tok/s vs ~50-100 tok/s with GPU).

## Running

```bash
cargo run --bin llama_chat_web --features cuda
```

- Backend: http://localhost:8000
- Frontend (dev): http://localhost:4000

## Troubleshooting

| Problem | Solution |
|---------|----------|
| `cmake: command not found` | Add `C:\Program Files\CMake\bin` to PATH, restart terminal |
| `No CUDA toolset found` | Install VS 2022 with C++ workload + CUDA integration |
| Build fails with CUDA errors | Run `cargo clean` then rebuild from VS Developer Prompt |
| First build takes 10-30 min | Normal — compiles llama.cpp from source. Subsequent builds are fast |
| Memory issues during build | Close other apps, try `cargo build -j 1` to reduce parallelism |
| Git Bash CUDA build fails | Use CMD/PowerShell instead — Git Bash can't load VS env vars |

## npm Scripts

| Script | Description |
|--------|-------------|
| `npm run dev` | CPU mode dev server |
| `npm run dev:cuda` | CUDA mode dev server |
| `npm run dev:metal` | Metal mode dev server (macOS) |
| `npm run build:cuda` | CUDA production build |
