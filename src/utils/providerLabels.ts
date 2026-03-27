/** Human-readable labels for provider IDs. */
export const PROVIDER_LABELS: Record<string, string> = {
  claude_code: 'Claude', codex: 'Codex', groq: 'Groq', gemini: 'Gemini',
  sambanova: 'SambaNova', cerebras: 'Cerebras', openrouter: 'OpenRouter',
  together: 'Together', deepseek: 'DeepSeek', mistral: 'Mistral',
  fireworks: 'Fireworks', xai: 'Grok', nvidia: 'NVIDIA NIM',
  huggingface: 'Hugging Face', cloudflare: 'Cloudflare', custom_openai: 'Custom',
};

/** Get display label for a provider ID. Falls back to the raw ID. */
export function getProviderLabel(id: string): string {
  return PROVIDER_LABELS[id] || id;
}
