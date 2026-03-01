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
| Ministral-3-14B-Reasoning | 14G | Q8_0 | ~10K/32768 | ~15 | Mistral bracket `[TOOL_CALLS]name[ARGS]{...}` | Reasoning model. Config fix: temp 1.0→0.7, ctx 16384→32768. Previously 1/6 (context exhausted). Wrote to `results/` instead of `agent-tests/results/` |

## NEAR-PASS (5/6)

| Model | Size | Quant | Score | Tok/s | Tool Format | Notes |
|-------|------|-------|-------|-------|-------------|-------|
| GLM-4.7-Flash | 17G | Q4_K_M | 5.5/6 | 21.7 | SYSTEM.EXEC | T3: math error on avg salary ($123,875 vs $127,875). T5: minor JSON syntax. All 6 files written correctly. Agentic config (temp=0.7, top_p=1.0, min_p=0.01). See retest section below |

## PARTIAL
| GLM-4.6V-Flash | 9.4G | Q8_0 | 2/6 | 41.6 | `<tool_call>{json}<\|end_of_box\|>` format. T1 PASS, T2 FAIL (single quotes in JSON, unescaped `"` in content), T3 PASS (all 4 answers correct), T4 FAIL (correct analysis but malformed write_file JSON), T5-T6 not attempted. Uses `<\|end_of_box\|>` as close tag (handled). ctx=131072, temp=0.7 |
| Qwen3-30B-A3B-2507 | 18G | Q4_K_M | — | ~0.04 | Correct Qwen `<tool_call>` format but unusably slow (~0.04 tok/s) |

## FAIL

| Model | Size | Quant | Score | Tok/s | Failure Mode |
|-------|------|-------|-------|-------|-------------|
| Ministral-3-14B-Reasoning | 14G | Q8_0 | 1/6 | — | ~~Wasted tokens on repeated buggy Python scripts for Test 2. Context exhausted at 16384~~ **FIXED: Now 6/6 with correct config (temp=0.7, ctx=32768)** |
| Nemotron-3-Nano-30B-A3B | 23G | Q4_K_M | 0/6 | ~4 | Very slow (partial RAM offload). Stuck retrying Test 2 Python code endlessly |
| Qwen3-8B | 8.2G | Q8_0 | 0/6 | — | Thinking-mode `<think>` loop consumed all tokens without productive output |
| Gemma-3-12B-it | 12G | Q8_0 | 0/6 | 20.4 | No tool call capability — described what it would do instead of acting |
| GPT-OSS-20B | 12G | MXFP4 | 0/6 | 21.0 | No tool call capability — generated 1 sentence and stopped |
| MiniCPM4.1-8B | 16G | BF16 | 0/6 | 17.8 | No tool call capability — generated pseudo-code explaining the approach |
| RNJ-1-Instruct | 8.3G | Q8_0 | 0/6 | — | Stuck in repetition loop (same THOUGHT + List Directory repeated) |
| Granite-4.0-h-tiny | 4G | Q4_K_M | 0/6 | 17.5 | Immediate EOS — generated 0 response tokens |
| Ministral-3-3B | 3.5G | Q8_0 | 1/6 | 21.2 | After regex fix: T1 PASS, T2-T4 FAIL (buggy Python), T5-T6 not attempted (web_fetch hang). See retest section below |

## SKIPPED

| Model | Size | Reason |
|-------|------|--------|
| Qwen3-Coder-30B-A3B-1M-UD | 36G Q8_K_XL | Exceeds 24GB VRAM |
| Qwen3-Coder-Next | 46G Q4_K_M | Exceeds 24GB VRAM |

## Retest with Corrected Configs (2026-02-24)

Fixed preset configs for 4 models (wrong temp/top_p/top_k from initial testing). Added Mistral tool tags for Ministral-3-3B.

| Model | Config Fix | Retest Result | Notes |
|-------|-----------|---------------|-------|
| Ministral-3-3B | Added preset (temp=0.1, top_p=0.95) + Mistral tool tags | **Still FAIL** | Immediate EOS (0 tokens generated). No chat template in GGUF. Model too small for agentic tasks |
| GLM-4.7-Flash | temp 0.8→1.0, top_p 0.6→0.95, top_k 2→40 | **FAIL (0/6 written)** | Attempted all 6 tests, read files correctly, but `echo` write commands malformed (`<` instead of `>`, `<&1"` loop). Context exhausted at 16384. Worse than previous 4/6 — higher temp increased repetition tendency |
| GLM-4.7-Flash (agentic) | temp 1.0→0.7, top_p 0.95→1.0, top_k 40→0, min_p=0.01, ctx 16384→32768 | **NEAR-PASS (5.5/6)** | All 6 tests completed, all 6 files written. T1-T2: perfect. T3: 3/4 correct (avg salary math error). T4: excellent analysis. T5: 8 entries correct (minor JSON syntax). T6: excellent API docs. Config based on official Z.ai agentic/SWE benchmark params |
| Ministral-3-14B-Reasoning | temp 1.0→0.7, ctx 16384→32768 | **PASS (6/6)** | All 6 tests perfect. T3 avg salary correct ($127,875). T5 perfect JSON syntax. Used Mistral bracket format. Wrote to `results/` instead of `agent-tests/results/` (minor path issue). Reasoning model with `[THINK]`/`[/THINK]` tokens. Previously 1/6 — context exhaustion was the main issue |
| MiniCPM4.1-8B | temp 0.6→0.7, top_p 0.95→0.7 | **SKIP** | BF16 format fails to load ("null result from llama cpp"). llama-cpp-2 v0.1.122 may not support BF16 tensors |
| Nemotron-3-Nano | temp 1.0→0.6, top_p 1.0→0.95 | **SKIP** | 24.5GB Q4_K_M on 24GB VRAM → partial RAM offload (~4 tok/s). Not practical for 6-test suite |

**Conclusion:** Config fixes DO matter for capable models. Ministral-3-14B-Reasoning jumped from 1/6 → 6/6 just by fixing temp (1.0→0.7) and context (16384→32768). GLM-4.7-Flash jumped from 4/6 → 5.5/6 with official agentic config. However, fundamentally broken models (no tool training, too small, wrong format) don't improve with config changes alone.

## Retest after EXEC_PATTERN Regex Fix (2026-02-24)

Fixed closing-tag regex in `command_executor.rs`, `useMessageParsing.ts`, `toolSpanCollectors.ts` — models that mirror the opening tag as `<||SYSTEM.EXEC||>` (instead of `<SYSTEM.EXEC||>`) were silently not executing tool calls.

| Model | Previous Result | Retest Result | Score | Tok/s | Notes |
|-------|----------------|---------------|-------|-------|-------|
| Ministral-3-3B | FAIL (0/6, "Immediate EOS") | **PARTIAL** | 1/6 | 21.2 | T1: PASS (good summary). T2: FAIL (regex extraction got invoice_number only, items/amounts empty). T3: FAIL (Python error). T4: FAIL (used `yield` as variable name — reserved keyword, missing `import json`). T5-T6: not attempted (stuck on hallucinated `web_fetch` to api.weather.yandex.com that blocked headless Chrome indefinitely). Wrote to `results/` instead of `agent-tests/results/`. Mixed format: SYSTEM.EXEC + Mistral bracket `[TOOL_CALLS]execute_python[ARGS]{...}` |

**Root cause of previous 0-token failure:** NOT model inability — was two bugs in our system:
1. Non-existent `conversation_id` in WebSocket caused silent `GenerationResult::Error` (0 tokens, no error message)
2. EXEC_PATTERN regex didn't match `<||SYSTEM.EXEC||>` closing tag (model mirrored opening `<||` prefix)

**Findings:**
- Ministral-3-3B CAN make tool calls after regex fix (21.2 tok/s, fast)
- But generates buggy Python (reserved keywords, missing imports) and hallucinated tools (web_fetch to random APIs)
- Headless Chrome `web_fetch` has no global timeout — blocked indefinitely when Chrome hung. Needs investigation.
- Model is usable for simple tool calls (read_file, write_file) but unreliable for multi-step reasoning

## Web Search Test Results (2026-02-24)

Test prompt: "Use the web_search tool to search for 'Rust programming language latest version 2025' and summarize what you find. Then use web_fetch to fetch https://www.rust-lang.org and tell me what the current stable Rust version is."

| Model | web_search | web_fetch | Tokens | Tok/s | Result | Notes |
|-------|-----------|-----------|--------|-------|--------|-------|
| Devstral-Small-2-2512 | PASS | PASS | 4542 | 4.2 | **PASS** | Full summary, identified Rust 1.93.1. Mistral bracket format |
| Devstral-Small-2507 | PASS | PASS | 3161 | ~2 | **PASS** | Summary after search ("Rust 1.89.0"), no summary after fetch |
| Magistral-Small-2509 | PASS | PASS | 4608 | 5.7 | **PASS** | Both tools executed. Hit token limit before finishing summary. Required bare-JSON format fix (`[TOOL_CALLS]{"name":"..."}`) |
| Qwen3-Coder-30B-A3B-1M | PASS | PASS | 3375 | — | **PASS** | Both tools executed with commentary. No final summary after fetch. Llama3 XML format |
| GLM-4.7-Flash | PASS | — | 852 | — | **PARTIAL** | web_search worked, model stopped after results without calling web_fetch |

**Bug found & fixed:** Magistral-Small-2509 emits `[TOOL_CALLS]{"name":"web_search","arguments":{...}}` (bare JSON after tag, no `name[ARGS]` separator). Added `detect_mistral_json()` in backend (`command_executor.rs`) and corresponding frontend parsers (`toolParser.ts`, `toolSpanCollectors.ts`, `useMessageParsing.ts`). Also refactored Mistral parsing into helper functions to fix ESLint complexity warnings.

## Key Findings

1. **Mistral-family (including reasoning models) and Qwen3-Coder reliably pass all 6 tests.** 5 models now at 6/6: Devstral-Small-2, Devstral-Small, Magistral-Small, Qwen3-Coder-30B, Ministral-3-14B-Reasoning. All have strong tool-calling training.
2. **Minimum viable size is ~14B parameters** for agentic tasks. All models under 14B failed.
3. **Tool format matters:** Models must be trained to emit a recognized tool call format (Mistral bracket/comma, Qwen XML/JSON, or standard JSON). Models without this training (Gemma, GPT-OSS, MiniCPM, Granite) cannot perform agentic tasks regardless of size.
4. **Thinking mode is a liability:** GLM and Qwen3 models emit `<think>` tags that waste context and can leak into tool output, corrupting results.
5. **Python-via-tool is fragile:** Models that try to solve tasks by generating Python scripts (Ministral-14B, Nemotron, GLM-4.6V) frequently generate buggy code and burn through context on retries.
6. **MoE models can be slow if VRAM-constrained:** Nemotron-3-Nano (23GB on 24GB VRAM) ran at ~4 tok/s due to partial RAM offload.
7. **Context window size is critical:** Models with only 16K context (Ministral-3B, Qwen3-8B) exhaust tokens before completing all 6 tests.
8. **Sampling params matter for borderline models:** GLM-4.7-Flash jumped from 4/6 to 5.5/6 with official agentic config (temp=0.7, top_p=1.0, min_p=0.01). However, fundamentally broken models (no tool training, too small) don't improve with config changes alone.
9. **BF16 tensors unsupported:** llama-cpp-2 v0.1.122 cannot load BF16 GGUF files (MiniCPM4.1-8B). Only standard quantizations (Q4_K_M, Q6_K, Q8_0, etc.) work.
10. **Closing tag mirroring:** Some models mirror the opening tag format for closing tags (e.g., `<||SYSTEM.EXEC||>` instead of `<SYSTEM.EXEC||>`). The regex must handle both formats or tool calls silently fail with 0 tokens generated.
11. **Web search works across all passing models:** 4/5 tested models successfully executed both `web_search` and `web_fetch` tools. DuckDuckGo is the default search provider (no Chrome needed). `web_fetch` uses headless Chrome with ureq fallback.
12. **Mistral format has 4+ sub-variants:** Even within the Mistral family, tool call format varies: bracket (`name[ARGS]{json}`), comma (`name,{json}`), closed-tag JSON array, and bare JSON (`{"name":"...","arguments":{...}}`). Each needs separate detection.
