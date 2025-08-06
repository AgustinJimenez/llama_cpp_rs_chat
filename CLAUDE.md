# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Rust-based AI command-line assistant that provides LLMs with full command-line access to the local system. The application uses llama-cpp-2 for model inference and allows AI assistants to execute system commands through a special `!CMD!` syntax.

## Architecture

### Core Components

- **main.rs**: Entry point with chat loop, VRAM detection, GPU layer configuration, and command execution integration
- **llm_backend.rs**: Trait definition for LLM backends with common interfaces
- **llamacpp_backend.rs**: llama-cpp-2 implementation with context management and token generation
- **ai_operations.rs**: Command execution trait definitions and data structures  
- **command_executor.rs**: System command execution with cross-platform support

### Key Features

- **Command Execution**: AI can execute system commands via `!CMD!command!CMD!` tags
- **GPU Acceleration**: Smart GPU layer offloading with automatic VRAM detection
- **Context Management**: Dynamic context usage tracking with visual indicators
- **Prompt Formats**: Auto-detection of Mistral vs Qwen model formats
- **Conversation Persistence**: Automatic saving to `assets/conversations/`

## Build & Run Commands

### Quick Start
```bash
# Windows
run.bat

# Unix/Linux/macOS  
./run.sh
```

### Direct Cargo Commands
```bash
# Build and run (normal mode)
cargo run

# Build only
cargo build --release

# Run with clean output (suppress llama.cpp logs)
cargo run 2>nul          # Windows
cargo run 2>/dev/null    # Unix/Linux/macOS
```

### Environment Configuration

The application uses `.env` file for configuration:
```env
# Core modes
RUN_MODE=normal           # normal, debug_low, debug_high, build
PAUSE_ON_EXIT=false       # Windows pause behavior

# Logging control
LLAMA_LOG_LEVEL=3         # 0=debug, 1=info, 2=warn, 3=error, 4=none
LLAMA_DEBUG=false         # Enable app debug output
```

### Run Mode Options
- `normal`: Clean output, logs suppressed
- `debug_low`: App debug messages only  
- `debug_high`: All logs including llama.cpp internals
- `build`: Build-only mode for CI/CD

## Development Workflow

### Model Setup
1. First run prompts for GGUF model file path
2. Path is saved to `assets/model_path.txt` for reuse
3. Context size configuration (default: 8192 tokens)
4. GPU layer configuration with smart recommendations

### Command Execution System
- AI responses containing `!CMD!` trigger command execution
- Commands run through `SystemCommandExecutor` with cross-platform support
- Output is injected back into conversation context
- Supports command chaining with `&&`, `||`, `|` operators

### Context Management
- Dynamic token limits based on available context space
- Visual usage indicators (green < 70%, yellow 70-89%, red ≥ 90%)
- Safety buffers to prevent context overflow
- Automatic conversation truncation warnings

## Key Implementation Details

### GPU Configuration
The application provides intelligent GPU offloading recommendations:
- Automatic VRAM detection via nvidia-smi, wmic, PowerShell, WMI
- Smart layer calculation based on model size and available VRAM
- Manual fallback if auto-detection fails
- Performance optimization for different hardware configurations

### Prompt Format Detection
```rust
// Auto-detects from model filename
if name.contains("qwen") {
    PromptFormat::Qwen      // <|im_start|> format
} else {
    PromptFormat::Mistral   // [INST] format  
}
```

### Command Parsing
Commands are extracted from AI responses using pattern matching:
- Start: `!CMD!`
- Content: Everything until next `!CMD!` or end of line
- Supports full command lines with arguments and operators

## Testing & Debugging

### Debug Modes
Set `LLAMA_DEBUG=true` for detailed execution logs:
- Token generation details
- Context usage tracking  
- Command execution flow
- Batch processing information

### Performance Monitoring
- Token generation speed (tokens/sec)
- Context usage percentages
- Command execution timing
- Memory usage warnings

## Common Issues

### VRAM Detection
If GPU detection fails, the app prompts for manual VRAM entry. Common fixes:
- Ensure nvidia-smi is in PATH (NVIDIA GPUs)
- Check WMI permissions (Windows)
- Verify PowerShell execution policy

### Context Overflow
When context usage hits 85%+:
- Start new conversation
- Reduce context size setting
- Use shorter system prompts

### Build Dependencies
Requires:
- CMake (3.12+)
- C++ compiler (MSVC, GCC, Clang)  
- Rust toolchain (latest stable)