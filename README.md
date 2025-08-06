# 🦙 AI Command-Line Assistant

> **🚀 Core Vision**: An AI assistant with **FULL COMMAND-LINE ACCESS** that can manage your system, create projects dynamically, and browse the web.

A powerful interactive AI chat application built with Rust that gives the LLM complete access to:
- **🖥️ Command Line**: Execute any system command (mkdir, git, npm, curl, etc.)
- **📁 File System**: Create, read, edit files and directories dynamically  
- **🌐 Web Access**: Fetch information, APIs, documentation from the internet
- **🛠️ Project Creation**: Build entire projects from scratch using command-line tools

## ✨ Key Capabilities

### 🖥️ **Full Command-Line Access**
- Execute any system command through `!CMD!` syntax
- Chain commands with `&&`, `||`, `|` operators  
- File operations: `mkdir`, `echo`, `cp`, `mv`, `rm`
- Development tools: `git`, `npm`, `cargo`, `pip`, `docker`
- System tools: `ps`, `netstat`, `curl`, `wget`

### 🚀 **Dynamic Project Creation**
Instead of static templates, the AI creates projects intelligently:
```
User: "Create a Python FastAPI project with Docker"
AI: !CMD!mkdir my-api && cd my-api
    !CMD!echo "from fastapi import FastAPI..." > main.py
    !CMD!curl -s https://fastapi.tiangolo.com/tutorial/ | findstr requirements
    !CMD!echo "FROM python:3.11..." > Dockerfile
    !CMD!git init && git add .
```

### 🌐 **Web Information Access**  
- Fetch latest documentation: `curl -s https://docs.python.org/...`
- Check API endpoints: `curl -H "Accept: application/json" https://api.github.com/...`
- Download resources: `wget https://example.com/file.zip`

### 📁 **Intelligent File Management**
- Read any file: `findstr . filename.txt` / `cat filename.txt`
- Create complex directory structures dynamically
- Generate configuration files based on current best practices
- Backup and restore operations

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
# Edit .env to set RUN_MODE=normal for clean output

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
# Run mode: normal, debug_low, debug_high, build
RUN_MODE=normal               # normal = clean output (no logs)

# Script behavior
PAUSE_ON_EXIT=false           # Pause before exit (Windows)
```

### Run Modes

| Mode | Description | Best For |
|------|-------------|----------|
| `normal` | Clean output (no logs) | Clean demos, production |
| `debug_low` | App debug messages only | Development, app troubleshooting |
| `debug_high` | All logs including LLaMA.cpp | Deep troubleshooting, debugging |
| `build` | Build only (no execution) | CI/CD, compilation testing |

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
RUN_MODE=debug_high
```
Then run: `./run.sh` or `run.bat`

**Option 2: Temporary Override**
```bash
# Unix/Linux/macOS
RUN_MODE=debug_high ./run.sh

# Windows
set RUN_MODE=debug_high && run.bat
```

## 🚀 Performance Tips

1. **Use quantized models** (Q4_K_M, Q5_K_M) for best speed/quality balance
2. **Adjust context size** based on your needs (smaller = faster)
3. **Enable GPU acceleration** if available (CUDA/Metal)
4. **Use SSD storage** for faster model loading
5. **Run in normal mode** (`RUN_MODE=normal`) for best performance
6. **Choose optimal backend** - LLaMA.cpp for speed, Candle for pure Rust

## 📚 Documentation

- **[SCRIPTS.md](SCRIPTS.md)** - Detailed script usage and configuration
- **[.env.example](.env.example)** - All available configuration options

## 🔧 Advanced Usage

### Multiple Configurations
```bash
# Development config
cp .env.example .env.dev
echo "RUN_MODE=debug_low" >> .env.dev

# Production config  
cp .env.example .env.prod
echo "RUN_MODE=normal" >> .env.prod

# Use specific config
cp .env.dev .env && ./run.sh
```

### Override Settings
```bash
# Temporary override
RUN_MODE=debug_high ./run.sh

# Windows
set RUN_MODE=debug_high && run.bat
```

## 📝 License

This project is open source. Check the license file for details.

## 🤝 Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.
