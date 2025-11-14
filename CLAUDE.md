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

# Run Rust unit tests
cargo test
```

### Testing
```bash
# Run all E2E tests (requires backend running on port 8000)
npm test

# Run tests with Playwright UI
npm run test:ui

# Run tests in headed mode (see browser)
npm run test:headed

# Run tests with debugger
npm run test:debug

# Run tests in Docker environment
npm run test:docker
```

**Note**: E2E tests use Playwright and require the web backend to be running. Tests use `mock` feature flag for consistent test results without requiring actual model files.

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
- Mock implementation support via `mock` feature flag for testing (see Mock Implementation below)

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
The system supports four template formats:
- **ChatML**: `<|im_start|>role\ncontent<|im_end|>` (Qwen, OpenAI format)
- **Mistral**: `[INST] content [/INST]` (Mistral, Devstral format)
- **Llama3**: `<|start_header_id|>role<|end_header_id|>content<|eot_id|>` (Llama 3 format)
- **Gemma**: `<start_of_turn>role\ncontent<end_of_turn>\n` (Gemma 3 format, uses "model" instead of "assistant")

Auto-detected based on model metadata (tokenizer.chat_template in GGUF) or file name patterns. Template detection happens in `model_manager.rs::load_model()` and formatting in `chat_handler.rs::apply_model_chat_template()`.

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

### Tool Calling / Agentic System
The application implements a full agentic tool calling system that allows models to execute shell commands:

**Backend** (`src/web/utils.rs`, `src/web/command.rs`, `src/web/models.rs`):
- Tool definitions exposed via `get_available_tools_json()`
- `[AVAILABLE_TOOLS]...[/AVAILABLE_TOOLS]` injected into model prompt
- `/api/tools/execute` endpoint for command execution
- Supports bash/shell commands with quoted argument parsing
- **Backend translation layer**: Automatically translates file operations (read_file, write_file, list_directory) to bash commands for models that don't support them natively (e.g., Qwen models)

**Frontend** (`src/utils/toolParser.ts`, `src/hooks/useChat.ts`):
- Universal parser supporting Mistral, Llama3, and Qwen tool call formats
- Automatic agentic loop: model generates tool calls → executes → sends results back → model continues
- Safety limit: MAX_TOOL_ITERATIONS = 5
- Tool calls displayed with special UI styling in MessageBubble

**Supported formats**:
- Mistral: `[TOOL_CALLS]func_name[ARGS]{"arg": "value"}`
- Llama3: `<function=name>{"arg": "value"}</function>`
- Qwen: `<tool_call>{"name": "func", "arguments": {...}}</tool_call>`

**Model capability detection**:
The system automatically detects which tools each model supports based on chat template:
- Mistral/Devstral: Native file tools (read_file, write_file, list_directory)
- ChatML (Qwen): Bash only, file tools translated automatically
- Llama3: Native file tools
- Unknown: Default to bash translation (safe fallback)

See `TOOL_CALLING.md` and `BACKEND_TRANSLATION_COMPLETE.md` for detailed implementation documentation.

### Mock Implementation
For E2E testing without requiring actual model files, the project includes a mock implementation:

**How it works** (`src/chat_mock.rs`, `src/lib.rs`):
- Activated via `mock` feature flag in Cargo.toml
- Simulates model loading with fake metadata
- Returns canned responses for chat completions
- Allows testing UI, API, and tool calling without model files

**Usage**:
```bash
# Build with mock feature for testing
cargo build --features mock --bin llama_chat_web

# E2E tests automatically use mock when TEST_MODE=true
TEST_MODE=true cargo run --bin llama_chat_web
```

The mock implementation returns predictable responses making it ideal for automated testing.

### Recent Refactoring

**Major Refactoring (2025-01-13)**: Eliminated duplicate code in `main_web.rs`
- Removed 1,350 lines of duplicate structs and functions from `main_web.rs`
- File size reduced from 3,730 lines → 2,045 lines (45% reduction)
- All duplicates now imported from web modules via `use web::*`
- Removed duplicates: SamplerConfig, TokenData, all request/response structs, load_config(), add_to_model_history(), get_model_status(), calculate_optimal_gpu_layers(), load_model(), unload_model(), ConversationLogger, parse_conversation_to_messages(), get_available_tools_json(), apply_model_chat_template(), generate_llama_response(), handle_websocket(), handle_conversation_watch()
- Only unique code remains in main_web.rs: HTTP routing (handle_request_impl) and server initialization (main)

**Previous Refactoring (2025-01-08)**:
- Frontend: ModelConfigModal split from 1,205 lines → 9 files (~498 lines main)
- Backend: 1,712 lines extracted from main_web.rs → 8 modules
- Improved testability, maintainability, and modularity
- All functionality preserved with zero breaking changes

## Known Issues

### Tokenization Crash in llama-cpp-2
**Status**: Unresolved pre-existing library bug

**Symptoms**:
- Model stops generating mid-response when creating complex outputs (code, JSON, long text)
- Backend thread panics silently with no error message
- Conversation file shows in sidebar but doesn't exist on disk
- Affects ALL models (Gemma, Devstral, Qwen, etc.)

**Root Cause**:
Tokenization panic in llama-cpp-2 library at line 617 during token generation. The crash occurs:
1. Frontend optimistically creates conversation ID
2. Backend starts tokenization and generation
3. llama-cpp-2 panics during token processing
4. Thread crashes before conversation file is written
5. Frontend shows conversation ID but file never exists

**Impact**:
- JSON generation tests timeout (80s) in E2E suite
- Code generation requests (e.g., "write me a login page using svelte") fail
- Basic chat works but complex generations fail

**Potential Fixes** (not implemented):
1. Add error handling around tokenization calls
2. Reduce context size to avoid memory issues
3. Update llama-cpp-2 to newer version
4. Replace llama-cpp-2 with alternative library (llama.cpp direct bindings)

**Workaround**: Keep prompts simple and avoid requesting large code blocks or complex JSON outputs.

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
- **Tests**: `tests/e2e/` (Playwright E2E tests)
- **Documentation**: Various `.md` files including `TOOL_CALLING.md`, `REFACTORING_SUMMARY.md`

## HTTP API Endpoints

The web backend (`src/main_web.rs`) runs on **port 8000** by default and exposes the following REST API:

**Model Management**:
- `POST /api/load` - Load GGUF model with configuration
- `POST /api/unload` - Unload current model
- `GET /api/status` - Get model status and metadata
- `GET /api/conversations` - List all conversation files
- `GET /api/conversations/:filename` - Get conversation content

**Chat**:
- `POST /api/chat` - Send message and get response (non-streaming)
- `GET /api/chat/stream` - Server-sent events for streaming responses

**Tools**:
- `POST /api/tools/execute` - Execute tool (bash command)
- `GET /api/tools/available` - Get available tools JSON schema

**WebSocket**:
- `/ws/chat` - WebSocket for real-time chat streaming
- `/ws/conversations/:filename` - WebSocket for conversation file updates

**Static**:
- `GET /` - Serve frontend application
- `GET /assets/*` - Serve static assets
