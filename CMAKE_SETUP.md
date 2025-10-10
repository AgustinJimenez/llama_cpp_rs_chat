# CMake Setup Guide for LLaMA Integration

This project uses **real LLaMA implementation by default** for actual AI model inference.

The **mock implementation** is only used for automated E2E testing to avoid requiring actual model files.

## Current Status

By default, the project uses the **real LLaMA implementation** which requires CMake to build the underlying llama.cpp library.

## Setup for Real LLaMA Implementation

To build and run the application with actual LLaMA models:

### Step 1: Install CMake

CMake is required to build the underlying llama.cpp library. Unfortunately, this cannot be bundled as a Rust dependency.

#### Windows Installation Options:

**Option 1 - Official Installer (Recommended):**
1. Visit: https://cmake.org/download/
2. Download 'Windows x64 Installer'
3. **IMPORTANT**: During installation, check "Add CMake to system PATH for all users"
4. Restart your terminal/IDE after installation

**Option 2 - Package Managers:**
```bash
# Chocolatey
choco install cmake

# Scoop
scoop install cmake

# Windows Package Manager (winget)
winget install Kitware.CMake
```

### Step 2: Verify Installation

After installation, verify CMake is available:
```bash
cmake --version
```

You should see something like:
```
cmake version 3.x.x
```

### Step 3: Build the Application

Real LLaMA implementation is enabled by default:


```bash
cargo build
```

The first build will take significantly longer (10-30 minutes) as it compiles the entire llama.cpp library.

## Troubleshooting

### "cmake: command not found" or "program not found"

This means CMake is not in your system PATH. Try:

1. **Restart your terminal/IDE** after installing CMake
2. **Check PATH manually**:
   ```bash
   echo $PATH  # On Linux/Mac
   echo %PATH% # On Windows CMD
   $env:PATH   # On Windows PowerShell
   ```
3. **Add manually to PATH** if needed:
   - Windows: Add `C:\Program Files\CMake\bin` to your PATH
   - Or set environment variable: `CMAKE=C:\Program Files\CMake\bin\cmake.exe`

### Build taking too long

The first build compiles the entire llama.cpp library and can take 10-30 minutes depending on your system. Subsequent builds are much faster.

### Memory issues during build

If you encounter memory issues:
1. Close other applications
2. Use release mode: `cargo build --release`
3. Reduce parallel jobs: `cargo build -j 1`

## Development Workflow

### For Normal Development
The real LLaMA implementation is used by default:
```bash
cargo build
cargo tauri dev
```

### For E2E Testing Only
Mock implementation can be used to run tests without requiring actual models:
```bash
cargo test --features mock --no-default-features
```

## System Requirements

- **Windows**: Visual Studio Build Tools or Visual Studio Community
- **CMake**: Version 3.15 or later
- **Memory**: At least 4GB RAM for compilation
- **Storage**: ~2GB for compiled artifacts

## Alternative Solutions

If you continue having CMake issues:

1. **Use WSL2** (Windows Subsystem for Linux):
   ```bash
   # In WSL2
   sudo apt install cmake build-essential
   cargo build
   ```

2. **Use Docker** for development:
   ```bash
   docker-compose up --build
   ```

3. **Stick with mock mode** for UI development and use external API for AI features

## Getting Help

If you're still having issues:
1. Check that CMake is in your PATH: `where cmake` (Windows) or `which cmake` (Linux/Mac)
2. Try running CMake directly: `cmake --version`
3. Check our build script logs for more specific error messages
4. Consider using the Docker-based development environment