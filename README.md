# 🦙 Llama CPP Rust Chat

> **⚠️ Development Status**: This project is currently in active development. Features and APIs may change frequently.

A high-performance interactive chat application built with Rust that supports multiple LLM backends:
- **LLaMA.cpp** via [`llama-cpp-2`](https://crates.io/crates/llama-cpp-2) for CPU/GPU inference  
- **Candle** ML framework for native Rust inference
- Unified interface supporting GGUF models locally

## ✨ Features

- 🎨 **Colorized chat interface** with cyan user prompts and green assistant responses
- 📊 **Real-time context tracking** with color-coded usage indicators
- 🛡️ **Smart stop token filtering** prevents model artifacts from appearing in chat
- 💾 **Conversation persistence** - all chats are automatically saved as JSON
- 🧠 **Multi-format support** - Works with Mistral and Qwen model formats
- 🖥️ **Clean terminal interface** with automatic screen clearing
- 🔇 **Configurable logging** - Silent, normal, or debug modes via environment variables
- ⚙️ **Multiple backends** - Choose between LLaMA.cpp and Candle ML frameworks
- 🚀 **Easy execution** - Cross-platform run scripts with .env configuration

## 🛠️ Prerequisites

### Required Dependencies

1. **Rust** (latest stable)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **CMake** (version 3.12 or higher)
   - **macOS**: `brew install cmake`
   - **Ubuntu/Debian**: `sudo apt install cmake`
   - **Windows**: Download from [cmake.org](https://cmake.org/download/)
   - **Arch Linux**: `sudo pacman -S cmake`

3. **C++ Compiler**
   - **macOS**: Xcode Command Line Tools (`xcode-select --install`)
   - **Ubuntu/Debian**: `sudo apt install build-essential`
   - **Windows**: Visual Studio Build Tools or MinGW-w64

### Optional (for better performance)
- **CUDA** - For NVIDIA GPU acceleration
- **Metal** - For Apple Silicon GPU acceleration (macOS)

## 🚀 Quick Start

### Option 1: Easy Run Scripts (Recommended)

```bash
# Clone the repository
git clone <your-repo-url>
cd llama_cpp_rs_chat

# Set up configuration
cp .env.example .env
# Edit .env to set RUN_MODE=silent for clean output

# Run with scripts (handles all configuration automatically)
./run.sh        # Unix/Linux/macOS
run.bat         # Windows
```

### Option 2: Direct Cargo

```bash
# Build and run (with verbose logs)
cargo run

# Or build and run optimized version
cargo run --release

# Silent run (suppress llama.cpp logs)
cargo run 2>/dev/null          # Unix/Linux/macOS
cargo run 2>nul               # Windows
```

## 📁 Project Structure

```
llama_cpp_rs_chat/
├── src/
│   ├── main.rs                # Main chat application
│   ├── llm_backend.rs         # Backend trait definition
│   ├── llamacpp_backend.rs    # LLaMA.cpp implementation  
│   └── candle_backend.rs      # Candle ML implementation
├── assets/
│   ├── model_path.txt         # Stored model path (auto-generated)
│   └── conversations/         # Saved chat histories
├── run.sh                     # Unix/Linux/macOS run script
├── run.bat                    # Windows run script
├── .env.example               # Environment configuration template
├── SCRIPTS.md                 # Script documentation
├── Cargo.toml                 # Rust dependencies
└── README.md
```

## 🎮 Usage

1. **First run**: You'll be prompted to enter the path to your GGUF model file
2. **Set context size**: Choose your context window (default: 8192 tokens)
3. **Start chatting**: Type your messages and see responses in real-time
4. **Monitor usage**: Context usage is displayed after each response
5. **Exit**: Type `exit` to end the session

### Context Usage Colors
- 🟢 **Green**: < 70% used (plenty of space)
- 🟡 **Yellow**: 70-89% used (getting full) 
- 🔴 **Red**: ≥ 90% used (almost full - consider starting new chat)

## 🔧 Configuration

### Environment Configuration (.env)

Copy `.env.example` to `.env` and customize:

```env
# Run mode: normal, silent, debug
RUN_MODE=silent                # silent = no llama.cpp logs (cleanest)

# Logging levels  
LLAMA_LOG_LEVEL=3             # 0=debug, 1=info, 2=warn, 3=error, 4=none
LLAMA_DEBUG=false             # Enable Rust app debug output

# Script behavior
PAUSE_ON_EXIT=false           # Pause before exit (Windows)
```

### Run Modes

| Mode | Description | Best For |
|------|-------------|----------|
| `silent` | No stderr output | Clean demos, production |
| `normal` | Balanced logging | Development |
| `debug` | All logs visible | Troubleshooting |

### Supported Model Formats
- **Mistral models**: Uses `<s>[INST]` prompt format
- **Qwen models**: Uses `<|im_start|>` prompt format
- **Auto-detection**: Format is detected from the model filename

### Model Requirements
- Models must be in **GGUF format**
- Recommended: 4-bit or 8-bit quantized models for better performance
- Popular sources: [Hugging Face](https://huggingface.co/models?library=gguf)

### Backend Selection
- **LLaMA.cpp**: Production-ready, GPU accelerated
- **Candle**: Pure Rust implementation (experimental)

## 🗂️ Conversation Management

All conversations are automatically saved to `assets/conversations/` as JSON files:
```json
[
  {
    "role": "system",
    "content": "You are a helpful assistant."
  },
  {
    "role": "user", 
    "content": "Hello!"
  },
  {
    "role": "assistant",
    "content": "Hi there! How can I help you today?"
  }
]
```

## 🐛 Troubleshooting

### Common Issues

**"CMake not found"**
```bash
# Install CMake first, then try again
# macOS: brew install cmake
# Ubuntu: sudo apt install cmake
```

**"Provided path is not a valid .gguf file"**
- Ensure your model file has `.gguf` extension
- Check that the file path is correct and accessible

**"Context usage at 100%"**
- Start a new chat session
- Use a smaller context size
- Try a more efficient model

### Debug Mode

**Option 1: Using .env (Recommended)**
```env
RUN_MODE=debug
```
Then run: `./run.sh` or `run.bat`

**Option 2: Environment Variable**
```bash
LLAMA_DEBUG=true LLAMA_LOG_LEVEL=0 cargo run
```

**Option 3: Direct Code Change**
Change `DEBUG_MODE` to `true` in `src/llamacpp_backend.rs` and rebuild.

## 🚀 Performance Tips

1. **Use quantized models** (Q4_K_M, Q5_K_M) for best speed/quality balance
2. **Adjust context size** based on your needs (smaller = faster)
3. **Enable GPU acceleration** if available (CUDA/Metal)
4. **Use SSD storage** for faster model loading
5. **Run in silent mode** (`RUN_MODE=silent`) for best performance
6. **Choose optimal backend** - LLaMA.cpp for speed, Candle for pure Rust

## 📚 Documentation

- **[SCRIPTS.md](SCRIPTS.md)** - Detailed script usage and configuration
- **[.env.example](.env.example)** - All available configuration options

## 🔧 Advanced Usage

### Multiple Configurations
```bash
# Development config
cp .env.example .env.dev
echo "RUN_MODE=debug" >> .env.dev

# Production config  
cp .env.example .env.prod
echo "RUN_MODE=silent" >> .env.prod

# Use specific config
cp .env.dev .env && ./run.sh
```

### Override Settings
```bash
# Temporary override
RUN_MODE=debug ./run.sh

# Windows
set RUN_MODE=debug && run.bat
```

## 📝 License

This project is open source. Check the license file for details.

## 🤝 Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.
