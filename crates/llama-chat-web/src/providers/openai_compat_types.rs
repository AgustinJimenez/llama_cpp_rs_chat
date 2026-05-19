//! Response deserialization types for the OpenAI-compatible provider.
//!
//! Structs and enums used to parse the SSE streaming JSON from the API,
//! plus the `ProviderPreset` registry and `StreamResult` summary type.

use serde::{Deserialize, Serialize};

// ── Streaming data structures ──────────────────────────────────────────────

/// OpenAI chat completion chunk (streaming response)
#[derive(Debug, Deserialize)]
pub(super) struct ChatCompletionChunk {
    #[serde(default)]
    pub choices: Vec<ChunkChoice>,
    pub model: Option<String>,
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChunkChoice {
    pub delta: Option<Delta>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Delta {
    pub content: Option<String>,
    pub reasoning_content: Option<String>,
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ToolCallDelta {
    #[allow(dead_code)]
    pub index: Option<u32>,
    pub id: Option<String>,
    pub function: Option<FunctionDelta>,
}

#[derive(Debug, Deserialize)]
pub(super) struct FunctionDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CompletionTokensDetails {
    pub reasoning_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct UsageInfo {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    /// DeepSeek: cached input tokens (90% cheaper)
    pub prompt_cache_hit_tokens: Option<u64>,
    /// DeepSeek: non-cached input tokens (used for logging, not costing)
    #[allow(dead_code)]
    pub prompt_cache_miss_tokens: Option<u64>,
    /// OpenAI-style cached tokens (nested in prompt_tokens_details)
    #[serde(default)]
    pub prompt_tokens_details: Option<serde_json::Value>,
    /// Reasoning/thinking token breakdown
    pub completion_tokens_details: Option<CompletionTokensDetails>,
}

/// A fully-accumulated tool call from streaming deltas.
#[derive(Debug, Clone)]
pub(super) struct AccumulatedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Result of streaming one SSE response from the API.
pub(super) struct StreamResult {
    /// Text content produced by the model (may be empty if only tool calls).
    pub content: String,
    /// Reasoning/thinking content from reasoning models (e.g. deepseek-reasoner).
    pub reasoning_content: Option<String>,
    /// Accumulated tool calls (empty if the model produced only text).
    pub tool_calls: Vec<AccumulatedToolCall>,
    /// Model ID reported by the API.
    pub actual_model: Option<String>,
    /// Finish reason from the API.
    pub finish_reason: Option<String>,
    /// Token usage from this iteration.
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// Cached input tokens (DeepSeek prompt_cache_hit_tokens or OpenAI cached_tokens)
    pub cached_tokens: Option<u64>,
    /// Reasoning/thinking tokens (separate from content tokens)
    pub reasoning_tokens: Option<u64>,
}

// ── Provider presets ───────────────────────────────────────────────────────

/// Known provider presets with their base URLs and default models.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub base_url: &'static str,
    pub description: &'static str,
    pub models: &'static [&'static str],
    /// Environment variable name that may contain the API key.
    pub env_key: &'static str,
}

pub const PROVIDER_PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        id: "groq",
        name: "Groq",
        base_url: "https://api.groq.com/openai/v1",
        description: "Ultra-fast inference (Groq LPU)",
        models: &["llama-3.3-70b-versatile", "llama-3.1-8b-instant", "mixtral-8x7b-32768"],
        env_key: "GROQ_API_KEY",
    },
    ProviderPreset {
        id: "gemini",
        name: "Gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        description: "Google Gemini via OpenAI-compatible API",
        models: &["gemini-2.5-flash", "gemini-2.0-flash"],
        env_key: "GEMINI_API_KEY",
    },
    ProviderPreset {
        id: "sambanova",
        name: "SambaNova",
        base_url: "https://api.sambanova.ai/v1",
        description: "SambaNova Cloud inference",
        models: &["DeepSeek-V3.2", "Meta-Llama-3.3-70B-Instruct", "Qwen3-235B", "Llama-4-Maverick-17B-128E-Instruct"],
        env_key: "SAMBANOVA_API_KEY",
    },
    ProviderPreset {
        id: "cerebras",
        name: "Cerebras",
        base_url: "https://api.cerebras.ai/v1",
        description: "Cerebras fast inference",
        models: &["qwen-3-235b-a22b-instruct-2507", "llama3.1-8b"],
        env_key: "CEREBRAS_API_KEY",
    },
    ProviderPreset {
        id: "openrouter",
        name: "OpenRouter",
        base_url: "https://openrouter.ai/api/v1",
        description: "Access 100+ models via OpenRouter",
        models: &["auto"],
        env_key: "OPENROUTER_API_KEY",
    },
    ProviderPreset {
        id: "together",
        name: "Together AI",
        base_url: "https://api.together.xyz/v1",
        description: "Together AI inference",
        models: &["meta-llama/Llama-3.3-70B-Instruct-Turbo"],
        env_key: "TOGETHER_API_KEY",
    },
    ProviderPreset {
        id: "deepseek",
        name: "DeepSeek",
        base_url: "https://api.deepseek.com",
        description: "DeepSeek AI models",
        models: &["deepseek-v4-flash", "deepseek-v4-pro"],
        env_key: "DEEPSEEK_API_KEY",
    },
    ProviderPreset {
        id: "mistral",
        name: "Mistral AI",
        base_url: "https://api.mistral.ai/v1",
        description: "Mistral AI models with tool calling",
        models: &["mistral-small-latest", "mistral-large-latest", "codestral-latest", "open-mistral-nemo"],
        env_key: "MISTRAL_API_KEY",
    },
    ProviderPreset {
        id: "fireworks",
        name: "Fireworks AI",
        base_url: "https://api.fireworks.ai/inference/v1",
        description: "Fast inference on open-weight models",
        models: &["accounts/fireworks/models/llama-v3p3-70b-instruct", "accounts/fireworks/models/qwen2p5-72b-instruct"],
        env_key: "FIREWORKS_API_KEY",
    },
    ProviderPreset {
        id: "xai",
        name: "xAI (Grok)",
        base_url: "https://api.x.ai/v1",
        description: "xAI Grok models with tool calling",
        models: &["grok-2", "grok-2-mini"],
        env_key: "XAI_API_KEY",
    },
    ProviderPreset {
        id: "nvidia",
        name: "NVIDIA NIM",
        base_url: "https://integrate.api.nvidia.com/v1",
        description: "NVIDIA hosted inference (free daily limit)",
        models: &["meta/llama-3.1-70b-instruct", "mistralai/mistral-large-2-instruct"],
        env_key: "NVIDIA_API_KEY",
    },
    ProviderPreset {
        id: "huggingface",
        name: "Hugging Face",
        base_url: "https://router.huggingface.co/v1",
        description: "Hugging Face Inference API (free tier)",
        models: &["meta-llama/Llama-3.1-70B-Instruct", "mistralai/Mistral-7B-Instruct-v0.3"],
        env_key: "HF_TOKEN",
    },
    ProviderPreset {
        id: "cloudflare",
        name: "Cloudflare Workers AI",
        base_url: "",
        description: "Cloudflare Workers AI (free 10K neurons/day)",
        models: &["@cf/meta/llama-3.1-8b-instruct", "@cf/mistral/mistral-7b-instruct-v0.2"],
        env_key: "CLOUDFLARE_API_TOKEN",
    },
    ProviderPreset {
        id: "glm",
        name: "GLM (Zhipu AI)",
        base_url: "https://api.z.ai/api/paas/v4",
        description: "GLM models by Zhipu AI ($3-15/mo coding plan)",
        models: &["glm-5", "glm-4.7", "glm-4.6", "glm-4.5-air"],
        env_key: "GLM_API_KEY",
    },
    ProviderPreset {
        id: "kimi",
        name: "Kimi (Moonshot)",
        base_url: "https://api.moonshot.cn/v1",
        description: "Kimi K2.5 by Moonshot AI (auto context caching)",
        models: &["kimi-k2.5", "moonshot-v1-auto"],
        env_key: "KIMI_API_KEY",
    },
    ProviderPreset {
        id: "custom_openai",
        name: "Custom OpenAI-Compatible",
        base_url: "",
        description: "Any OpenAI-compatible endpoint (vLLM, Ollama, etc.)",
        models: &[],
        env_key: "",
    },
];
