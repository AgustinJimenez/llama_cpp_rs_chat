# Windows Build Guide

## Quick Start

### For CUDA (GPU) Support:

Run from **Command Prompt** or **PowerShell** (NOT Git Bash):

```cmd
build_cuda.bat
```

Or to start the dev server:

```cmd
dev_cuda.bat
```

### For CPU Mode:

From Git Bash (will use CPU, slower but no Visual Studio needed):
```bash
./build.sh
./dev.sh
```

## Why Two Different Scripts?

- **Git Bash** (`./build.sh`, `./dev.sh`) - Works for CPU-only mode
- **CMD/PowerShell** (`build_cuda.bat`, `dev_cuda.bat`) - Required for CUDA/GPU mode

## The Problem

Git Bash doesn't properly handle Visual Studio's environment variables needed for CUDA compilation. You must use Windows' native command prompt (CMD) or PowerShell for CUDA builds.

## What You Have

✅ Visual Studio Community 2022
✅ MSVC 14.44.35207
✅ CUDA Toolkit 12.8
✅ CUDA build targets installed

Everything is installed correctly! You just need to use the right terminal.

## How to Build with CUDA

### Option 1: Use the Batch Files (Easiest)

1. Open **Command Prompt** (or PowerShell)
2. Navigate to project:
   ```cmd
   cd E:\repo\llama_cpp_rs_chat
   ```
3. Run:
   ```cmd
   build_cuda.bat
   ```

### Option 2: Visual Studio Developer Command Prompt

1. Open Start Menu
2. Search for **"x64 Native Tools Command Prompt for VS 2022"**
3. Navigate to project and build:
   ```cmd
   cd E:\repo\llama_cpp_rs_chat
   cargo build --bin llama_chat_web --release
   ```

### Option 3: From PowerShell

```powershell
# Set up VS environment
& "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"

# Build
cargo build --bin llama_chat_web --release
```

## Testing the Build

After building successfully:

1. Start the server:
   ```cmd
   cargo run --bin llama_chat_web
   ```

2. Open your browser to: http://localhost:8000

3. You should see in the console:
   ```
   GPU acceleration enabled: 32 layers offloaded to GPU
   ```

## Performance Comparison

**CPU Mode:**
- Build time: ~5 minutes
- Token generation: ~2-5 tokens/second
- No GPU memory used

**CUDA Mode:**
- Build time: ~10-15 minutes (first time)
- Token generation: ~50-100 tokens/second (10-50x faster!)
- Uses ~4-8GB VRAM depending on model

## Troubleshooting

### "build_cuda.bat not recognized"
Make sure you're in the project directory:
```cmd
cd E:\repo\llama_cpp_rs_chat
```

### "Visual Studio environment failed to set up"
Open the batch file and verify the path:
```
C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat
```

### Build still fails with CUDA errors
Try cleaning first:
```cmd
cargo clean
build_cuda.bat
```

### Want to use Git Bash?
Git Bash only works for CPU mode. For CUDA, you MUST use CMD or PowerShell.

## Files Created

- `build_cuda.bat` - Build with CUDA support
- `dev_cuda.bat` - Start dev server with CUDA
- `build.sh` - Build CPU-only (Git Bash)
- `dev.sh` - Dev server CPU-only (Git Bash)
