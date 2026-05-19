# Quickstart

## 1. Download a model

Download any GGUF model. Recommended starting points:

| Model | Size | Use case |
|-------|------|----------|
| Qwen3-8B-Q8_0 | ~8 GB | General purpose, fast |
| Qwen3-30B-A3B-Q4 | ~17 GB | Agentic tasks (MoE) |
| Mistral-7B-Instruct-Q5 | ~5 GB | Small, fast chat |

Good sources: [Hugging Face](https://huggingface.co/models?library=gguf), [LM Studio model hub](https://lmstudio.ai/models).

For **vision** (image input), also download the matching `mmproj-*.gguf` file and place it in the same directory as the main model.

---

## 2. Start the app

```bash
npm run dev:auto:desktop   # Tauri desktop app
# or
npm run dev:auto           # Web-only server
```

Open **http://localhost:14000**.

---

## 3. Load a model

1. Click **Load Model** in the sidebar
2. Enter the path to your `.gguf` file (e.g. `E:/models/Qwen3-8B-Q8_0.gguf`)
3. Adjust GPU layers if needed (more layers = more VRAM, faster inference)
4. Click **Load**

The model loads in a background worker process. VRAM is reclaimed automatically when you unload.

---

## 4. Send your first message

Type in the chat box and press **Enter** (or **Ctrl+Enter** for newline).

### Enable tools

Click the **Tools** toggle to give the model access to:
- `read_file` / `write_file` — file system access
- `execute_command` — run shell commands
- `web_search` / `web_fetch` — browse the web
- `take_screenshot` — capture the screen

### Attach an image

Paste (`Ctrl+V`) or drag-and-drop an image to attach it to your message (requires a vision-capable model).

---

## 5. Use a cloud provider instead

If you don't have a powerful GPU, connect a cloud provider:

1. Click **Settings** → **Providers**
2. Select a provider (e.g. Groq — has a free tier)
3. Enter your API key
4. Choose a model from the dropdown

See [Providers](providers.md) for the full list.

---

## What's next?

- [Configure your model](models.md) — context size, samplers, KV cache
- [Add MCP servers](mcp.md) — extend tools
- [REST API](api.md) — automate the app
