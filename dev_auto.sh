#!/bin/bash
# Auto-detection script for optimal GPU acceleration
# Dynamically detects OS, GPU capabilities, and runs the best configuration
# Usage: ./dev_auto.sh [web|desktop]

set -e

# Get mode from first argument, default to web
MODE=${1:-web}

if [[ "$MODE" != "web" && "$MODE" != "desktop" ]]; then
    echo "❌ Invalid mode: $MODE. Use 'web' or 'desktop'"
    exit 1
fi

echo "🔍 Auto-detecting optimal GPU acceleration setup..."
echo "🎯 Mode: $(echo $MODE | tr '[:lower:]' '[:upper:]')"
echo ""

# Detect OS
OS=$(uname -s)
ARCH=$(uname -m)

echo "🖥️  Platform: $OS ($ARCH)"

# Initialize variables
USE_METAL=false
USE_CUDA=false
USE_CPU=false
FEATURES=""
SCRIPT_CMD=""

# Function to check if command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Function to check CUDA on Windows/Linux
check_cuda() {
    if command_exists nvcc; then
        local nvcc_version=$(nvcc --version 2>/dev/null | grep -o "release [0-9]\+\.[0-9]\+" | grep -o "[0-9]\+\.[0-9]\+" | head -1)
        if [ ! -z "$nvcc_version" ]; then
            echo "✅ CUDA Toolkit detected (version $nvcc_version)"
            return 0
        fi
    fi
    echo "❌ CUDA Toolkit not found"
    return 1
}

# Function to check Visual Studio on Windows (if running in Git Bash/WSL)
check_visual_studio() {
    if command_exists cl.exe || [ -f "/c/Program Files/Microsoft Visual Studio/2022/Community/VC/Tools/MSVC/"*"/bin/Hostx64/x64/cl.exe" ]; then
        echo "✅ Visual Studio/MSVC compiler detected"
        return 0
    fi
    echo "❌ Visual Studio/MSVC compiler not found"
    return 1
}

# Function to check Metal on macOS
check_metal() {
    if [ "$OS" = "Darwin" ]; then
        # Check if we have a GPU (all Macs since 2012 have GPU)
        if system_profiler SPDisplaysDataType >/dev/null 2>&1; then
            local gpu_info=$(system_profiler SPDisplaysDataType | grep -E "(Chipset Model|GPU)" | head -1)
            echo "✅ Metal support available: $gpu_info"
            return 0
        fi
    fi
    return 1
}

echo ""
echo "🔍 Checking GPU acceleration options..."

# Platform-specific detection
case "$OS" in
    "Darwin")
        echo "🍎 macOS detected"
        if check_metal; then
            USE_METAL=true
            FEATURES="metal"
            if [[ "$MODE" == "desktop" ]]; then
                SCRIPT_CMD="tauri:dev:metal"
            else
                SCRIPT_CMD="dev:metal"
            fi
            echo "🚀 Will use Metal acceleration for optimal performance"
        else
            USE_CPU=true
            if [[ "$MODE" == "desktop" ]]; then
                SCRIPT_CMD="tauri:dev"
            else
                SCRIPT_CMD="dev"
            fi
            echo "⚠️  Metal not available, falling back to CPU"
        fi
        ;;
    
    "MINGW"*|"MSYS"*|"CYGWIN"*)
        echo "🪟 Windows (Git Bash/MSYS) detected"
        if check_cuda && check_visual_studio; then
            USE_CUDA=true
            FEATURES="cuda"
            if [[ "$MODE" == "desktop" ]]; then
                SCRIPT_CMD="tauri:dev:cuda"
            else
                SCRIPT_CMD="dev:cuda"
            fi
            echo "🚀 Will use CUDA acceleration for optimal performance"
        else
            USE_CPU=true
            if [[ "$MODE" == "desktop" ]]; then
                SCRIPT_CMD="tauri:dev"
            else
                SCRIPT_CMD="dev"
            fi
            echo "⚠️  CUDA/Visual Studio not properly configured, falling back to CPU"
            echo "💡 Run 'build_cuda.bat' for CUDA setup instructions"
        fi
        ;;
    
    "Linux")
        echo "🐧 Linux detected"
        if check_cuda; then
            echo "⚠️  CUDA detected but not configured for this script"
            if [[ "$MODE" == "desktop" ]]; then
                echo "💡 You can manually use: cargo tauri dev --features cuda"
            else
                echo "💡 You can manually use: cargo build --features cuda --bin llama_chat_web"
            fi
        fi
        USE_CPU=true
        if [[ "$MODE" == "desktop" ]]; then
            SCRIPT_CMD="tauri:dev"
        else
            SCRIPT_CMD="dev"
        fi
        echo "🔄 Using CPU mode (recommended for Linux)"
        ;;
    
    *)
        echo "❓ Unknown OS: $OS"
        USE_CPU=true
        if [[ "$MODE" == "desktop" ]]; then
            SCRIPT_CMD="tauri:dev"
        else
            SCRIPT_CMD="dev"
        fi
        echo "🔄 Using CPU fallback mode"
        ;;
esac

echo ""
echo "📊 Configuration Summary:"
echo "   OS: $OS"
echo "   Architecture: $ARCH"
echo "   Metal: $([ "$USE_METAL" = true ] && echo "✅ Enabled" || echo "❌ Disabled")"
echo "   CUDA: $([ "$USE_CUDA" = true ] && echo "✅ Enabled" || echo "❌ Disabled")"
echo "   CPU Fallback: $([ "$USE_CPU" = true ] && echo "✅ Active" || echo "❌ Not needed")"
if [ ! -z "$FEATURES" ]; then
    echo "   Features: $FEATURES"
fi
echo "   Command: npm run $SCRIPT_CMD"
echo ""

# Display performance expectations
if [ "$USE_METAL" = true ]; then
    echo "🎯 Expected Performance: 5-20x faster than CPU (Metal GPU acceleration)"
elif [ "$USE_CUDA" = true ]; then
    echo "🎯 Expected Performance: 10-50x faster than CPU (CUDA GPU acceleration)"
else
    echo "🎯 Expected Performance: CPU-only mode (slower but compatible)"
fi

echo ""
echo "🚀 Starting development server with optimal configuration..."
if [[ "$MODE" == "desktop" ]]; then
    echo "🖥️  Desktop App: Native window will open"
    echo "🔧 Backend: Embedded within desktop app"
else
    echo "🌐 Frontend: http://localhost:4000"
    echo "🔧 Backend API: http://localhost:8000"
fi
echo ""

# Run the optimal command
npm run "$SCRIPT_CMD"