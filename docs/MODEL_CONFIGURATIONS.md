# Model Configurations

Recommended inference parameters for each GGUF model available in `E:\.lmstudio\models\`.

Parameters sourced from official model cards on HuggingFace and vendor documentation.

---

## Model Inventory

**Location:** `E:\.lmstudio\`

| # | Model | Arch | Params | Quant | Size | Doc |
|---|-------|------|--------|-------|------|-----|
| 1 | Qwen3-Coder-Next | qwen3next | 80B | Q4_K_M | 48.49 GB | ✓ |
| 2 | GLM-4.7-Flash | deepseek2 | 30B | Q4_K_M | 18.13 GB | ✓ |
| 3 | Nemotron-3-Nano-30B-A3B | nemotron | 30B-A3B | Q4_K_M | 24.52 GB | ✓ |
| 4 | Devstral-Small-2-2512 | mistral3 | 24B | Q6_K | 20.22 GB | ✓ |
| 5 | rnj-1-instruct | gemma3 | 8.3B | Q8_0 | 8.84 GB | ✓ |
| 6 | Ministral-3-14B-Reasoning | mistral3 | 14B | Q8_0 | 15.24 GB | ✓ |
| 7 | MiniCPM4.1-8B | minicpm | 8B | BF16 | 16.37 GB | ✓ |
| 8 | Magistral-Small-2509 | llama | 24B | Q6_K | 20.22 GB | ✓ |
| 9 | granite-4.0-h-tiny | granitehybrid | 7B | Q4_K_M | 4.23 GB | ⚠ |
| 10 | gpt-oss-20b | gpt-oss | 20B | MXFP4 | 12.11 GB | ⚠ |
| 11 | Qwen3-8B | qwen3 | 8B | Q8_0 | 8.71 GB | ✓ |
| 12 | gemma-3-12b-it | gemma3 | 12B | Q8_0 | 13.36 GB | ✓ |
| 13 | Qwen3-Coder-30B-A3B-1M | qwen3moe | 30B-A3B | Q4_K_S | 17.46 GB | ✓ |
| 14 | Qwen3-Coder-30B-A3B-1M-UD | qwen3moe | 30B-A3B | Q8_K_XL | 35.99 GB | ✓ |
| 15 | Qwen3-30B-A3B-2507 | qwen3moe | 30B-A3B | Q4_K_M | 18.56 GB | ✓ |
| 16 | Devstral-Small-2507 | llama | 24B | Q4_K_M | 14.33 GB | ✓ |
| 17 | Qwen3.5-35B-A3B | qwen35moe | 35B-A3B | Q4_K_M | 19.70 GB | ✓ |

*Last updated: 2026-02-25*

---

## Qwen3-Coder-Next (Alibaba)

**File:** `lmstudio-community/Qwen3-Coder-Next-GGUF/Qwen3-Coder-Next-Q4_K_M.gguf`

| Property | Value |
|----------|-------|
| Creator | Alibaba Qwen Team |
| Architecture | qwen3next (Hybrid Mamba-Transformer) |
| Total Parameters | 80B |
| Active Parameters | 3B (MoE) |
| Quantization | Q4_K_M |
| Context Window | 262,144 tokens (256K) |
| Chat Template | ChatML (`<\|im_start\|>...<\|im_end\|>`) |
| Thinking Mode | Non-thinking only |

### Official Sampling Parameters (HuggingFace)

| Parameter | Value | Notes |
|-----------|-------|-------|
| temperature | 1.0 | Official recommendation |
| top_p | 0.95 | Official recommendation |
| top_k | 40 | Official recommendation |

### GGUF Embedded Parameters

| Parameter | Value |
|-----------|-------|
| temperature | 1.0 |
| top_p | 0.95 |
| top_k | 40 |
| context_length | 262144 |

### Notes
- Hybrid Mamba-Transformer architecture with SSM (State Space Model) components.
- Despite 80B total parameters, only 3B are activated per token (very efficient).
- Optimized for agentic coding tasks with tool-calling capabilities.
- Non-thinking mode only (no `<think>` blocks).
- If OOM, reduce context to 32K.

### Sources
- https://huggingface.co/Qwen/Qwen3-Coder-Next

---

## GLM-4.7-Flash (Zai/THUDM)

**File:** `lmstudio-community/GLM-4.7-Flash-GGUF/GLM-4.7-Flash-Q4_K_M.gguf`

| Property | Value |
|----------|-------|
| Creator | Zai / THUDM |
| Architecture | deepseek2 (MLA attention) |
| Total Parameters | 30B |
| Quantization | Q4_K_M |
| Context Window | 202,752 tokens (~200K) |
| Chat Template | Custom GLM (`[gMASK]<sop>`) |

### Official Sampling Parameters (Z.ai)

**General use:**

| Parameter | Value | Notes |
|-----------|-------|-------|
| temperature | 1.0 | Default for general tasks |
| top_p | 0.95 | Default |
| max_new_tokens | 131072 | Default generation limit |

**Agentic / tool-calling (Terminal Bench, SWE Bench):**

| Parameter | Value | Notes |
|-----------|-------|-------|
| temperature | 0.7 | Official for SWE/agentic tasks |
| top_p | 1.0 | Official for SWE/agentic tasks |
| top_k | -1 | Disabled per code examples |
| min_p | 0.01 | Recommended for llama.cpp (overrides default 0.05) |
| repeat_penalty | 1.0 | **Must be disabled** per Z.ai |

**Community-suggested (HuggingFace discussion #6):**

| Parameter | Value |
|-----------|-------|
| temperature | 0.6 |
| top_k | 20 |
| repeat_penalty | 1.05 |
| min_p | 0.05 |
| top_p | 0.95 |

### GGUF Embedded Parameters

| Parameter | Value |
|-----------|-------|
| temperature | 1.0 |
| context_length | 202752 |

### Notes
- Uses DeepSeek2-style MLA (Multi-head Latent Attention) architecture.
- For agentic tasks, use temperature=0.7, top_p=1.0 (from Terminal Bench / SWE Bench config).
- Z.ai explicitly states: **disable repeat penalty** to avoid looping.
- For llama.cpp: set min_p=0.01 (backend default is 0.05, too high for this model).
- Has tool calling support via `<tools>` XML tags.
- Supports vLLM and SGLang frameworks.
- For τ²-Bench (deterministic agentic), use temperature=0 with "Preserved Thinking mode".

### Sources
- https://huggingface.co/zai-org/GLM-4.7-Flash
- https://huggingface.co/zai-org/GLM-4.7-Flash/discussions/6
- https://unsloth.ai/docs/models/glm-4.7-flash

---

## Nemotron-3-Nano-30B-A3B (NVIDIA)

**File:** `lmstudio-community/NVIDIA-Nemotron-3-Nano-30B-A3B-GGUF/NVIDIA-Nemotron-3-Nano-30B-A3B-Q4_K_M.gguf`

| Property | Value |
|----------|-------|
| Creator | NVIDIA |
| Architecture | nemotron_h_moe (Hybrid Mamba-MoE) |
| Total Parameters | 30B |
| Active Parameters | 3B (MoE) |
| Quantization | Q4_K_M |
| Context Window | 1,048,576 tokens (1M) |
| Chat Template | ChatML (`<\|im_start\|>...<\|im_end\|>`) |
| Thinking Mode | Supports `enable_thinking=True` |

### Official Sampling Parameters (NVIDIA)

| Parameter | Value | Notes |
|-----------|-------|-------|
| temperature | 1.0 | For reasoning tasks with thinking |
| top_p | 1.0 | For reasoning tasks |
| temperature | 0.6 | For tool calling / standard chat |
| top_p | 0.95 | For tool calling / standard chat |

### GGUF Embedded Parameters

| Parameter | Value |
|-----------|-------|
| temperature | 1.0 |
| top_p | 1.0 |
| context_length | 1048576 |

### Notes
- Hybrid Mamba-MoE architecture with SSM components.
- Only 3B parameters activated per token (very efficient).
- Native 1M context window.
- Use `enable_thinking=True` for reasoning tasks (temperature=1.0).
- Use `enable_thinking=False` for standard chat to reduce latency.

### Sources
- https://build.nvidia.com/nvidia/nemotron-3-nano-30b-a3b/modelcard

---

## Devstral-Small-2-24B-Instruct-2512 (Mistral)

**File:** `lmstudio-community/Devstral-Small-2-24B-Instruct-2512-GGUF/Devstral-Small-2-24B-Instruct-2512-Q6_K.gguf`

| Property | Value |
|----------|-------|
| Creator | Mistral AI |
| Architecture | mistral3 (dense transformer) |
| Total Parameters | 24B |
| Quantization | Q6_K |
| Context Window | 393,216 tokens (384K) |
| Chat Template | Mistral (`[INST]...[/INST]`) |
| Vision | Yes (multimodal) |

### Official Sampling Parameters (Mistral)

| Parameter | Value | Notes |
|-----------|-------|-------|
| temperature | 0.15 | Low temp for deterministic agentic tasks |

### GGUF Embedded Parameters

| Parameter | Value |
|-----------|-------|
| context_length | 393216 |

### Notes
- Vision-capable model (can analyze images).
- Runs on single RTX 4090 or 32GB Mac.
- Optimized for agentic coding tasks.
- Requires vLLM with `--tool-call-parser mistral --enable-auto-tool-choice`.
- Uses YaRN rope scaling for extended context.

### Sources
- https://huggingface.co/mistralai/Devstral-Small-2-24B-Instruct-2512

---

## rnj-1-instruct (EssentialAI)

**File:** `lmstudio-community/rnj-1-instruct-GGUF/rnj-1-instruct-Q8_0.gguf`

| Property | Value |
|----------|-------|
| Creator | EssentialAI |
| Architecture | gemma3 |
| Total Parameters | 8.3B |
| Quantization | Q8_0 |
| Context Window | 32,768 tokens (32K, extendable to 128K) |
| Chat Template | Llama3-style |

### Official Sampling Parameters (EssentialAI)

| Parameter | Value | Notes |
|-----------|-------|-------|
| temperature | 0.0 - 0.2 | Avoid higher temps |
| top_p | 0.95 | From examples |

### GGUF Embedded Parameters

| Parameter | Value |
|-----------|-------|
| temperature | 0.2 |
| context_length | 32768 |

### Notes
- Strong code inclination - tends to write code even for non-code tasks.
- **Always use a system prompt** (e.g., "You are a helpful assistant.")
- Omitting system prompt causes truncated outputs.
- Uses YaRN rope scaling (factor 4.0) for context extension.
- Trained on 32K, can extrapolate to 128K.

### Sources
- https://huggingface.co/EssentialAI/rnj-1-instruct

---

## Ministral-3-14B-Reasoning-2512 (Mistral)

**File:** `lmstudio-community/Ministral-3-14B-Reasoning-2512-GGUF/Ministral-3-14B-Reasoning-2512-Q8_0.gguf`

| Property | Value |
|----------|-------|
| Creator | Mistral AI |
| Architecture | mistral3 (dense transformer) |
| Total Parameters | 14B |
| Quantization | Q8_0 |
| Context Window | 262,144 tokens (256K) |
| Chat Template | Mistral with reasoning |
| Vision | Yes (multimodal) |

### Official Sampling Parameters (Mistral)

| Parameter | Value | Notes |
|-----------|-------|-------|
| temperature | 0.7 | Reasoning-specific recommendation (HuggingFace card examples, vLLM recipes). General baseline is 1.0 |
| top_p | 0.95 | Consistent across all Mistral examples |
| top_k | 40 | Not specified by Mistral (API doesn't expose it); 40 is safe default |
| repeat_penalty | 1.0 | Default (not specified) |
| context_size | 32768 | Practical for 24GB GPU. Max supported: 262,144 (YaRN rope scaling) |

**Config variants:**

| Variant | Temperature | Notes |
|---------|-------------|-------|
| Reasoning (recommended) | 0.7 | HuggingFace examples, vLLM recipes |
| General baseline | 1.0 | Model card "most environments" |
| Community/Unsloth | 0.6 | Lower temp for more deterministic output |

### GGUF Embedded Parameters

| Parameter | Value |
|-----------|-------|
| context_length | 262144 |

### Reasoning Tokens
- Uses `[THINK]`/`[/THINK]` delimiters for reasoning traces
- Keep reasoning traces in conversation history for multi-turn
- Budget ~16K-32K tokens for thinking traces in complex tasks

### Agent Test Score: **6/6** (with correct config)
- Previously 1/6 with temp=1.0, ctx=16384 (context exhaustion, buggy Python scripts)
- Fixed to 6/6 with temp=0.7, ctx=32768
- Uses Mistral bracket format `[TOOL_CALLS]name[ARGS]{...}`
- All math correct (avg salary $127,875)
- Minor: wrote to `results/` instead of `agent-tests/results/`

### Notes
- Built-in reasoning/thinking mode via chat template.
- Vision-capable (maintain ~1:1 aspect ratio for images).
- Keep reasoning traces in multi-turn context.
- Fits in 32GB VRAM (BF16) or <24GB with quantization.
- Uses YaRN rope scaling for extended context (factor 16.0, from 16K base to 256K).
- Dense 14B model (not MoE, despite "3" in name referring to model family version).

### Sources
- https://huggingface.co/mistralai/Ministral-3-14B-Reasoning-2512
- https://docs.mistral.ai/models/ministral-3-14b-25-12
- https://docs.mistral.ai/capabilities/reasoning
- https://docs.vllm.ai/projects/recipes/en/latest/Mistral/Ministral-3-Reasoning.html

---

## Ministral-3-3B-Instruct-2512 (Mistral)

**File:** `lmstudio-community/Ministral-3-3B-Instruct-2512-GGUF/Ministral-3-3B-Instruct-2512-Q8_0.gguf`

| Property | Value |
|----------|-------|
| Creator | Mistral AI |
| Architecture | mistral3 (dense transformer) |
| Total Parameters | 3B |
| Quantization | Q8_0 |
| Context Window | 262,144 tokens (256K) |
| Chat Template | Mistral |
| Vision | Yes (multimodal) |

### Official Sampling Parameters (Mistral)

| Parameter | Value | Notes |
|-----------|-------|-------|
| temperature | < 0.1 | Production / daily-driver |
| temperature | 0.15 | Used in vLLM code examples |
| top_p | — | Not specified by Mistral |
| top_k | — | Not specified by Mistral |
| repeat_penalty | — | Not specified (presence/frequency default to 0) |

### GGUF Embedded Parameters

| Parameter | Value |
|-----------|-------|
| context_length | 262144 |

### Notes
- Smallest model in the Ministral-3 family.
- **Too small for agentic tasks** — scored 1/6 on agent test suite.
- Can make tool calls (Mistral bracket format `[TOOL_CALLS]name[ARGS]{...}`) but generates buggy Python (reserved keywords, missing imports) and hallucinates tool calls to non-existent APIs.
- Fast at 21.2 tok/s on RTX 4090.
- Usable for simple tool calls (read_file, write_file) but unreliable for multi-step reasoning.
- Config verified correct against official sources (2026-02-24).

### Sources
- https://huggingface.co/mistralai/Ministral-3-3B-Instruct-2512
- https://huggingface.co/mistralai/Ministral-3-3B-Instruct-2512-GGUF
- https://docs.vllm.ai/projects/recipes/en/latest/Mistral/Ministral-3-Instruct.html

---

## MiniCPM4.1-8B (OpenBMB)

**File:** `Mungert/MiniCPM4.1-8B-GGUF/MiniCPM4.1-8B-bf16.gguf`

| Property | Value |
|----------|-------|
| Creator | OpenBMB |
| Architecture | minicpm |
| Total Parameters | 8B |
| Quantization | BF16 |
| Context Window | 65,536 tokens (64K, extendable to 128K) |
| Chat Template | ChatML (`<\|im_start\|>...<\|im_end\|>`) |

### Official Sampling Parameters (OpenBMB)

| Parameter | Value | Notes |
|-----------|-------|-------|
| temperature | 0.7 | Recommended |
| top_p | 0.7 | Recommended |
| repetition_penalty | 1.02 | Optional |
| max_tokens | 1024 | Standard limit |

### GGUF Embedded Parameters

| Parameter | Value |
|-----------|-------|
| context_length | 65536 |

### Notes
- Native 32K context, extendable to 128K with LongRoPE.
- Supports InfLLM v2 sparse attention for long sequences.
- Dense attention threshold at 8K tokens.
- Use `add_special_tokens=True` with vLLM chat API.

### Sources
- https://huggingface.co/openbmb/MiniCPM4-8B

---

## Magistral-Small-2509 (Mistral)

**File:** `lmstudio-community/Magistral-Small-2509-GGUF/Magistral-Small-2509-Q6_K.gguf`

| Property | Value |
|----------|-------|
| Creator | Mistral AI |
| Architecture | llama (dense transformer) |
| Total Parameters | 24B |
| Quantization | Q6_K |
| Context Window | 131,072 tokens (128K) |
| Chat Template | Mistral with `[THINK]` reasoning |
| Vision | Yes (up to 10 images) |

### Official Sampling Parameters (Mistral)

| Parameter | Value | Notes |
|-----------|-------|-------|
| temperature | 0.7 | Recommended |
| top_p | 0.95 | Recommended |
| max_tokens | 131072 | Full context |

### GGUF Embedded Parameters

| Parameter | Value |
|-----------|-------|
| context_length | 131072 |

### Notes
- Uses `[THINK]...[/THINK]` tokens for reasoning (special tokens, not strings).
- Vision-capable (max 10 images per prompt).
- Performance may degrade past 40K tokens but still works.
- Always include system prompt for best results.

### Sources
- https://huggingface.co/mistralai/Magistral-Small-2509

---

## granite-4.0-h-tiny (IBM)

**File:** `lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf`

| Property | Value |
|----------|-------|
| Creator | IBM |
| Architecture | granitehybrid (Hybrid MoE + SSM) |
| Total Parameters | 7B |
| Active Parameters | 1B (MoE) |
| Quantization | Q4_K_M |
| Context Window | 1,048,576 tokens (1M) |
| Chat Template | Custom Granite |

### Official Sampling Parameters (IBM)

| Parameter | Value | Notes |
|-----------|-------|-------|
| temperature | 0.0 | Best for standard inference |
| top_p | 1.0 | With temperature=0 |
| top_k | 0 | With temperature=0 |

### GGUF Embedded Parameters

| Parameter | Value |
|-----------|-------|
| context_length | 1048576 |

### Notes
- **Not fully functional.** Tool calls work (native `name{json}` format) but the model does not produce a natural language summary after receiving tool output — it just stops. Requires `repeat_penalty: 1.1` to prevent infinite tool-call loops (see llama.cpp issue #16678).
- Hybrid MoE + SSM architecture (only 1B active params).
- Native 1M context window, but use 4K-32K in practice on 24GB VRAM (1M = 300GB KV cache).
- Multilingual: 12 languages.
- Use temperature=0 for deterministic results.

### Sources
- https://www.ibm.com/granite/docs/models/granite

---

## gpt-oss-20b (OpenAI)

**File:** `lmstudio-community/gpt-oss-20b-GGUF/gpt-oss-20b-MXFP4.gguf`

| Property | Value |
|----------|-------|
| Creator | OpenAI |
| Architecture | gpt-oss (MoE) |
| Total Parameters | 21B |
| Active Parameters | 3.6B (MoE) |
| Quantization | MXFP4 |
| Context Window | 131,072 tokens (128K) |
| Chat Template | Harmony format |

### Official Sampling Parameters (OpenAI)

| Parameter | Value | Notes |
|-----------|-------|-------|
| - | - | Not explicitly documented |

### GGUF Embedded Parameters

| Parameter | Value |
|-----------|-------|
| context_length | 131072 |

### Notes
- **Not fully functional.** The model "thinks out loud" with internal monologue (`<|start|>assistant<|channel|>analysis` tags) visible in output. It tries multiple tool call formats (JSON `list_directory`, `execute_command`) before self-correcting to `<||SYSTEM.EXEC>` tags. Tool execution works but output is noisy. Use 8K context on 24GB VRAM (128K = 22.5GB KV cache overflow).
- **Must use Harmony response format** (won't work otherwise).
- Configurable reasoning effort via system prompt:
  - `"Reasoning: low"` - Fast responses
  - `"Reasoning: medium"` - Balanced
  - `"Reasoning: high"` - Deep analysis
- Fits in 16GB VRAM with MXFP4 quantization.
- Has sliding window attention.
- Supports browser and Python built-in tools.

### Sources
- https://huggingface.co/openai/gpt-oss-20b

---

## Qwen3-8B (Alibaba)

**File:** `lmstudio-community/Qwen3-8B-GGUF/Qwen3-8B-Q8_0.gguf`

| Property | Value |
|----------|-------|
| Creator | Alibaba Qwen Team |
| Architecture | qwen3 (dense transformer) |
| Total Parameters | 8B |
| Quantization | Q8_0 |
| Context Window | 32,768 tokens (32K, extendable to 128K) |
| Chat Template | ChatML (`<\|im_start\|>...<\|im_end\|>`) |
| Thinking Mode | Supports `enable_thinking=True/False` |

### Official Sampling Parameters (Qwen)

| Parameter | Value | Notes |
|-----------|-------|-------|
| temperature | 0.6 | Thinking mode |
| top_p | 0.95 | Thinking mode |
| top_k | 20 | Both modes |
| min_p | 0 | Both modes |
| temperature | 0.7 | Non-thinking mode |
| top_p | 0.8 | Non-thinking mode |

### GGUF Embedded Parameters

| Parameter | Value |
|-----------|-------|
| context_length | 32768 |

### Notes
- **DO NOT use greedy decoding** in thinking mode (causes repetitions).
- For multi-turn: exclude `<think>` content from history.
- Native 32K context, extendable to 128K with YaRN.
- YaRN may impact short text performance.

### Sources
- https://huggingface.co/Qwen/Qwen3-8B

---

## gemma-3-12b-it (Google)

**File:** `lmstudio-community/gemma-3-12b-it-GGUF/gemma-3-12b-it-Q8_0.gguf`

| Property | Value |
|----------|-------|
| Creator | Google |
| Architecture | gemma3 |
| Total Parameters | 12B |
| Quantization | Q8_0 |
| Context Window | 131,072 tokens (128K) |
| Chat Template | Gemma (`<start_of_turn>...<end_of_turn>`) |
| Vision | Yes (multimodal) |

### Official Sampling Parameters (Google)

| Parameter | Value | Notes |
|-----------|-------|-------|
| do_sample | False | Examples use deterministic |
| max_new_tokens | 8192 | Max output length |

### GGUF Embedded Parameters

| Parameter | Value |
|-----------|-------|
| context_length | 131072 |
| sliding_window | 1024 |

### Notes
- Vision-capable (images at 896x896, encoded to 256 tokens each).
- Uses sliding window attention (1024 tokens).
- Linear rope scaling (factor 8.0) for 128K context.
- Use `torch.bfloat16` for GPU inference.
- Refer to [Gemma 3 Technical Report](https://goo.gle/Gemma3Report) for detailed params.

### Sources
- https://huggingface.co/google/gemma-3-12b-it

---

## Devstral-Small-2507 (Mistral)

**File:** `lmstudio-community/Devstral-Small-2507-GGUF/Devstral-Small-2507-Q4_K_M.gguf`

| Property | Value |
|----------|-------|
| Creator | Mistral AI |
| Architecture | Llama (dense transformer) |
| Total Parameters | 24B |
| Quantization | Q4_K_M |
| Context Window | 131,072 tokens (128K) |
| Tokenizer | Tekken (131K vocabulary) |
| Chat Template | Mistral v7 (`[INST]...[/INST]`) |
| Base Model | Mistral-Small-3.1-24B-Base-2503 |

### Recommended Sampling Parameters

| Parameter | Value | Notes |
|-----------|-------|-------|
| temperature | **0.15** | Official example uses 0.15 for agentic/tool use |
| top_p | 0.95 | Not explicitly specified; common default |
| top_k | 64 | Community-recommended |
| min_p | 0.01 | Community-recommended |
| repetition_penalty | 1.0 | No repetition penalty needed |

### Notes
- Designed specifically for **agentic coding tasks** (software engineering, tool use).
- Very low temperature (0.15) recommended for deterministic tool-calling behavior.
- Supports Mistral's native function calling format (`[TOOL_CALLS]`).
- In this app, uses the `__AGENTIC__` system prompt with `<||SYSTEM.EXEC>` tags.

### Sources
- https://huggingface.co/mistralai/Devstral-Small-2507
- https://muxup.com/2025q2/recommended-llm-parameter-quick-reference

---

## Qwen3-30B-A3B-Instruct-2507

**File:** `lmstudio-community/Qwen3-30B-A3B-Instruct-2507-GGUF/Qwen3-30B-A3B-Instruct-2507-Q4_K_M.gguf`

| Property | Value |
|----------|-------|
| Creator | Alibaba Qwen Team |
| Architecture | Mixture of Experts (MoE) |
| Total Parameters | 30.5B |
| Active Parameters | 3.3B (8 of 128 experts) |
| Quantization | Q4_K_M |
| Context Window | 262,144 tokens (256K) |
| Chat Template | ChatML (`<\|im_start\|>...<\|im_end\|>`) |
| Thinking Mode | Non-thinking only (no `<think>` blocks) |

### Recommended Sampling Parameters

| Parameter | Value | Notes |
|-----------|-------|-------|
| temperature | **0.7** | Official recommendation |
| top_p | **0.8** | Official recommendation |
| top_k | **20** | Official recommendation |
| min_p | 0 | Official recommendation |
| presence_penalty | 0 to 2 | Adjust upward to reduce repetition |
| max_output_tokens | 16,384 | Adequate for most queries |

### Notes
- MoE architecture: only 3.3B parameters active per token (very fast for its size).
- Non-thinking mode only (the `-2507` variant; the `-Thinking-2507` variant has thinking mode).
- Excellent for complex reasoning with long context.
- Tool calling via ChatML tool format (`<tool_call>`).
- Known issue: model safety training may refuse direct file operation tool names; use bash workarounds.

### Sources
- https://huggingface.co/Qwen/Qwen3-30B-A3B-Instruct-2507
- https://muxup.com/2025q2/recommended-llm-parameter-quick-reference

---

## Qwen3-Coder-30B-A3B-Instruct-1M

**Files:**
- `lmstudio-community/.../Qwen3-Coder-30B-A3B-Instruct-1M-UD-Q8_K_XL.gguf` (Q8_K_XL, higher quality)
- `unsloth/.../Qwen3-Coder-30B-A3B-Instruct-1M-Q4_K_S.gguf` (Q4_K_S, smaller/faster)

| Property | Value |
|----------|-------|
| Creator | Alibaba Qwen Team |
| Architecture | Mixture of Experts (MoE) |
| Total Parameters | 30.5B |
| Active Parameters | 3.3B (8 of 128 experts) |
| Quantization | Q8_K_XL / Q4_K_S |
| Context Window | 262,144 tokens native (extendable to 1M via YaRN) |
| Chat Template | ChatML (`<\|im_start\|>...<\|im_end\|>`) |
| Thinking Mode | Non-thinking only |

### Recommended Sampling Parameters

| Parameter | Value | Notes |
|-----------|-------|-------|
| temperature | **0.7** | Official recommendation |
| top_p | **0.8** | Official recommendation |
| top_k | **20** | Official recommendation |
| repetition_penalty | **1.05** | Official recommendation (slightly higher than Qwen3 base) |
| max_output_tokens | 65,536 | Adequate for most instruct queries |

### Quantization Comparison

| Variant | Size | Quality | Speed | Best For |
|---------|------|---------|-------|----------|
| Q8_K_XL | ~32 GB | Higher fidelity | Slower | Maximum output quality |
| Q4_K_S | ~16 GB | Good | Faster | Balance of quality and speed |

### Notes
- Code-specialized variant of Qwen3-30B-A3B with extended context training.
- Native 256K context, extendable to 1M tokens with YaRN rope scaling.
- Optimized for code generation, analysis, and refactoring.
- Same MoE architecture as Qwen3-30B-A3B (3.3B active parameters).
- Same tool calling behavior as Qwen3-30B-A3B (bash tools work; file tools may be refused).

### Sources
- https://huggingface.co/Qwen/Qwen3-Coder-30B-A3B-Instruct

---

## Vision / Image Handling

Models that support multimodal (image + text) input require a separate **mmproj** (multimodal projection) GGUF file alongside the model. This file contains the CLIP vision encoder that converts images into embeddings the LLM can understand.

### Vision-Capable Models (mmproj available locally)

| Model | Model Size | mmproj File | mmproj Size | Max Images | Status |
|-------|-----------|-------------|-------------|------------|--------|
| **Magistral-Small-2509** (24B Q6_K) | 20.2 GB | mmproj-Magistral-Small-2509-F16.gguf | 838 MB | 10 | Untested |
| **Devstral-Small-2** (24B Q6_K) | 20.2 GB | mmproj-Devstral-Small-2-...-F16.gguf | 838 MB | — | Untested |
| **Ministral-3-14B-Reasoning** (14B Q8_0) | 15.2 GB | mmproj-Ministral-3-14B-...-F16.gguf | 838 MB | — | Untested |
| **Ministral-3-3B** (3B Q8_0) | 3.4 GB | mmproj-Ministral-3-3B-...-F16.gguf | 802 MB | — | Untested |
| **gemma-3-12b-it** (12B Q8_0) | 13.4 GB | mmproj-model-f16.gguf | 815 MB | — | Untested |
| **GLM-4.6V-Flash** (30B Q4_K_M) | 18.1 GB | mmproj-GLM-4.6V-Flash-F16.gguf | 1.7 GB | — | Untested |

### VRAM Considerations

The mmproj file adds ~0.8–1.7 GB to VRAM usage on top of the model itself. On a 24 GB GPU:
- **Magistral-Small + mmproj**: ~21 GB (tight fit with 32K context)
- **Ministral-14B-R + mmproj**: ~16 GB (comfortable)
- **gemma-3-12b + mmproj**: ~14 GB (plenty of room)
- **GLM-4.6V-Flash + mmproj**: ~20 GB (larger vision encoder)

### Notes
- All Mistral-family models (Magistral, Devstral, Ministral) share a similar 838 MB vision encoder
- GLM-4.6V-Flash has a larger 1.7 GB encoder (different architecture)
- gemma-3 encodes images at 896x896 resolution → 256 tokens per image
- Magistral-Small officially supports up to 10 images per prompt
- mmproj files are located in the same directory as the model GGUF file
- The app auto-detects mmproj files via `scan_for_mmproj_files()` in `model.rs`

### Models Without Vision

The following models in the inventory are **text-only** (no mmproj available):
- Qwen3-Coder-Next (80B)
- Nemotron-3-Nano-30B-A3B
- MiniCPM4.1-8B
- Qwen3-8B
- Qwen3-Coder-30B-A3B
- Qwen3-30B-A3B
- Devstral-Small-2507 (older version, text-only)
- granite-4.0-h-tiny
- gpt-oss-20b
- rnj-1-instruct

---

## Quick Reference

### Parameter Comparison Across Models (Agentic Config)

| Parameter | Devstral-Small | GLM-4.7-Flash | Magistral-Small | Qwen3-Coder | Qwen3-30B-A3B | Ministral-14B-R | Ministral-3-3B |
|-----------|---------------|---------------|-----------------|-------------|---------------|-----------------|----------------|
| temperature | 0.15 | 0.7 | 0.7 | 0.7 | 0.7 | 0.7 | 0.1 |
| top_p | 0.95 | 1.0 | 0.95 | 0.8 | 0.8 | 0.95 | 0.95 |
| top_k | 64 | 0 (disabled) | 40 | 20 | 20 | 40 | 40 |
| min_p | 0.01 | 0.01 | — | — | 0 | — | — |
| rep. penalty | 1.0 | 1.0 (must!) | 1.0 | 1.05 | — | 1.0 | 1.0 |
| context | 128K | 32K* | 131K | 256K (1M) | 256K | 32K** | 256K |
| active params | 24B (dense) | 30B (MLA) | 24B (dense) | 3.3B (MoE) | 3.3B (MoE) | 14B (dense) | 3B (dense) |
| agent score | 6/6 | 5.5/6 | 6/6 | 6/6 | slow | 6/6 | 1/6 |

*GLM-4.7-Flash: native 200K context, but 32K recommended for 24GB VRAM
**Ministral-14B-R: max 256K (YaRN scaling), but 32K practical for 24GB VRAM

### Model Selection Guide

| Use Case | Recommended Model |
|----------|-------------------|
| Agentic tasks / tool calling | Devstral-Small (most reliable tool use) |
| Agentic + reasoning | Ministral-14B-Reasoning (6/6, reasoning traces, dense 14B) |
| Complex reasoning | Qwen3-30B-A3B (strong reasoning, large context) |
| Code generation / analysis | Qwen3-Coder (code-optimized, 1M context) |
| Long document processing | Qwen3-Coder Q4_K_S (1M context, lighter) |
| Maximum output quality | Qwen3-Coder Q8_K_XL (highest precision) |
| Fastest inference | Qwen3-30B-A3B or Qwen3-Coder (3.3B active) |
| Vision / image analysis | Magistral-Small (best quality) or Ministral-14B-R (fits easily on 24GB) |

### Config.json Examples

**For Devstral (agentic tool use):**
```json
{
  "sampler_type": "Temperature",
  "temperature": 0.15,
  "top_p": 0.95,
  "top_k": 64,
  "system_prompt": "__AGENTIC__",
  "context_size": 131072
}
```

**For Qwen3-30B-A3B (general reasoning):**
```json
{
  "sampler_type": "Temperature",
  "temperature": 0.7,
  "top_p": 0.8,
  "top_k": 20,
  "system_prompt": "__AGENTIC__",
  "context_size": 131072
}
```

**For Qwen3-Coder (code tasks):**
```json
{
  "sampler_type": "Temperature",
  "temperature": 0.7,
  "top_p": 0.8,
  "top_k": 20,
  "system_prompt": "__AGENTIC__",
  "context_size": 262144
}
```

---

## Qwen3.5-35B-A3B (Alibaba)

**File:** `lmstudio-community/Qwen3.5-35B-A3B-GGUF/Qwen3.5-35B-A3B-Q4_K_M.gguf`

| Property | Value |
|----------|-------|
| Creator | Alibaba Qwen Team |
| Architecture | qwen35moe (Gated DeltaNet + MoE) |
| Total Parameters | 35B |
| Active Parameters | ~3B (8 routed + 1 shared of 256 experts) |
| Quantization | Q4_K_M |
| File Size | 19.70 GB |
| Context Window | 262,144 tokens (256K, extensible to ~1M via YaRN) |
| Chat Template | ChatML (`<\|im_start\|>...<\|im_end\|>`) |
| Thinking Mode | Yes (`<think>...</think>`, on by default) |
| Vision | Yes (mmproj-Qwen3.5-35B-A3B-BF16.gguf, 861 MB) |
| Tool Format | Qwen native (`<tool_call>`/`</tool_call>`) |
| `general.name` | `Qwen3.5-35B-A3B` |

### Official Sampling Parameters (HuggingFace)

**Non-thinking mode (general tasks, recommended for agentic use):**

| Parameter | Value | Notes |
|-----------|-------|-------|
| temperature | 0.7 | Official recommendation |
| top_p | 0.8 | Official recommendation |
| top_k | 20 | Official recommendation |
| min_p | 0.0 | Official recommendation |
| presence_penalty | 1.5 | Important — reduces repetition |
| repetition_penalty | 1.0 | Not needed |

**Thinking mode (general tasks):**

| Parameter | Value | Notes |
|-----------|-------|-------|
| temperature | 1.0 | Official recommendation |
| top_p | 0.95 | Official recommendation |
| top_k | 20 | Official recommendation |
| presence_penalty | 1.5 | Reduces repetition |

**Thinking mode (precise coding):**

| Parameter | Value | Notes |
|-----------|-------|-------|
| temperature | 0.6 | Lower for deterministic code |
| top_p | 0.95 | Official recommendation |
| top_k | 20 | Official recommendation |
| presence_penalty | 0.0 | Disabled for coding |

### GGUF Embedded Parameters

| Parameter | Value |
|-----------|-------|
| context_length | 262144 |

### Notes
- Novel hybrid architecture: 3 Gated DeltaNet (linear attention, O(n)) layers per 1 full softmax attention layer, in repeating 4-layer cycles across 40 layers.
- MoE with 256 experts (8 routed + 1 shared active per token) — only ~3B active parameters for fast inference.
- Vision-language model with image and video understanding (OCR, document QA, spatial reasoning).
- `presence_penalty` of 1.5 is officially recommended for general tasks — significantly higher than typical models.
- **llama.cpp performance warning**: Currently ~35% slower than Qwen3-30B-A3B on CUDA due to incomplete GPU kernel support for DeltaNet layers (llama.cpp issue #19894).
- **VRAM on 24GB GPU**: Model (19.7 GB) + mmproj (861 MB) ≈ 20.6 GB. Use `context_size` 16384–32768.
- Released 2026-02-24. llama.cpp support is via b8140/b8145 — may still have bugs.

### Sources
- https://huggingface.co/Qwen/Qwen3.5-35B-A3B
- https://github.com/QwenLM/Qwen3.5
