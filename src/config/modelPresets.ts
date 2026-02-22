// Model-specific recommended sampling parameters and tool tag formats
// These are used as fallbacks when GGUF doesn't have embedded params
// Keyed by `general.name` from GGUF metadata

import type { SamplerConfig } from '@/types';

// Tool tag configuration per model
// Each model may use different tags for command execution
export interface ToolTags {
  execOpen: string;    // Opening tag before command
  execClose: string;   // Closing tag after command
  outputOpen: string;  // Opening tag before command output
  outputClose: string; // Closing tag after command output
}

// Default SYSTEM.EXEC tags (works for most models)
export const DEFAULT_TOOL_TAGS: ToolTags = {
  execOpen: '<||SYSTEM.EXEC>',
  execClose: '<SYSTEM.EXEC||>',
  outputOpen: '<||SYSTEM.OUTPUT>',
  outputClose: '<SYSTEM.OUTPUT||>',
};

// Tool tag families for different model architectures
// Models trained with specific tool formats are more likely to follow them
const TOOL_TAG_FAMILIES = {
  // Qwen models use <tool_call> tags natively
  qwen: {
    execOpen: '<tool_call>',
    execClose: '</tool_call>',
    outputOpen: '<tool_response>',
    outputClose: '</tool_response>',
  } as ToolTags,
  // Mistral models use [TOOL_CALLS] format natively
  mistral: {
    execOpen: '[TOOL_CALLS]',
    execClose: '[/TOOL_CALLS]',
    outputOpen: '[TOOL_RESULTS]',
    outputClose: '[/TOOL_RESULTS]',
  } as ToolTags,
  // GLM models use <tool_call> with <|observation|> for results
  glm: {
    execOpen: '<tool_call>',
    execClose: '</tool_call>',
    outputOpen: '<|observation|>',
    outputClose: '',
  } as ToolTags,
  // Default for models without native tool format
  default: DEFAULT_TOOL_TAGS,
};

// Map of general.name -> tool tags
// Only override for models where native tags work better than SYSTEM.EXEC
export const MODEL_TOOL_TAGS: Record<string, ToolTags> = {
  // Qwen models - strong tool calling with native tags
  "Qwen_Qwen3 Coder Next": TOOL_TAG_FAMILIES.qwen,
  "Qwen3 8B": TOOL_TAG_FAMILIES.qwen,
  "Qwen_Qwen3 30B A3B Instruct 2507": TOOL_TAG_FAMILIES.qwen,
  "Qwen3-Coder-30B-A3B-Instruct-1M": TOOL_TAG_FAMILIES.qwen,
  // Mistral models - strong tool calling with native tags
  "mistralai_Devstral Small 2507": TOOL_TAG_FAMILIES.mistral,
  "mistralai_Devstral Small 2 24B Instruct 2512": TOOL_TAG_FAMILIES.mistral,
  "Magistral-Small-2509": TOOL_TAG_FAMILIES.mistral,
  "mistralai_Ministral 3 14B Reasoning 2512": TOOL_TAG_FAMILIES.mistral,
  // GLM models - use default SYSTEM.EXEC (model doesn't follow <tool_call> closing tags)
  "Zai org_GLM 4.6V Flash": TOOL_TAG_FAMILIES.default,
  "Zai org_GLM 4.7 Flash": TOOL_TAG_FAMILIES.default,
  // Other models - use default SYSTEM.EXEC (no strong native tool format)
  // MiniCPM, Gemma, Granite, Nemotron, GPT-OSS, RNJ all use default
};

// Look up tool tags for a model by general.name
export function findToolTagsByName(generalName: string): ToolTags {
  // Exact match
  if (MODEL_TOOL_TAGS[generalName]) return MODEL_TOOL_TAGS[generalName];
  // Fuzzy match
  const normalized = generalName.toLowerCase().replace(/[_\-\s]+/g, ' ');
  for (const [key, tags] of Object.entries(MODEL_TOOL_TAGS)) {
    const normalizedKey = key.toLowerCase().replace(/[_\-\s]+/g, ' ');
    if (normalized.includes(normalizedKey) || normalizedKey.includes(normalized)) {
      return tags;
    }
  }
  return DEFAULT_TOOL_TAGS;
}

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
    repeat_penalty: 1.05,
  },
  "Qwen3 8B": {
    sampler_type: "Temperature",
    temperature: 0.7,
    top_p: 0.8,
    top_k: 20,
    repeat_penalty: 1.0,
  },
  "Qwen_Qwen3 30B A3B Instruct 2507": {
    sampler_type: "Temperature",
    temperature: 0.7,
    top_p: 0.8,
    top_k: 20,
    repeat_penalty: 1.0,
  },
  "Qwen3-Coder-30B-A3B-Instruct-1M": {
    sampler_type: "Temperature",
    temperature: 0.7,
    top_p: 0.8,
    top_k: 20,
    repeat_penalty: 1.05,
  },

  // Mistral models
  "mistralai_Devstral Small 2507": {
    sampler_type: "Temperature",
    temperature: 0.15,
    top_p: 0.95,
    top_k: 64,
    repeat_penalty: 1.0,
  },
  "mistralai_Devstral Small 2 24B Instruct 2512": {
    sampler_type: "Temperature",
    temperature: 0.15,
    top_p: 0.95,
    top_k: 64,
    repeat_penalty: 1.0,
  },
  "Magistral-Small-2509": {
    sampler_type: "Temperature",
    temperature: 0.7,
    top_p: 0.95,
    top_k: 40,
    repeat_penalty: 1.0,
  },
  "mistralai_Ministral 3 14B Reasoning 2512": {
    sampler_type: "Temperature",
    temperature: 1.0,
    top_p: 0.95,
    top_k: 40,
    repeat_penalty: 1.0,
  },

  // NVIDIA
  "Nemotron Nano 3 30B A3B": {
    sampler_type: "Temperature",
    temperature: 1.0,
    top_p: 1.0,
    top_k: 40,
    repeat_penalty: 1.0,
  },

  // Google
  "Gemma 3 12b It": {
    sampler_type: "Greedy",
    temperature: 0.0,
    top_p: 1.0,
    top_k: 1,
    repeat_penalty: 1.0,
  },

  // IBM
  "Ibm Granite_Granite 4.0 H Tiny": {
    sampler_type: "Greedy",
    temperature: 0.0,
    top_p: 1.0,
    top_k: 0,
    repeat_penalty: 1.1,
  },

  // OpenAI
  "Openai_Gpt Oss 20b": {
    sampler_type: "Temperature",
    temperature: 0.7,
    top_p: 0.95,
    top_k: 40,
    repeat_penalty: 1.0,
  },

  // OpenBMB
  "MiniCPM4.1-8B": {
    sampler_type: "Temperature",
    temperature: 0.6,
    top_p: 0.95,
    top_k: 40,
    repeat_penalty: 1.02,
  },

  // EssentialAI
  "EssentialAI_rnj 1 Instruct": {
    sampler_type: "Temperature",
    temperature: 0.2,
    top_p: 0.95,
    top_k: 40,
    repeat_penalty: 1.0,
  },

  // Microsoft Phi
  "Phi 4 Mini Reasoning": {
    sampler_type: "Temperature",
    temperature: 0.8,
    top_p: 0.95,
    top_k: 50,
    min_p: 0.0,
    repeat_penalty: 1.0,
    context_size: 32768,
  },

  // Zhipu AI GLM models
  "Zai org_GLM 4.7 Flash": {
    sampler_type: "Temperature",
    temperature: 0.8,
    top_p: 0.6,
    top_k: 2,
    repeat_penalty: 1.1,
    context_size: 16384,
  },
  "Zai org_GLM 4.6V Flash": {
    sampler_type: "Temperature",
    temperature: 0.8,
    top_p: 0.6,
    top_k: 2,
    repeat_penalty: 1.1,
    context_size: 16384,
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
  repeat_penalty: 1.0,
};
