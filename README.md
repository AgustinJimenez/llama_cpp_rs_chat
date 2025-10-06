# 🦙 LLaMA Chat - Desktop AI Assistant

A modern desktop AI chat application built with Tauri, Rust, and llama-cpp-2. Features a beautiful UI with integrated shell command execution capabilities.

![LLaMA Chat Screenshot](https://img.shields.io/badge/Platform-Desktop-blue) ![Rust](https://img.shields.io/badge/Language-Rust-orange) ![Tauri](https://img.shields.io/badge/Framework-Tauri-green)

## ✨ Features

- 🖥️ **Native Desktop Application** - Built with Tauri for optimal performance
- 🧠 **Local LLM Inference** - Powered by llama-cpp-2 with multiple model support
- 🎨 **Modern UI** - Beautiful gradient design with real-time chat interface
- ⚙️ **Advanced Sampling** - 11 different sampling strategies including IBM-recommended settings
- 🔧 **Command Execution** - Integrated shell command capabilities for AI assistance
- 💾 **Conversation Logging** - Automatic chat history with timestamped files
- 🌐 **Cross-Platform** - Works on macOS, Windows, and Linux

## 🚀 Quick Start

### Option 1: Desktop Application (Recommended)

1. **Install Dependencies:**
   ```bash
   # Install Node.js dependencies
   npm install
   
   # Install Tauri CLI (if not already installed)
   cargo install tauri-cli
   ```

2. **Run the Desktop App:**
   ```bash
   # Development mode with hot reload
   cargo tauri dev
   
   # Or build for production
   cargo tauri build
   ```

### Option 2: Command Line Interface

For testing or headless usage:

```bash
# Run the CLI version (original test interface)
cargo run --bin test

# Or run the default Tauri binary (if not using desktop UI)
cargo run
```

## 🔧 Configuration

### Sampler Settings

The application supports multiple sampling strategies with IBM-recommended defaults:

- **Temperature**: 0.7 (IBM recommended)
- **Top P**: 0.95 (IBM recommended) 
- **Top K**: 20 (IBM recommended)
- **Mirostat Tau**: 5.0
- **Mirostat Eta**: 0.1

### Available Samplers

Choose from 11 different sampling strategies:

| Sampler | Description | Status |
|---------|-------------|--------|
| `Greedy` | Deterministic selection | ✅ Working |
| `Temperature` | Temperature-based sampling | ✅ Working |
| `Mirostat` | Mirostat sampling | ✅ Working |
| `TopP` | Nucleus sampling | ⚠️ Model-dependent |
| `TopK` | Top-K sampling | ⚠️ Model-dependent |
| `Typical` | Typical sampling | ⚠️ Model-dependent |
| `MinP` | Minimum probability threshold | ⚠️ Model-dependent |
| `TempExt` | Extended temperature | ⚠️ Model-dependent |
| `ChainTempTopP` | Temperature + Top-P chain | ⚠️ Model-dependent |
| `ChainTempTopK` | Temperature + Top-K chain | ⚠️ Model-dependent |
| `ChainFull` | Full chain (IBM recommended) | ⚠️ Model-dependent |

*Note: Advanced samplers may crash with some models due to compatibility issues*

### Model Configuration

Update the model path in `src/chat.rs`:

```rust
pub const MODEL_PATH: &str = "/path/to/your/model.gguf";
```

Default model:
```
/Users/agus/.lmstudio/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf
```

## 📁 Project Structure

```
llama_cpp_rs_chat/
├── src/
│   ├── lib.rs              # Tauri commands and state management
│   ├── main.rs             # Tauri app entry point
│   ├── chat.rs             # LLaMA chat engine with samplers
│   └── test.rs             # CLI version (for testing)
├── tauri.conf.json         # Tauri app configuration
├── index.html              # Frontend UI
├── main.js                 # Frontend JavaScript
├── package.json            # Node.js dependencies
├── assets/conversations/   # Chat history storage
└── vendor/llama-cpp-sys-2/ # llama.cpp bindings
```

## 🎯 Usage

### Desktop App

1. **Launch** the application using `cargo tauri dev`
2. **Chat** with the AI using the beautiful interface
3. **Configure** samplers via the settings button
4. **Execute Commands** by asking the AI to perform file operations
5. **View History** - all conversations are automatically saved

### Command Examples

Ask the AI to help with:

- **File Operations**: "Find all .txt files in this directory"
- **Code Analysis**: "Check the Rust code in src/ and summarize it"
- **System Tasks**: "Show me the current directory contents"
- **Development**: "Help me understand this codebase"

### CLI Mode

For development and testing:

```bash
# Run with debug output
RUST_LOG=debug cargo run --bin test

# Test specific functionality
cargo test --bin test
```

## 🔧 Development

### Building

```bash
# Build library
cargo build --lib

# Build Tauri app binary
cargo build --bin llama_cpp_chat

# Build CLI test binary
cargo build --bin test

# Build Tauri desktop app
cargo tauri build
```

### Frontend Development

```bash
# Install dependencies
npm install

# Start frontend dev server
npm run dev

# Build frontend
npm run build
```

### Testing

```bash
# Run Rust tests
cargo test

# Run e2e tests
cargo test test_e2e --bin test
```

## 🐳 Docker Support (Legacy)

For consistent environments or macOS Sequoia compatibility:

```bash
# Build and run
docker-compose up llama-chat

# Run tests
docker-compose --profile test up test-runner
```

## 📝 Conversation Logging

All conversations are automatically saved to:
```
assets/conversations/chat_YYYY-MM-DD-HH-mm-ss-SSS.txt
```

Format includes:
- User messages
- AI responses  
- Command executions with output
- Timestamps

## 🛠️ System Requirements

- **Rust** 1.70+ 
- **Node.js** 16+
- **Operating System**: macOS, Windows, or Linux
- **Memory**: 4GB+ RAM (depends on model size)
- **Storage**: Space for model files (typically 1-8GB)

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Submit a pull request

## 📄 License

This project is open source. Please check the license file for details.

## 🆘 Troubleshooting

### Common Issues

1. **Tauri build fails**: Ensure you have the latest Tauri CLI
2. **Model not found**: Update the MODEL_PATH in `src/chat.rs`
3. **Sampling crashes**: Try using `Greedy` or `Temperature` samplers
4. **macOS Sequoia issues**: Use Docker or wait for llama.cpp updates

### Debug Mode

Enable detailed logging:
```bash
RUST_LOG=debug cargo tauri dev
```

### Support

- Check existing GitHub issues
- Create a new issue with detailed error information
- Include system information and model details

---

**Built with ❤️ using Rust, Tauri, and llama.cpp**