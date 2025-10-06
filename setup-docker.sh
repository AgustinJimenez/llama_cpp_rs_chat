#!/bin/bash

# LLaMA Chat Docker Setup Script

set -e

echo "🦙 LLaMA Chat Docker Setup"
echo "=========================="

# Check if Docker is installed
if ! command -v docker &> /dev/null; then
    echo "❌ Docker is not installed. Please install Docker first."
    echo "   Visit: https://docs.docker.com/get-docker/"
    exit 1
fi

# Check for Docker Compose (try V2 first, then V1)
DOCKER_COMPOSE_CMD=""
if docker compose version &> /dev/null; then
    DOCKER_COMPOSE_CMD="docker compose"
    echo "✅ Docker Compose V2 detected"
elif command -v docker-compose &> /dev/null; then
    DOCKER_COMPOSE_CMD="docker-compose"
    echo "✅ Docker Compose V1 detected"
else
    echo "❌ Docker Compose is not installed. Please install Docker Compose first."
    echo "   Visit: https://docs.docker.com/compose/install/"
    exit 1
fi

# Check if Docker daemon is running
if ! docker info &> /dev/null; then
    echo "❌ Docker daemon is not running. Please start Docker first."
    echo ""
    echo "To start Docker:"
    if [[ "$OSTYPE" == "darwin"* ]]; then
        echo "   - Open Docker Desktop application"
        echo "   - Or run: open -a Docker"
    elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
        echo "   - Run: sudo systemctl start docker"
        echo "   - Or: sudo service docker start"
    else
        echo "   - Start Docker Desktop or Docker service"
    fi
    echo ""
    exit 1
fi

echo "✅ Docker daemon is running"

# Check Docker version and warn if too old
DOCKER_VERSION=$(docker --version | grep -oE '[0-9]+\.[0-9]+' | head -1)
MIN_VERSION="20.10"

if [ "$(printf '%s\n' "$MIN_VERSION" "$DOCKER_VERSION" | sort -V | head -n1)" != "$MIN_VERSION" ]; then
    echo "⚠️  Warning: Docker version $DOCKER_VERSION detected. Recommended: $MIN_VERSION or higher"
    echo "   Some features may not work correctly with older Docker versions"
    echo ""
fi

# Create necessary directories
echo "📁 Creating necessary directories..."
mkdir -p ./assets/conversations
mkdir -p ./config
mkdir -p ./ssl

# Check if model exists
MODEL_PATH="$HOME/.lmstudio/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf"
if [ -f "$MODEL_PATH" ]; then
    echo "✅ Model found at $MODEL_PATH"
else
    echo "⚠️  Model not found at $MODEL_PATH"
    echo "   Please ensure you have the Granite model downloaded via LM Studio"
    echo "   Or update the MODEL_PATH in docker-compose.yml"
fi

# Create config directory with sample config
cat > ./config/llama-config.toml << EOF
[model]
path = "/app/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf"
context_size = 32768

[sampler]
type = "Greedy"
temperature = 0.7
top_p = 0.95
top_k = 20
mirostat_tau = 5.0
mirostat_eta = 0.1
EOF

echo "✅ Created sample configuration"

# Build the Docker image
echo "🔨 Building Docker image..."
docker build -t llama-chat-app .

echo ""
echo "🎉 Setup complete!"
echo ""
echo "Available commands:"
echo "  🚀 Start main app:     $DOCKER_COMPOSE_CMD up llama-chat-app"
echo "  🖥️  Start CLI version:  $DOCKER_COMPOSE_CMD --profile cli up llama-chat-cli"
echo "  🔧 Development mode:   $DOCKER_COMPOSE_CMD --profile dev up llama-chat-dev"
echo "  🌐 Production mode:    $DOCKER_COMPOSE_CMD --profile production up"
echo ""
echo "The application will be available at:"
echo "  - Main app: http://localhost:3000"
echo "  - Health check: http://localhost:3000/health"
echo ""
echo "To view logs:"
echo "  $DOCKER_COMPOSE_CMD logs -f llama-chat-app"
echo ""
echo "To stop:"
echo "  $DOCKER_COMPOSE_CMD down"