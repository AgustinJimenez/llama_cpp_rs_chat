# Terminal LLM Chat

A powerful terminal-based chat application using llama-cpp-python with system command execution capabilities.

## Features

- 🤖 **Interactive terminal chat** with real-time streaming responses
- 🔧 **Command execution** - AI can run system commands to accomplish tasks
- 📁 **Conversation logging** - All chats saved with timestamps to JSON files
- 🖥️ **OS-aware** - Adapts to Windows/Linux/macOS automatically
- ⚡ **High performance** - 32K context, unlimited response tokens
- 🎯 **Smart model detection** - Automatically finds your LLM model

## Setup

1. Install dependencies:
   ```bash
   pip install -r requirements.txt
   ```

2. Ensure your GGUF model path is in `../assets/model_path.txt` (optional - has fallback)

## Usage

### From python_test directory:
```bash
python terminal_chat.py
```

### From project root:
```bash
python run_chat.py
```

## Commands

- Type your message and press Enter
- Type `quit`, `exit`, or `q` to end the conversation
- Press Ctrl+C to interrupt
- AI can execute commands using ```bash blocks, EXECUTE:, or RUN: syntax

## Files

- `terminal_chat.py` - Main chat application with command execution
- `requirements.txt` - Python dependencies
- `README.md` - This documentation

## Configuration

- **Model path**: Auto-detected from `../assets/model_path.txt` or falls back to default LMStudio path
- **Conversations**: Auto-saved to `../assets/conversations/chat_[timestamp].json`
- **Context**: 32,000 tokens
- **Commands**: Always enabled for maximum AI capabilities

## AI Capabilities

The AI assistant can:
- Execute terminal/system commands
- Install software and packages
- Create, modify, and delete files
- Run scripts and programs
- Access system information
- And much more!