# Cloud Providers

Connect any OpenAI-compatible API as a provider. All providers support the full agentic tool loop.

---

## Built-in providers

| Provider | Free tier | API base |
|----------|-----------|----------|
| Groq | Yes | `https://api.groq.com/openai/v1` |
| Mistral AI | Yes (limited) | `https://api.mistral.ai/v1` |
| Google Gemini | Yes | `https://generativelanguage.googleapis.com/v1beta/openai` |
| Anthropic (Claude) | No | `https://api.anthropic.com/v1` |
| SambaNova | Yes | `https://api.sambanova.ai/v1` |
| Cerebras | Yes | `https://api.cerebras.ai/v1` |
| DeepSeek | Paid | `https://api.deepseek.com/v1` |
| OpenRouter | Pay-as-you-go | `https://openrouter.ai/api/v1` |
| Fireworks AI | Pay-as-you-go | `https://api.fireworks.ai/inference/v1` |
| xAI (Grok) | No | `https://api.x.ai/v1` |
| Custom | — | Enter any OpenAI-compat base URL |

---

## Setup

1. Open **Settings → Providers**
2. Select a provider
3. Enter your API key
4. (Optional) Enter a custom base URL for self-hosted or proxy endpoints
5. Select a model from the dropdown

---

## Custom / self-hosted

Use "Custom OpenAI" to connect any OpenAI-compatible endpoint:

- **Ollama**: `http://localhost:11434/v1`
- **LM Studio**: `http://localhost:1234/v1`
- **vLLM**: your server address
- **llama-server** (upstream llama.cpp): `http://localhost:8080/v1`

Leave the API key field blank or enter a dummy value for local endpoints.

---

## Tool calling

All providers that support function calling work with the agentic tool loop. Providers that do not support function calling fall back to parsing tool calls from text output.

See [`docs/PROVIDERS.md`](../PROVIDERS.md) for detailed per-provider test results.
