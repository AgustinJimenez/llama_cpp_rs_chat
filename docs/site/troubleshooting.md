# Troubleshooting

## Model loading

**"CUDA out of memory" or very slow inference (>50ms/token)**

The KV cache doesn't fit in VRAM and overflows to RAM. Fix:
1. Reduce `context_size` (halving it roughly halves KV cache VRAM)
2. Switch KV cache to `q8_0` or `turbo2`/`turbo3` (50%–25% of default VRAM)
3. Reduce `gpu_layers` to push some layers to CPU

**Model loads but outputs garbage**

Usually a wrong chat template. Check:
- The model's GGUF metadata has a valid `tokenizer.chat_template`
- System prompt format matches the model family
- Try a different sampler preset (some models need specific temperature settings)

**Worker process crashes on load**

Check the log file (`tauri-dev.err.log` or `server_stderr.log`). Common causes:
- Not enough RAM for the model weight file
- Corrupt GGUF file
- Missing llama.cpp support for this model architecture

---

## Performance

**Low tokens/second despite GPU**

1. Check `gpu_layers` — should equal the model's layer count for full GPU inference
2. Verify CUDA is active: model status panel should show GPU memory usage
3. Context size too large for VRAM → KV cache paging. Reduce context.
4. Flash attention: enable for MoE models (required for Qwen3.5-35B)

**High VRAM usage at idle**

The model stays loaded between conversations. Use **Unload Model** in the sidebar to free VRAM.

---

## Tools / agentic mode

**Tools not executing**

- Ensure "Tools" is toggled on in the chat input bar
- Some models need a system prompt that lists available tools — check model settings
- Local models: tool call parsing may fail for some model families. Check `docs/MODEL_CONFIGURATIONS.md` for tested configs.

**Web search returns "blocked" or no results**

- The default search uses the in-app browser (Tauri) or falls back to curl
- Google may block curl requests — use the browser backend instead
- In web mode without Tauri, install Chrome and enable the CDP fallback

---

## UI

**Sidebar flickers when starting a new conversation**

Known issue — sidebar briefly shows a loading state. No data is lost.

**`<think>` tags appear as plain text**

Update to the latest version. This was fixed in a recent release.

---

## Build errors

**`cmake: command not found`**

Run `npm run cargo -- build ...` instead of bare `cargo build`. CMake is auto-downloaded.

**`error: linker 'link.exe' not found` (Windows)**

Open the "x64 Native Tools Command Prompt for VS 2022" from the Start Menu and build from there. Or install the VS 2022 C++ build tools.

**`libwebkit2gtk not found` (Linux)**

Install WebKit:
```bash
sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev
```

---

## Getting help

- Check existing issues in the repository
- Include: OS, GPU model, CUDA version, model name + quantization, error message
