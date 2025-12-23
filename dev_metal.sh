#!/bin/bash
# Development script with Metal acceleration for macOS

echo "🚀 Starting development server with Metal GPU acceleration for macOS..."
echo "🔧 Using Apple Metal Performance Shaders for LLM inference"
echo ""

# Check if we're on macOS
if [[ "$OSTYPE" != "darwin"* ]]; then
    echo "❌ Metal acceleration is only available on macOS"
    echo "💡 Use 'npm run dev' for CPU mode or 'npm run dev:cuda' for CUDA (Windows)"
    exit 1
fi

# Check for Apple Silicon (optional - Metal works on Intel Macs too)
if [[ $(uname -m) == "arm64" ]]; then
    echo "🍎 Detected Apple Silicon - Metal acceleration will be optimal"
else
    echo "🍎 Detected Intel Mac - Metal acceleration available"
fi

echo "📦 Building with Metal support..."
npm run dev:metal