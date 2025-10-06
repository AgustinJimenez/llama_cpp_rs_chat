# 🦙 LLaMA Chat - Docker Deployment

This guide helps you run LLaMA Chat using Docker, avoiding macOS compilation issues with `llama-cpp-2`.

## 🎯 Why Docker?

- **Solves macOS Sequoia compilation issues** with `llama-cpp-2`
- **Consistent environment** across different systems
- **Easy deployment** and scaling
- **Isolated dependencies** 

## 🚀 Quick Start

1. **Run the setup script:**
   ```bash
   ./setup-docker.sh
   ```

2. **Start the application:**
   ```bash
   # For Docker Compose V2 (newer)
   docker compose up llama-chat-app
   
   # For Docker Compose V1 (legacy)
   docker-compose up llama-chat-app
   ```

3. **Open in browser:**
   ```
   http://localhost:3000
   ```

## 📋 Prerequisites

- Docker and Docker Compose installed
- LM Studio with Granite model downloaded (or update model path)
- At least 8GB RAM available to Docker

## 🔧 Configuration

### Model Setup

The default configuration expects the Granite model at:
```
~/.lmstudio/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf
```

To use a different model, update the `MODEL_PATH` in `docker-compose.yml`:

```yaml
environment:
  - MODEL_PATH=/app/models/your-model.gguf
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MODEL_PATH` | `/app/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf` | Path to GGUF model file |
| `LLAMA_CONTEXT_SIZE` | `32768` | Context window size |
| `LLAMA_SAMPLER_TYPE` | `Greedy` | Sampler type (see available types below) |
| `RUST_LOG` | `info` | Log level |

### Available Sampler Types

- `Greedy` - Deterministic (recommended for testing)
- `Temperature` - Temperature-based sampling
- `Mirostat` - Mirostat v2 sampling
- `TopP` - Nucleus sampling
- `TopK` - Top-K sampling
- `Typical` - Typical sampling
- `MinP` - Minimum probability sampling
- `TempExt` - Extended temperature sampling
- `ChainTempTopP` - Chained Temperature + Top-P
- `ChainTempTopK` - Chained Temperature + Top-K
- `ChainFull` - Full chain sampling (IBM recommended)

## 🎮 Usage Modes

### 🌐 Web Application (Default)
```bash
# Use the command detected by setup script
docker compose up llama-chat-app
# or
docker-compose up llama-chat-app
```
- Access at: http://localhost:3000
- Full React + Tauri interface
- Real-time chat with LLaMA

### 🖥️ CLI Version
```bash
docker compose --profile cli up llama-chat-cli
```
- Interactive command-line interface
- Direct model interaction
- Useful for testing and debugging

### 🔧 Development Mode
```bash
docker compose --profile dev up llama-chat-dev
```
- Live reload for development
- Frontend dev server on :5173
- Backend auto-recompilation

### 🏭 Production Mode
```bash
docker compose --profile production up
```
- Includes Nginx reverse proxy
- SSL termination (configure certificates)
- Optimized for production

## 📁 Directory Structure

```
llama_cpp_rs_chat/
├── Dockerfile              # Main production build
├── Dockerfile.dev          # Development build
├── docker-compose.yml      # Container orchestration
├── nginx.conf              # Nginx configuration
├── setup-docker.sh         # Setup script
├── assets/
│   └── conversations/      # Persistent chat logs
├── config/                 # Configuration files
├── models/                 # Model files (mounted)
└── ssl/                    # SSL certificates
```

## 🔍 Troubleshooting

### Model Not Found
```bash
# Check if model exists
ls -la ~/.lmstudio/models/lmstudio-community/granite-4.0-h-tiny-GGUF/

# Update model path in docker-compose.yml
```

### Port Already in Use
```bash
# Stop existing containers
docker-compose down

# Or use different ports
# Edit ports in docker-compose.yml: "3001:3000"
```

### Container Build Issues
```bash
# Clean rebuild
docker-compose down
docker system prune -a
docker-compose build --no-cache
```

### Performance Issues
```bash
# Allocate more memory to Docker
# Docker Desktop → Settings → Resources → Memory: 8GB+

# Check container resources
docker stats llama-chat-tauri-app
```

## 📊 Monitoring

### Health Checks
```bash
# Check application health
curl http://localhost:3000/health

# View container health
docker-compose ps
```

### Logs
```bash
# View application logs
docker-compose logs -f llama-chat-app

# View all services
docker-compose logs -f

# Follow specific service
docker logs -f llama-chat-tauri-app
```

### Resource Usage
```bash
# Monitor resource usage
docker stats

# View container details
docker inspect llama-chat-tauri-app
```

## 🛡️ Security Notes

- Models are mounted read-only by default
- Application runs as non-root user
- Conversations are persisted to host filesystem
- Nginx includes security headers in production mode

## 🔧 Customization

### Custom Model
1. Place your GGUF model in a directory
2. Update `docker-compose.yml`:
   ```yaml
   volumes:
     - /path/to/your/models:/app/models:ro
   environment:
     - MODEL_PATH=/app/models/your-model.gguf
   ```

### Custom Configuration
1. Create `config/custom.toml`
2. Mount it in `docker-compose.yml`:
   ```yaml
   volumes:
     - ./config:/app/config
   ```

### SSL Certificates
1. Generate certificates:
   ```bash
   mkdir ssl
   openssl req -x509 -nodes -days 365 -newkey rsa:2048 \
     -keyout ssl/key.pem -out ssl/cert.pem
   ```
2. Uncomment SSL lines in `nginx.conf`
3. Start with production profile

## 🆘 Getting Help

- Check logs: `docker-compose logs -f`
- Verify setup: `./setup-docker.sh`
- Test manually: `docker run -it llama-chat-app /bin/bash`
- Report issues with logs and configuration details

## 🎉 Success!

Once running, you should see:
- ✅ Container status: `docker-compose ps`
- ✅ Health check: `curl http://localhost:3000/health`
- ✅ Web interface: http://localhost:3000
- ✅ Chat working with real LLaMA model