# CUDA Setup Guide

## Current Status

✅ CUDA Toolkit 12.8 is installed
✅ nvcc (CUDA compiler) is working
❌ Visual Studio CUDA integration is missing
❌ MSVC compiler not in PATH

## The Problem

CMake found your CUDA Toolkit but couldn't find the **Visual Studio CUDA build customizations**. These are files that let Visual Studio compile CUDA code, and they're separate from the CUDA Toolkit itself.

The error you saw:
```
CMake Error: No CUDA toolset found.
```

This happens because:
1. You have Visual Studio **Build Tools** (minimal installation)
2. But you need Visual Studio with **CUDA integration**

## Solution 1: Install Visual Studio with CUDA Support (Recommended)

### Option A: Visual Studio Community (Free, Full IDE)

1. Download Visual Studio Community: https://visualstudio.microsoft.com/
2. During installation, select:
   - ✅ **Desktop development with C++**
   - ✅ Under "Individual Components", search and check:
     - **MSVC v143 - VS 2022 C++ x64/x86 build tools**
     - **C++ CMake tools for Windows**
     - **C++ ATL for latest build tools**
3. After installation, CUDA will automatically integrate with Visual Studio

### Option B: Add to Existing Build Tools

1. Run Visual Studio Installer
2. Modify your "Build Tools for Visual Studio 2022"
3. Select:
   - ✅ **Desktop development with C++** (full workload)
   - ✅ Under "Individual Components":
     - **MSVC v143 build tools**
     - **Windows SDK**
     - **C++ CMake tools**

## Solution 2: Use Visual Studio Developer Command Prompt

If you already have Visual Studio installed elsewhere:

1. Open **"x64 Native Tools Command Prompt for VS 2022"** from Start Menu
2. Navigate to your project:
   ```cmd
   cd E:\repo\llama_cpp_rs_chat
   ```
3. Build with CUDA:
   ```cmd
   cargo build --bin llama_chat_web --features docker --release
   ```

Or use the provided script:
```cmd
build_cuda.bat
```

## Solution 3: Enable CUDA Feature in Cargo.toml

Once Visual Studio is properly set up, enable CUDA:

1. Edit `Cargo.toml`:
   ```toml
   llama-cpp-2 = { version = "0.1.122", optional = true, features = ["cuda"] }
   ```

2. Clean and rebuild:
   ```cmd
   cargo clean
   cargo build --bin llama_chat_web --features docker --release
   ```

## Verify CUDA Setup

After installation, verify everything works:

```cmd
# Check CUDA compiler
nvcc --version

# Check Visual Studio compiler
where cl.exe

# Check CMake
cmake --version

# Try building
cargo build --bin llama_chat_web --features docker
```

## Current Workaround (CPU Mode)

For now, the app is running in **CPU mode** (slower but works):
- Build time: ~4-5 minutes
- Generation speed: Slower (CPU-based)
- No Visual Studio CUDA integration needed

## What You'll Get with CUDA

Once properly configured:
- **10-50x faster inference** depending on model size
- GPU memory usage (8-16GB VRAM typical)
- Offload model layers to GPU automatically

## Quick Test

To test if CUDA would work now, try this from **Visual Studio Developer Command Prompt**:

```cmd
cd E:\repo\llama_cpp_rs_chat
set "CARGO_MANIFEST_DIR=E:\repo\llama_cpp_rs_chat"
cargo build --bin llama_chat_web --features docker
```

If this works, your issue is just that the regular command prompt doesn't have the Visual Studio environment variables set up.

## Alternative: Use Pre-built CUDA Libraries

If you can't get Visual Studio CUDA working, consider:
1. Use CPU mode (current setup - works fine, just slower)
2. Use a different LLM backend (like llama.cpp server with pre-built binaries)
3. Use cloud GPU (RunPod, Vast.ai, etc.)

## Need Help?

If you're stuck:
1. Make sure you have at least 20GB free disk space
2. Restart your computer after installing Visual Studio
3. Try the `build_cuda.bat` script
4. Check that `cl.exe` is in PATH after VS installation
