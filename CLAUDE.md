# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Quick Start for Development

**Default development mode (with hot reload):**
```bash
npm run dev
```
- Access app at **http://localhost:4000** (Vite dev server with hot reload)
- Backend API runs on http://localhost:8000
- Frontend changes reload automatically

## Project Overview

A modern AI chat application built with Tauri, Rust, and llama-cpp-2. Features both native desktop app and web application with integrated shell command execution. Uses local LLM inference with GGUF models and supports CUDA GPU acceleration.

## Recent Major Improvements

### 2025-12-16 - Code Cleanup & Development Environment
- ✅ **Removed old file-based conversation logging** (`src/web/conversation.rs`)
  - SQLite-based logging (`src/web/database/conversation.rs`) is now the only implementation
  - All conversations stored in `assets/llama_chat.db`
- ✅ **Fixed all compiler warnings** (31 warnings → 0 code warnings)
  - Marked unused helper functions with `#[allow(dead_code)]`
  - Note: `vram_calculator.rs` IS integrated - used by `model_manager.rs` for GPU layer calculation
  - Removed unused imports and cleaned up module declarations
- ✅ **Documented default development mode** in CLAUDE.md
  - `npm run dev` starts both Vite (port 4000) and Rust backend (port 8000)
  - Frontend hot reload via Vite dev server
  - Always use http://localhost:4000 for development

### 2025-01-15 (Continued) - Markdown & UI Improvements
- ✅ **Fixed conversation loading crash** - Removed `rehypeHighlight` dependency error
- ✅ **Enhanced markdown rendering** with `react-syntax-highlighter`
  - Dracula syntax highlighting theme for code blocks
  - Complete markdown component support (headings, lists, bold, italic, blockquotes)
  - Removed extra padding/background from code blocks for cleaner UI
- ✅ **Added thinking model support** (Qwen3-8B and similar)
  - Extracts and displays `<think>` tags separately in collapsible sections
  - Content without thinking tags renders as clean markdown
  - Thinking process shown with 💭 icon in blue highlighted box
- ✅ **Added conversation loading E2E tests**
  - `tests/e2e/conversation-loading.test.ts` - Full conversation workflow tests
  - `tests/e2e/conversation-loading-simple.test.ts` - Basic loading verification

### 2025-01-15 - Code Quality & Security
- ✅ **Fixed critical command injection vulnerability** in `src/web/command.rs` with whitelist-based validation
- ✅ **Eliminated duplicate function definitions** in `src/main_web.rs` (compilation error fix)
- ✅ **Replaced 26 println! with proper file-based logging** (`logs/llama_chat.log`)
- ✅ **Extracted 14 magic numbers to named constants** for better maintainability
- ✅ **Improved error handling** - replaced `.unwrap()` with `.expect()` for better debugging

### Architecture Refactoring
- ✅ **Split main_web.rs**: 2,083 lines → 219 lines (89% reduction)
  - Created 8 focused route handler modules in `src/web/routes/`
  - Each route is now in its own file with clear responsibilities
- ✅ **Split chat_handler.rs**: 647 lines → 4 lines (99% reduction)
  - `src/web/chat/templates.rs` - Chat template formatting (244 lines)
  - `src/web/chat/generation.rs` - Token generation logic (413 lines)
  - Backward compatible via re-exports

### Testing
- ✅ **Added 42 comprehensive unit tests** (100% passing)
  - 23 tests for command parsing (security-critical)
  - 16 tests for VRAM/GPU calculations
  - 3 tests for template parsing
- ✅ **All 35 E2E tests passing** with 6 different models
- ✅ **Added conversation loading E2E tests**
- ✅ **Added file creation E2E tests** - Verifies models can create files using write_file tool or bash
- ✅ **Zero test duplicates**

## Build & Development Commands

### Web Development (Primary) - DEFAULT MODE

**IMPORTANT: Always use this for development:**
```bash
npm run dev
```

This command starts BOTH servers concurrently:
1. **Vite dev server** on **http://localhost:4000** (with hot reload)
2. **Rust backend** on **http://localhost:8000**

**How it works:**
- Vite proxies `/api` and `/ws` requests to the Rust backend (configured in `vite.config.ts`)
- Frontend changes hot reload automatically
- **Always access the app at http://localhost:4000 during development**
- Port 8000 serves the backend API only (no frontend, no hot reload)

**Alternative commands:**
```bash
# Build frontend for production
npm run build

# Manually start Vite only (if backend already running)
npx vite --host --port 4000

# Manually start backend only (if Vite already running)
cargo run --bin llama_chat_web
```

**Port Summary:**
- **Port 4000**: Vite dev server - **USE THIS FOR DEVELOPMENT** ✅
- **Port 8000**: Rust backend API only (no frontend)

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

### Markdown Rendering & Thinking Models

**Markdown Support** (`src/components/MessageBubble.tsx`):
- Uses `react-syntax-highlighter` with Dracula theme for code blocks
- Full ReactMarkdown component set: headings (h1-h3), lists (ul/ol), bold, italic, blockquotes, code
- Syntax highlighting for 180+ languages
- Clean rendering with no extra padding or background on code blocks
- Separate rendering for user and assistant messages

**Thinking/Reasoning Model Support**:
The UI automatically detects and handles thinking models (like Qwen3-8B) that output internal reasoning:
- Extracts `<think>...</think>` tags from model responses
- Displays thinking process in collapsible blue-highlighted section (💭 Thinking Process)
- Main response shown separately without thinking tags
- Uses HTML `<details>` element for expandable/collapsible UI
- Thinking section collapsed by default to reduce visual clutter

**Implementation details**:
```typescript
// Extract thinking content
const thinkingContent = message.content.match(/<think>([\s\S]*?)<\/think>/);

// Remove thinking tags from main content
const contentWithoutThinking = cleanContent.replace(/<think>[\s\S]*?<\/think>/g, '').trim();
```

**Supported thinking models**:
- Qwen3-8B (and larger variants)
- Any model that outputs `<think>` tags for chain-of-thought reasoning

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

## Code Quality Best Practices

### Security
- ✅ **Command Whitelist**: All shell commands validated against `ALLOWED_COMMANDS` in `command.rs`
- ✅ **No Arbitrary Execution**: Commands like `rm -rf`, `shutdown`, `format` are blocked
- ✅ **Path Validation**: Filesystem-wide searches (`find /`, `find /usr`) are restricted

### Constants & Configuration
- ✅ **Named Constants**: All magic numbers extracted to constants (see `model_manager.rs`)
  - `DEFAULT_VRAM_GB = 22.0` - VRAM fallback
  - `VRAM_SAFETY_MARGIN_GB = 2.0` - System overhead
  - `KV_CACHE_MULTIPLIER = 4.0` - Cache calculation
  - Model size thresholds: `SMALL_MODEL_GB`, `MEDIUM_MODEL_GB`, etc.
- ✅ **Logging**: Use `log_debug!`, `log_info!`, `log_warn!` instead of `println!`
- ✅ **Error Handling**: Use `.expect("descriptive message")` instead of `.unwrap()`

### Testing Strategy
- **Unit Tests**: Test pure functions (parsing, calculations, validation)
- **E2E Tests**: Test full user flows with real models
- **Test Coverage**: Focus on security-critical and calculation-heavy code
- **Integration Tests**: Already covered by E2E, don't duplicate in unit tests

### Module Organization
- Keep route handlers in `src/web/routes/` (one file per route group)
- Keep business logic separate from HTTP routing
- Use re-exports for backward compatibility when refactoring
- Prefer small, focused modules (~200-400 lines) over monoliths

## Debugging

### Log Files
The application uses per-conversation logging stored in `logs/conversations/`:
- Each conversation gets its own log file: `logs/conversations/chat_YYYY-MM-DD-HH-mm-ss-SSS.log`
- Also a general `system.log` for system-wide events

### How to Debug Issues
1. **Clear logs before testing**: `rm -f logs/conversations/*.log`
2. **Run the test scenario** in the web UI at http://localhost:4000
3. **Check the latest log**: `ls -lt logs/conversations/ | head -3` then `cat logs/conversations/<latest>.log`

### Key Log Messages to Look For
- `=== TEMPLATE DEBUG ===` - Shows which chat template is being used
- `=== FINAL PROMPT BEING SENT TO MODEL ===` - The exact prompt sent to LLM
- `🔧 SYSTEM.EXEC detected:` - Command was detected and will be executed
- `📤 Command output length:` - Command was executed successfully
- `✅ Command executed, output injected` - Output was injected into context
- `Stopping generation - EOS token detected` - Model finished generating
- `hit_stop_condition: true/false` - Whether generation stopped due to stop tokens

### Common Debug Scenarios
- **Command not executed**: Check if `🔧 SYSTEM.EXEC detected` appears. If not, the regex didn't match the model's output format
- **Model stops after command**: Check if `hit_stop_condition` was reset to `false` after command execution
- **Wrong prompt format**: Check `=== FINAL PROMPT ===` to verify the system prompt is correct

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
5. **Debugging**: Check `logs/llama_chat.log` for detailed backend logs with timestamps
6. **Running Tests**:
   - Unit tests: `cargo test --bin llama_chat_web` (42 tests)
   - E2E tests: `npm run test` (requires backend running on port 8000)
   - Single browser: `npx playwright test --project=chromium`

## Dependencies

### Backend (Rust)
- **Rust**: 1.70+ (uses 2021 edition)
- **CMake**: Required for llama-cpp-2 compilation
- **CUDA** (optional): For GPU acceleration
- **Tauri CLI**: For desktop app builds
- `llama-cpp-2`: LLM inference engine
- `hyper`: HTTP server
- `tokio`: Async runtime
- `serde_json`: JSON serialization
- `chrono`: Timestamps for logging
- `lazy_static`: Global logger instance

### Frontend (Node.js)
- **Node.js**: 16+ for tooling
- `react` + `react-dom`: UI framework
- `vite`: Build tool and dev server
- `react-markdown`: Markdown rendering
- `react-syntax-highlighter`: Code syntax highlighting with Dracula theme
- `remark-gfm`: GitHub Flavored Markdown support
- `@tailwindcss/typography`: Typography plugin for markdown prose
- `lucide-react`: Icon library
- `react-hot-toast`: Toast notifications
- `playwright`: E2E testing framework

## File Locations

- **Frontend**: `index.html`, `main.js`, `src/components/`
- **Backend Core**: `src/lib.rs`, `src/main.rs`, `src/chat.rs`
- **Web Server**: `src/main_web.rs` (219 lines - just server setup + routing)
- **Web Routes**: `src/web/routes/` (8 modular route handlers)
  - `health.rs`, `chat.rs`, `config.rs`, `conversation.rs`
  - `model.rs`, `files.rs`, `tools.rs`, `static_files.rs`
- **Chat Logic**: `src/web/chat/` (split from chat_handler.rs)
  - `templates.rs` - Chat template formatting (ChatML, Mistral, Llama3, Gemma)
  - `generation.rs` - Token generation with sampling
- **Other Web Modules**: `src/web/` (models, config, command, conversation, etc.)
- **Conversations**: `assets/conversations/`
- **Logs**: `logs/llama_chat.log` (file-based logging with timestamps)
- **Config**: Root directory (config.json for sampler settings)
- **Tauri Config**: `tauri.conf.json`
- **Tests**:
  - `tests/e2e/` - Playwright E2E tests (32 tests, 6 models)
  - Unit tests embedded in module files (42 tests total)
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
