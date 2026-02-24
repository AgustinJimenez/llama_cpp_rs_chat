# Agent Test Results

Hardware: RTX 4090 24GB, Windows 11, llama-cpp-2 v0.1.122
Test suite: 6 tests (read/summarize, extract JSON, answer from JSON, multi-file reasoning, log transform, write docs from spec)

## PASS (6/6)

| Model | Size | Quant | Tokens Used | Tok/s | Tool Format | Notes |
|-------|------|-------|-------------|-------|-------------|-------|
| Devstral-Small-2-2512 | 19G | Q6_K | 12022/131072 | 29.1 | Mistral bracket `[TOOL_CALLS]name[ARGS]{...}` | Clean execution |
| Devstral-Small-2507 | 14G | Q4_K_M | — | — | Mistral comma `name,{...}` | Clean execution |
| Magistral-Small-2509 | 19G | Q6_K | 12059/131072 | 30.7 | JSON `{"name":"...","arguments":{...}}` | 131K context window |
| Qwen3-Coder-30B-A3B-1M | 17G | Q4_K_S | — | — | Llama3 XML `<function=name>` | MoE 3B active |

## PARTIAL

| Model | Size | Quant | Score | Tok/s | Issues |
|-------|------|-------|-------|-------|--------|
| GLM-4.7-Flash | 17G | Q4_K_M | 4/6 | 30.3 | T3: wrong employee count/avg salary. T6: file not written. Wrote to `expected/` instead of `results/` |
| GLM-4.6V-Flash | 9.4G | Q8_0 | 2/6 | — | `</think><\|begin_of_box\|>` tokens leaked into JSON output. Wrong salary calc. T6 missing |
| Qwen3-30B-A3B-2507 | 18G | Q4_K_M | — | ~0.04 | Correct Qwen `<tool_call>` format but unusably slow (~0.04 tok/s) |

## FAIL

| Model | Size | Quant | Score | Tok/s | Failure Mode |
|-------|------|-------|-------|-------|-------------|
| Ministral-3-14B-Reasoning | 14G | Q8_0 | 1/6 | — | Wasted tokens on repeated buggy Python scripts for Test 2. Context exhausted at 16384 |
| Nemotron-3-Nano-30B-A3B | 23G | Q4_K_M | 0/6 | ~4 | Very slow (partial RAM offload). Stuck retrying Test 2 Python code endlessly |
| Qwen3-8B | 8.2G | Q8_0 | 0/6 | — | Thinking-mode `<think>` loop consumed all tokens without productive output |
| Gemma-3-12B-it | 12G | Q8_0 | 0/6 | 20.4 | No tool call capability — described what it would do instead of acting |
| GPT-OSS-20B | 12G | MXFP4 | 0/6 | 21.0 | No tool call capability — generated 1 sentence and stopped |
| MiniCPM4.1-8B | 16G | BF16 | 0/6 | 17.8 | No tool call capability — generated pseudo-code explaining the approach |
| RNJ-1-Instruct | 8.3G | Q8_0 | 0/6 | — | Stuck in repetition loop (same THOUGHT + List Directory repeated) |
| Granite-4.0-h-tiny | 4G | Q4_K_M | 0/6 | 17.5 | Immediate EOS — generated 0 response tokens |
| Ministral-3-3B | 3.5G | Q8_0 | 0/6 | — | Context limit (16K) hit before completing any test. Buggy Python scripts |

## SKIPPED

| Model | Size | Reason |
|-------|------|--------|
| Qwen3-Coder-30B-A3B-1M-UD | 36G Q8_K_XL | Exceeds 24GB VRAM |
| Qwen3-Coder-Next | 46G Q4_K_M | Exceeds 24GB VRAM |

## Key Findings

1. **Only Mistral-family and Qwen3-Coder reliably pass all 6 tests.** These models have strong tool-calling training built in.
2. **Minimum viable size is ~14B parameters** for agentic tasks. All models under 14B failed.
3. **Tool format matters:** Models must be trained to emit a recognized tool call format (Mistral bracket/comma, Qwen XML/JSON, or standard JSON). Models without this training (Gemma, GPT-OSS, MiniCPM, Granite) cannot perform agentic tasks regardless of size.
4. **Thinking mode is a liability:** GLM and Qwen3 models emit `<think>` tags that waste context and can leak into tool output, corrupting results.
5. **Python-via-tool is fragile:** Models that try to solve tasks by generating Python scripts (Ministral-14B, Nemotron, GLM-4.6V) frequently generate buggy code and burn through context on retries.
6. **MoE models can be slow if VRAM-constrained:** Nemotron-3-Nano (23GB on 24GB VRAM) ran at ~4 tok/s due to partial RAM offload.
7. **Context window size is critical:** Models with only 16K context (Ministral-3B, Qwen3-8B) exhaust tokens before completing all 6 tests.
