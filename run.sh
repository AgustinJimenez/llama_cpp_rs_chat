#!/bin/bash
# Unified run script - all configuration via .env file
# Usage: ./run.sh

echo "🚀 LLaMA.cpp Chat Runner"

# Set defaults
LLAMA_LOG_LEVEL=${LLAMA_LOG_LEVEL:-3}
LLAMA_DEBUG=${LLAMA_DEBUG:-false}
RUN_MODE=${RUN_MODE:-normal}
PAUSE_ON_EXIT=${PAUSE_ON_EXIT:-false}

# Load .env file if it exists
if [ -f .env ]; then
    echo "📄 Loading configuration from .env..."
    export $(cat .env | grep -v '^#' | grep -v '^$' | xargs)
else
    echo "⚙️  No .env file found - using defaults"
    echo "   💡 Copy .env.example to .env to customize settings"
fi

# Display current configuration
echo "🔧 Configuration:"
echo "   RUN_MODE=${RUN_MODE}"
echo "   LLAMA_LOG_LEVEL=${LLAMA_LOG_LEVEL}"
echo "   LLAMA_DEBUG=${LLAMA_DEBUG}"

# Handle different run modes
case "$RUN_MODE" in
    "silent")
        echo "🔇 Running in silent mode (stderr suppressed)..."
        cargo run 2>/dev/null
        ;;
    "debug")
        echo "🐛 Running in debug mode (all logs visible)..."
        export LLAMA_LOG_LEVEL=0
        export LLAMA_DEBUG=true
        cargo run
        ;;
    "normal"|*)
        echo "🚀 Running in normal mode..."
        cargo run
        ;;
esac

echo "✅ Execution completed"

# Pause if requested
if [ "$PAUSE_ON_EXIT" = "true" ]; then
    echo "Press any key to continue..."
    read -n 1 -s
fi