# Cloud Providers

All OpenAI-compatible providers support the agentic tool loop (read files, write files, run commands, web search, etc.).

## Test Checklist

| # | Provider | Status | Simple Chat | Agentic (tool calls) | Notes |
|---|----------|--------|-------------|----------------------|-------|
| 1 | Groq | PASS | PASS | PASS (3 tools) | Tested 2026-03-25 |
| 2 | Claude Code | PASS | PASS | PASS (2 tools) | Tested 2026-03-25 |
| 3 | Gemini | PASS | PASS (2.5-flash) | PASS (2 tools) | Tested 2026-03-25. 2.0-flash quota exhausted on free tier |
| 4 | SambaNova | PASS | PASS (Llama-3.3-70B) | PASS (DeepSeek-V3.2, 2 tools) | Tested 2026-03-25. Models updated to current catalog |
| 5 | Cerebras | PASS | PASS (llama3.1-8b) | PASS (qwen-235b, 2 tools) | Tested 2026-03-25. Models: qwen-3-235b, llama3.1-8b |
| 6 | OpenRouter | - | - | - | |
| 7 | Together AI | - | - | - | |
| 8 | DeepSeek | - | - | - | |
| 9 | Mistral AI | PASS | PASS (mistral-small, 840ms) | PASS (2 tools, 2.6s) | Tested 2026-03-25 |
| 10 | Fireworks AI | - | - | - | |
| 11 | xAI (Grok) | NO FREE | 403 (no credits) | - | No free tier, requires purchase |
| 12 | NVIDIA NIM | - | - | - | |
| 13 | Hugging Face | - | - | - | |
| 14 | Cloudflare | - | - | - | |
| 15 | Codex CLI | PASS | PASS (gpt-5) | PASS (tools) | Tested 2026-03-26. Windows fix: node+codex.js |
| 16 | Custom OpenAI | - | - | - | User-provided endpoint |

## Provider Details

### 1. Groq
- **Website**: https://console.groq.com
- **API Base**: `https://api.groq.com/openai/v1`
- **Env Var**: `GROQ_API_KEY`
- **Free Tier**: Yes — generous free tier with rate limits
- **Models**: `llama-3.3-70b-versatile`, `llama-3.1-8b-instant`, `mixtral-8x7b-32768`
- **Tool Calling**: Yes

### 2. Claude Code (CLI-backed)
- **Website**: https://claude.ai
- **Requires**: Claude Code CLI installed (`claude` command)
- **Free Tier**: Included with Claude Pro/Max subscription
- **Models**: `opus`, `sonnet`, `haiku`
- **Tool Calling**: Yes (native Claude tools)

### 3. Gemini
- **Website**: https://aistudio.google.com
- **API Base**: `https://generativelanguage.googleapis.com/v1beta/openai`
- **Env Var**: `GEMINI_API_KEY`
- **Free Tier**: Yes — free tier with rate limits
- **Models**: `gemini-2.5-flash`, `gemini-2.0-flash`
- **Tool Calling**: Yes

### 4. SambaNova
- **Website**: https://cloud.sambanova.ai
- **API Base**: `https://api.sambanova.ai/v1`
- **Env Var**: `SAMBANOVA_API_KEY`
- **Free Tier**: Yes — free tier available
- **Models**: `Meta-Llama-3.1-405B-Instruct`, `Meta-Llama-3.1-70B-Instruct`
- **Tool Calling**: Yes

### 5. Cerebras
- **Website**: https://cloud.cerebras.ai
- **API Base**: `https://api.cerebras.ai/v1`
- **Env Var**: `CEREBRAS_API_KEY`
- **Free Tier**: Yes — free tier with rate limits
- **Models**: `qwen-3-235b-a22b-instruct-2507`, `llama3.1-8b`
- **Tool Calling**: Yes

### 6. OpenRouter
- **Website**: https://openrouter.ai
- **API Base**: `https://openrouter.ai/api/v1`
- **Env Var**: `OPENROUTER_API_KEY`
- **Free Tier**: Some free models available
- **Models**: `auto` (routes to best available), or specify any model ID
- **Tool Calling**: Depends on model

### 7. Together AI
- **Website**: https://api.together.ai
- **API Base**: `https://api.together.xyz/v1`
- **Env Var**: `TOGETHER_API_KEY`
- **Free Tier**: Free credits on signup
- **Models**: `meta-llama/Llama-3.3-70B-Instruct-Turbo`
- **Tool Calling**: Yes

### 8. DeepSeek
- **Website**: https://platform.deepseek.com
- **API Base**: `https://api.deepseek.com/v1`
- **Env Var**: `DEEPSEEK_API_KEY`
- **Free Tier**: Free credits on signup
- **Models**: `deepseek-chat`, `deepseek-reasoner`
- **Tool Calling**: Yes

### 9. Mistral AI
- **Website**: https://console.mistral.ai
- **API Base**: `https://api.mistral.ai/v1`
- **Env Var**: `MISTRAL_API_KEY`
- **Free Tier**: Yes — free "Experiment" plan with rate limits
- **Models**: `mistral-small-latest`, `mistral-large-latest`, `codestral-latest`, `open-mistral-nemo`
- **Tool Calling**: Yes

### 10. Fireworks AI
- **Website**: https://fireworks.ai
- **API Base**: `https://api.fireworks.ai/inference/v1`
- **Env Var**: `FIREWORKS_API_KEY`
- **Free Tier**: Free credits on signup
- **Models**: `accounts/fireworks/models/llama-v3p3-70b-instruct`, `accounts/fireworks/models/qwen2p5-72b-instruct`
- **Tool Calling**: Yes

### 11. xAI (Grok)
- **Website**: https://console.x.ai
- **API Base**: `https://api.x.ai/v1`
- **Env Var**: `XAI_API_KEY`
- **Free Tier**: $25/month free API credits (verify current availability)
- **Models**: `grok-2`, `grok-2-mini`
- **Tool Calling**: Yes

### 12. NVIDIA NIM
- **Website**: https://build.nvidia.com
- **API Base**: `https://integrate.api.nvidia.com/v1`
- **Env Var**: `NVIDIA_API_KEY`
- **Free Tier**: Yes — 1000 free API calls/day
- **Models**: `meta/llama-3.1-70b-instruct`, `mistralai/mistral-large-2-instruct`
- **Tool Calling**: Yes (select models)

### 13. Hugging Face
- **Website**: https://huggingface.co/settings/tokens
- **API Base**: `https://router.huggingface.co/v1`
- **Env Var**: `HF_TOKEN`
- **Free Tier**: Yes — free tier with rate limits
- **Models**: `meta-llama/Llama-3.1-70B-Instruct`, `mistralai/Mistral-7B-Instruct-v0.3`
- **Tool Calling**: Some models via TGI

### 14. Cloudflare Workers AI
- **Website**: https://dash.cloudflare.com
- **API Base**: `https://api.cloudflare.com/client/v4/accounts/{ACCOUNT_ID}/ai/v1` (set as Custom Base URL)
- **Env Var**: `CLOUDFLARE_API_TOKEN`
- **Free Tier**: Yes — 10,000 neurons/day
- **Models**: `@cf/meta/llama-3.1-8b-instruct`, `@cf/mistral/mistral-7b-instruct-v0.2`
- **Tool Calling**: Limited
- **Note**: Requires Cloudflare account ID in the base URL

### 15. Codex CLI (CLI-backed)
- **Website**: https://github.com/openai/codex
- **Requires**: Codex CLI installed (`codex` command)
- **Models**: `gpt-5`
- **Tool Calling**: Yes (native Codex tools)

### 16. Custom OpenAI-Compatible
- **API Base**: User-provided (e.g. `http://localhost:11434/v1` for Ollama)
- **Free Tier**: Depends on endpoint
- **Models**: User-provided
- **Tool Calling**: Depends on endpoint
- **Use for**: vLLM, Ollama, LM Studio, text-generation-webui, or any OpenAI-compatible server

## Test Procedure

For each provider:

1. **Simple chat**: Send "What is 2+2?" — verify response streams correctly
2. **Agentic task**: Send "List the files in E:/repo/tmp_project and read hello.txt" — verify tool calls execute and results return
3. **Check**: Response displays in UI, cost/tokens shown if available, no errors in console
