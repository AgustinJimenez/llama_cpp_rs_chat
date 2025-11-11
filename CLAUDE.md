# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

A modern AI chat application built with Tauri, Rust, and llama-cpp-2. Features both native desktop app and web application with integrated shell command execution. Uses local LLM inference with GGUF models and supports CUDA GPU acceleration.

## Build & Development Commands

### Web Development (Primary)
```bash
# Start web app (Vite frontend + Rust backend)
npm run dev

# Build frontend for production
npm run build
```

### Desktop Development
```bash
# Run Tauri desktop app with hot reload
cargo tauri dev

# Build production desktop app
cargo tauri build
```

### Rust Builds
```bash
# Build library
cargo build --lib

# Build web backend server
cargo build --bin llama_chat_web

# Build CLI test binary (currently commented out in Cargo.toml)
# cargo build --bin test

# Run tests
cargo test
```

### CMake Configuration (Windows)
This project requires CMake for building llama-cpp-2 with CUDA. On Windows, if CMake is not in PATH:
```bash
# Set CMAKE environment variable to cmake.exe path
# Example paths that have been used:
# C:\Program Files\CMake\bin\cmake.exe
# E:\repo\llama_cpp_rs_test\cmake\windows\bin\cmake.exe
```

## Architecture

### Dual-Mode Application
The project supports two execution modes:
1. **Desktop App**: Tauri-based native application (`src/main.rs`)
2. **Web App**: Standalone web server with WebSocket support (`src/main_web.rs`)

### Backend Architecture (`src/`)

**Core Chat Engine** (`chat.rs`)
- LLaMA model loading and inference
- 11 different sampling strategies (Greedy, Temperature, Mirostat, TopP, TopK, Typical, MinP, TempExt, ChainTempTopP, ChainTempTopK, ChainFull)
- Token generation with streaming support
- Model context management

**Tauri Interface** (`lib.rs`, `main.rs`)
- Tauri commands for model control
- State management for conversations and chat engine
- Mock implementation support via `mock` feature flag for testing

**Web Server Modules** (`src/web/`)
Recent refactoring split `main_web.rs` into focused modules:
- `models.rs` - Data structures (SamplerConfig, ChatRequest, ChatMessage, ModelStatus, etc.)
- `config.rs` - Configuration loading and model history management
- `command.rs` - Shell command parsing and execution with quote handling
- `conversation.rs` - ConversationLogger for file-based chat logging with timestamps
- `model_manager.rs` - Model loading/unloading, GPU layer calculation, status management
- `chat_handler.rs` - Chat template application (ChatML, Mistral, Llama3) and LLaMA response generation
- `websocket.rs` - WebSocket handlers for real-time chat streaming and conversation file watching
- `utils.rs` - Utility functions including tool definitions for model context

**Main Web Server** (`main_web.rs`)
- HTTP route handling for all endpoints
- Server initialization and setup
- Integration of all web modules

### Frontend Architecture (`src/components/`)

**Model Configuration** (`model-config/`)
Recently refactored into modular components:
- `index.tsx` - Main modal orchestrator (498 lines, down from 1,205)
- `constants.ts` - Sampler types and preset configurations
- `ModelFileInput.tsx` - File path input with validation and history
- `ModelMetadataDisplay.tsx` - Model metadata viewer (architecture, parameters, etc.)
- `ContextSizeSection.tsx` - Context window size controls
- `SystemPromptSection.tsx` - System prompt configuration
- `GpuLayersSection.tsx` - GPU offloading controls
- `SamplingParametersSection.tsx` - Temperature, top-p, top-k, etc.
- `PresetsSection.tsx` - Quick preset buttons

**Other Components**
- `ModelSelector.tsx` - Model selection interface
- `SettingsModal.tsx` - Application settings

### Data Flow

**Model Loading**:
1. User selects GGUF model file
2. `model_manager.rs::load_model()` reads metadata with gguf-llms
3. Calculates optimal GPU layers based on VRAM
4. Creates LlamaModel with LlamaContextParams
5. Stores model in global state (Arc<Mutex<Option<LlamaModel>>>)

**Chat Generation**:
1. User sends message via HTTP/WebSocket
2. `chat_handler.rs::apply_model_chat_template()` formats messages
3. Tokenizes prompt using model
4. `chat_handler.rs::generate_llama_response()` streams tokens
5. Executes shell commands if requested (via `command.rs`)
6. Logs conversation to file (`conversation.rs::ConversationLogger`)
7. Returns response via streaming or complete message

**Conversation Persistence**:
- All chats saved to `assets/conversations/chat_YYYY-MM-DD-HH-mm-ss-SSS.txt`
- Format includes timestamped user/assistant messages and command outputs
- Can resume from existing conversation files

## Key Configuration

### Model Path
Default model path is in `chat.rs` but typically overridden at runtime:
```rust
/Users/agus/.lmstudio/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf
```

### Sampler Configuration
Located in `src/web/models.rs::SamplerConfig`:
- IBM-recommended defaults: temp=0.7, top_p=0.95, top_k=20
- Configurable via web UI or config file
- Different samplers have varying model compatibility

### GPU Acceleration
- Built with CUDA support via `llama-cpp-2` features
- Automatic GPU layer calculation in `model_manager.rs::calculate_optimal_gpu_layers()`
- Estimates based on model size, VRAM, and context size

## Important Notes

### Chat Templates
The system supports three template formats:
- **ChatML**: `<|im_start|>role\ncontent<|im_end|>`
- **Mistral**: `[INST] content [/INST]`
- **Llama3**: `<|start_header_id|>role<|end_header_id|>content<|eot_id|>`

Auto-detected based on model metadata or file name patterns.

### Command Execution
The AI can execute shell commands. Command parsing supports:
- Quoted arguments with spaces: `command "arg with spaces"`
- Single and double quotes
- Escaped characters
- Security: Commands run with user permissions (no sandboxing)

### WebSocket Support
Two WebSocket endpoints:
1. `/ws/chat` - Real-time token streaming during generation
2. `/ws/conversations/:filename` - File change notifications for conversation updates

### Recent Refactoring
The codebase underwent major refactoring (2025-01-08):
- Frontend: ModelConfigModal split from 1,205 lines → 9 files (~498 lines main)
- Backend: 1,712 lines extracted from main_web.rs → 8 modules
- Improved testability, maintainability, and modularity
- All functionality preserved with zero breaking changes

## Development Workflow

1. **Making Changes**: Prefer editing existing files over creating new ones
2. **Testing Web Changes**: `npm run dev` provides hot reload for frontend
3. **Testing Desktop Changes**: `cargo tauri dev` rebuilds Rust code automatically
4. **Model Testing**: Use CLI mode or web interface with local GGUF models
5. **Debugging**: Use `RUST_LOG=debug` for detailed logging

## Dependencies

- **Rust**: 1.70+ (uses 2021 edition)
- **Node.js**: 16+ for frontend tooling
- **CMake**: Required for llama-cpp-2 compilation
- **CUDA** (optional): For GPU acceleration
- **Tauri CLI**: For desktop app builds

## File Locations

- **Frontend**: `index.html`, `main.js`, `src/components/`
- **Backend Core**: `src/lib.rs`, `src/main.rs`, `src/chat.rs`
- **Web Server**: `src/main_web.rs`, `src/web/`
- **Conversations**: `assets/conversations/`
- **Config**: Root directory (config.json for sampler settings)
- **Tauri Config**: `tauri.conf.json`
