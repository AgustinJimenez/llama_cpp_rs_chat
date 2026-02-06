# Model Configurations

Recommended inference parameters for each GGUF model available in `E:\.lmstudio\models\`.

Parameters sourced from official model cards on HuggingFace and vendor documentation.

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

## Quick Reference

### Parameter Comparison Across Models

| Parameter | Devstral-Small | Qwen3-30B-A3B | Qwen3-Coder |
|-----------|---------------|---------------|-------------|
| temperature | 0.15 | 0.7 | 0.7 |
| top_p | 0.95 | 0.8 | 0.8 |
| top_k | 64 | 20 | 20 |
| min_p | 0.01 | 0 | - |
| rep. penalty | 1.0 | - | 1.05 |
| context | 128K | 256K | 256K (1M ext.) |
| active params | 24B (dense) | 3.3B (MoE) | 3.3B (MoE) |

### Model Selection Guide

| Use Case | Recommended Model |
|----------|-------------------|
| Agentic tasks / tool calling | Devstral-Small (most reliable tool use) |
| Complex reasoning | Qwen3-30B-A3B (strong reasoning, large context) |
| Code generation / analysis | Qwen3-Coder (code-optimized, 1M context) |
| Long document processing | Qwen3-Coder Q4_K_S (1M context, lighter) |
| Maximum output quality | Qwen3-Coder Q8_K_XL (highest precision) |
| Fastest inference | Qwen3-30B-A3B or Qwen3-Coder (3.3B active) |

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
