// Model-specific recommended sampling parameters
// These are used as fallbacks when GGUF doesn't have embedded params
// Keyed by `general.name` from GGUF metadata

import type { SamplerConfig } from '@/types';

export type ModelPreset = Partial<SamplerConfig>;

// Map of general.name -> recommended params
// Sources: HuggingFace model cards, vendor documentation
export const MODEL_PRESETS: Record<string, ModelPreset> = {
  // Qwen models
  "Qwen_Qwen3 Coder Next": {
    sampler_type: "Temperature",
    temperature: 1.0,
    top_p: 0.95,
    top_k: 40,
  },
  "Qwen3 8B": {
    sampler_type: "Temperature",
    temperature: 0.7,
    top_p: 0.8,
    top_k: 20,
  },
  "Qwen_Qwen3 30B A3B Instruct 2507": {
    sampler_type: "Temperature",
    temperature: 0.7,
    top_p: 0.8,
    top_k: 20,
  },
  "Qwen3-Coder-30B-A3B-Instruct-1M": {
    sampler_type: "Temperature",
    temperature: 0.7,
    top_p: 0.8,
    top_k: 20,
  },

  // Mistral models
  "mistralai_Devstral Small 2507": {
    sampler_type: "Temperature",
    temperature: 0.15,
    top_p: 0.95,
    top_k: 64,
  },
  "mistralai_Devstral Small 2 24B Instruct 2512": {
    sampler_type: "Temperature",
    temperature: 0.15,
    top_p: 0.95,
    top_k: 64,
  },
  "Magistral-Small-2509": {
    sampler_type: "Temperature",
    temperature: 0.7,
    top_p: 0.95,
    top_k: 40,
  },
  "mistralai_Ministral 3 14B Reasoning 2512": {
    sampler_type: "Temperature",
    temperature: 1.0,
    top_p: 0.95,
    top_k: 40,
  },

  // NVIDIA
  "Nemotron Nano 3 30B A3B": {
    sampler_type: "Temperature",
    temperature: 1.0,
    top_p: 1.0,
    top_k: 40,
  },

  // Google
  "Gemma 3 12b It": {
    sampler_type: "Greedy",
    temperature: 0.0,
    top_p: 1.0,
    top_k: 1,
  },

  // IBM
  "Ibm Granite_Granite 4.0 H Tiny": {
    sampler_type: "Greedy",
    temperature: 0.0,
    top_p: 1.0,
    top_k: 0,
  },

  // OpenAI
  "Openai_Gpt Oss 20b": {
    sampler_type: "Temperature",
    temperature: 0.7,
    top_p: 0.95,
    top_k: 40,
  },

  // OpenBMB
  "MiniCPM4.1-8B": {
    sampler_type: "Temperature",
    temperature: 0.7,
    top_p: 0.7,
    top_k: 40,
  },

  // EssentialAI
  "EssentialAI_rnj 1 Instruct": {
    sampler_type: "Temperature",
    temperature: 0.2,
    top_p: 0.95,
    top_k: 40,
  },

  // Zai
  "Zai org_GLM 4.7 Flash": {
    sampler_type: "Temperature",
    temperature: 1.0,
    top_p: 0.95,
    top_k: 40,
  },
};

// Fuzzy match helper - tries to find a preset by partial name match
export function findPresetByName(generalName: string): ModelPreset | null {
  // Exact match first
  if (MODEL_PRESETS[generalName]) {
    return MODEL_PRESETS[generalName];
  }

  // Normalize the name for fuzzy matching
  const normalized = generalName.toLowerCase().replace(/[_\-\s]+/g, ' ');

  // Try partial matches
  for (const [key, preset] of Object.entries(MODEL_PRESETS)) {
    const normalizedKey = key.toLowerCase().replace(/[_\-\s]+/g, ' ');
    if (normalized.includes(normalizedKey) || normalizedKey.includes(normalized)) {
      return preset;
    }
  }

  return null;
}

// Default fallback params when no preset is found
export const DEFAULT_PRESET: ModelPreset = {
  sampler_type: "Temperature",
  temperature: 0.7,
  top_p: 0.95,
  top_k: 40,
  mirostat_tau: 5.0,
  mirostat_eta: 0.1,
};
