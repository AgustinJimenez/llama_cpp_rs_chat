Claude Code agents: read AGENTS.md for the canonical development guidance. If instructions ever diverge for Claude specifically, note it here after updating AGENTS.md first.

## Native Chat Template Implementation (2026-01-20)

Successfully implemented Phase 1 of native chat template support using llama.cpp's `chat_template()` and `apply_chat_template()` APIs.

### Implementation Summary

**File: `src/web/chat/templates.rs`**
- Added `parse_conversation_to_messages()` function to convert conversation format to `Vec<LlamaChatMessage>`
- Modified `apply_model_chat_template()` to route based on `system_prompt`:
  - `system_prompt = None` → "Model Default" mode → Uses native llama.cpp template
  - `system_prompt = Some(...)` → "Agentic/Custom" mode → Uses legacy hardcoded templates

**File: `src/web/chat/generation.rs`**
- Updated to pass `model` reference and `system_prompt` to template application function

**File: `src/web/websocket.rs`**
- Updated token counting to load config and pass `system_prompt` parameter

### Critical Finding: Template Type Detection

The template type detection in `src/web/model_manager.rs` lines 145-164 only recognizes hardcoded patterns:
- ChatML: `<|im_start|>` and `<|im_end|>`
- Mistral: `[INST]` and `[/INST]`
- Llama3: `<|start_header_id|>`
- Gemma: `<start_of_turn>` and `<end_of_turn>`
- **Generic: Fallback for everything else**

GLM-4.6V-Flash gets detected as "Generic" because its template uses `[gMASK]<sop><|user|>...<|assistant|>` format.

However, **this is not a problem** - the `apply_model_chat_template()` function now checks `system_prompt` mode BEFORE checking template type, so even "Generic" templates use the native llama.cpp template when in Model Default mode.

### Testing Results

✅ **SUCCESS**: Native template is being applied correctly
- Logs confirm: `=== USING NATIVE CHAT TEMPLATE ===`
- Prompt format: `[gMASK]<sop><|user|>\nHello! Can you introduce yourself briefly?<|assistant|>\n`
- This is GLM's native format, not the hardcoded ChatML we were using before

⚠️ **REMAINING ISSUE**: Model still gets stuck in infinite loops
- This is a DIFFERENT problem from template formatting
- The native template is correct, but stop tokens aren't working properly
- Model generates `</think>` tags and repeats "我是GLM-4.5" indefinitely
- Need to investigate stop token configuration in Phase 2

### Next Steps (Phase 2)

1. Investigate GLM's stop token requirements
2. Implement native tool format detection (GLM uses XML `<tool>` tags)
3. Add tool call execution for native format
4. Test tool calling with GLM model
