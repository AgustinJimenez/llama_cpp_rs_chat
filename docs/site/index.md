# LLaMA Chat

A local-first AI chat application built on llama.cpp with full agentic tool support.

## Features

- **Local inference** — run GGUF models directly on your hardware, no API key required
- **GPU acceleration** — CUDA (NVIDIA), Metal (Apple), Vulkan, and CPU-only builds
- **Agentic tools** — file read/write, web search, shell execution, desktop automation, OCR
- **Cloud providers** — OpenAI-compatible API for Groq, Mistral, Anthropic, Gemini, and more
- **MCP support** — connect external MCP servers for additional tools
- **Vision** — attach images to messages (requires a multimodal model + mmproj)
- **Desktop app** — native Tauri desktop app or standalone web server mode

## Documentation

| Page | Description |
|------|-------------|
| [Installation](installation.md) | Build and install on Windows, macOS, Linux |
| [Quickstart](quickstart.md) | Get running in minutes |
| [Models](models.md) | Load, configure, and tune local models |
| [Providers](providers.md) | Connect cloud AI providers |
| [MCP](mcp.md) | Add external tool servers via MCP |
| [Desktop Tools](desktop-tools.md) | Let the AI control your desktop |
| [API Reference](api.md) | REST API for headless/programmatic use |
| [Troubleshooting](troubleshooting.md) | Common issues and fixes |

## Quick start

```bash
# Desktop app (auto-detects GPU)
npm run dev:auto:desktop

# Web server only (no Tauri)
npm run dev:auto
```

Then open **http://localhost:14000** in your browser.
