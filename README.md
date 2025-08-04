# 🦙 Llama CPP Rust Chat

> **⚠️ Development Status**: This project is currently in active development. Features and APIs may change frequently.

An interactive chat application built with Rust that uses the [`llama-cpp-2`](https://crates.io/crates/llama-cpp-2) library to run GGUF-based LLM models locally.

## ✨ Features

- 🎨 **Colorized chat interface** with cyan user prompts and green assistant responses
- 📊 **Real-time context tracking** with color-coded usage indicators
- 🛡️ **Smart stop token filtering** prevents model artifacts from appearing in chat
- 💾 **Conversation persistence** - all chats are automatically saved as JSON
- 🧠 **Multi-format support** - Works with Mistral and Qwen model formats
- 🖥️ **Clean terminal interface** with automatic screen clearing

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

```bash
# Clone the repository
git clone <your-repo-url>
cd llama_cpp_rs_chat

# Build and run (debug mode for development)
cargo run

# Or build and run optimized version
cargo run --release
```

## 📁 Project Structure

```
llama_cpp_rs_chat/
├── src/
│   └── main.rs                 # Main chat application
├── assets/
│   ├── model_path.txt         # Stored model path (auto-generated)
│   └── conversations/         # Saved chat histories
├── error_log                  # Debug output (when enabled)
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

### Supported Model Formats
- **Mistral models**: Uses `<s>[INST]` prompt format
- **Qwen models**: Uses `<|im_start|>` prompt format
- **Auto-detection**: Format is detected from the model filename

### Model Requirements
- Models must be in **GGUF format**
- Recommended: 4-bit or 8-bit quantized models for better performance
- Popular sources: [Hugging Face](https://huggingface.co/models?library=gguf)

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
To enable detailed token generation logs, change `DEBUG_MODE` to `true` in `src/main.rs`:
```rust
const DEBUG_MODE: bool = true;  // Enable debug output
```
Then rebuild and run:
```bash
cargo run --release
```

## 🚀 Performance Tips

1. **Use quantized models** (Q4_K_M, Q5_K_M) for best speed/quality balance
2. **Adjust context size** based on your needs (smaller = faster)
3. **Enable GPU acceleration** if available (CUDA/Metal)
4. **Use SSD storage** for faster model loading

## 📝 License

This project is open source. Check the license file for details.

## 🤝 Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.
