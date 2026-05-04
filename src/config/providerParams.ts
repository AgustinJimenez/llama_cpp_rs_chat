/**
 * Provider-specific parameter schemas.
 *
 * Each provider has a set of supported parameters with their types, defaults,
 * and constraints. The UI dynamically renders controls based on these schemas.
 * Parameters not listed for a provider are hidden from the UI.
 */

export interface ParamSchema {
  key: string;
  label: string;
  type: 'number' | 'boolean' | 'select' | 'string';
  description?: string;
  default?: number | string | boolean;
  min?: number;
  max?: number;
  step?: number;
  options?: { value: string; label: string }[];
  /** If true, this param is added/removed by the user (not shown by default) */
  optional?: boolean;
}

export interface ProviderParamSchema {
  /** Provider ID (matches backend preset id) */
  providerId: string;
  /** Human-readable provider name */
  name: string;
  /** Parameters this provider supports */
  params: ParamSchema[];
}

// Common params shared by most OpenAI-compatible providers
const COMMON_PARAMS: ParamSchema[] = [
  {
    key: 'temperature',
    label: 'Temperature',
    type: 'number',
    description: 'Controls randomness. Higher = more creative, lower = more focused.',
    default: 0.7,
    min: 0,
    max: 2,
    step: 0.1,
  },
  {
    key: 'top_p',
    label: 'Top P',
    type: 'number',
    description: 'Nucleus sampling. 0.1 = only top 10% probability tokens.',
    default: 1,
    min: 0,
    max: 1,
    step: 0.05,
  },
  {
    key: 'max_tokens',
    label: 'Max Tokens',
    type: 'number',
    description: 'Maximum tokens in the response.',
    default: 8192,
    min: 1,
    max: 128000,
    step: 256,
  },
  {
    key: 'frequency_penalty',
    label: 'Frequency Penalty',
    type: 'number',
    description: 'Penalize tokens based on frequency in text so far.',
    default: 0,
    min: -2,
    max: 2,
    step: 0.1,
    optional: true,
  },
  {
    key: 'presence_penalty',
    label: 'Presence Penalty',
    type: 'number',
    description: 'Penalize tokens that have appeared in text so far.',
    default: 0,
    min: -2,
    max: 2,
    step: 0.1,
    optional: true,
  },
];

// Thinking/reasoning params
const THINKING_TOGGLE: ParamSchema = {
  key: 'thinking',
  label: 'Thinking Mode',
  type: 'select',
  description: 'Enable extended thinking/reasoning before responding.',
  default: 'enabled',
  options: [
    { value: 'enabled', label: 'Enabled' },
    { value: 'disabled', label: 'Disabled' },
  ],
};

const THINKING_ADAPTIVE: ParamSchema = {
  key: 'thinking',
  label: 'Thinking Mode',
  type: 'select',
  description: 'Control when the model uses extended thinking.',
  default: 'adaptive',
  options: [
    { value: 'adaptive', label: 'Adaptive (auto)' },
    { value: 'enabled', label: 'Always On' },
    { value: 'disabled', label: 'Disabled' },
  ],
};

const REASONING_EFFORT_DEEPSEEK: ParamSchema = {
  key: 'reasoning_effort',
  label: 'Reasoning Effort',
  type: 'select',
  description: 'How deeply the model reasons before answering.',
  default: 'high',
  options: [
    { value: 'high', label: 'High' },
    { value: 'max', label: 'Max' },
  ],
};

const REASONING_EFFORT_OPENAI: ParamSchema = {
  key: 'reasoning_effort',
  label: 'Reasoning Effort',
  type: 'select',
  description: 'Constrains effort on reasoning. Lower = faster, fewer tokens.',
  default: 'medium',
  options: [
    { value: 'low', label: 'Low' },
    { value: 'medium', label: 'Medium' },
    { value: 'high', label: 'High' },
  ],
};

const BUDGET_TOKENS: ParamSchema = {
  key: 'budget_tokens',
  label: 'Thinking Budget',
  type: 'number',
  description: 'Max tokens for extended thinking (1024-128000).',
  default: 10000,
  min: 1024,
  max: 128000,
  step: 1024,
};

const RESPONSE_FORMAT: ParamSchema = {
  key: 'response_format',
  label: 'Response Format',
  type: 'select',
  description: 'Force structured output format.',
  default: 'text',
  options: [
    { value: 'text', label: 'Text' },
    { value: 'json_object', label: 'JSON' },
  ],
  optional: true,
};

// Provider-specific schemas
export const PROVIDER_PARAMS: Record<string, ProviderParamSchema> = {
  deepseek: {
    providerId: 'deepseek',
    name: 'DeepSeek',
    params: [
      THINKING_TOGGLE,
      REASONING_EFFORT_DEEPSEEK,
      ...COMMON_PARAMS.filter((p) => !['frequency_penalty', 'presence_penalty'].includes(p.key)),
      RESPONSE_FORMAT,
    ],
  },
  openai: {
    providerId: 'openai',
    name: 'OpenAI',
    params: [...COMMON_PARAMS, RESPONSE_FORMAT],
  },
  // OpenAI reasoning models (o3, o4-mini) have limited params
  openai_reasoning: {
    providerId: 'openai_reasoning',
    name: 'OpenAI Reasoning (o3/o4)',
    params: [
      REASONING_EFFORT_OPENAI,
      {
        key: 'max_completion_tokens',
        label: 'Max Completion Tokens',
        type: 'number',
        description: 'Maximum tokens for reasoning models (replaces max_tokens).',
        default: 16384,
        min: 1,
        max: 128000,
        step: 256,
      },
    ],
  },
  anthropic: {
    providerId: 'anthropic',
    name: 'Anthropic (Claude)',
    params: [
      THINKING_ADAPTIVE,
      BUDGET_TOKENS,
      ...COMMON_PARAMS.filter((p) => !['frequency_penalty', 'presence_penalty'].includes(p.key)),
    ],
  },
  groq: {
    providerId: 'groq',
    name: 'Groq',
    params: [...COMMON_PARAMS],
  },
  gemini: {
    providerId: 'gemini',
    name: 'Gemini',
    params: [...COMMON_PARAMS],
  },
  mistral: {
    providerId: 'mistral',
    name: 'Mistral AI',
    params: [...COMMON_PARAMS],
  },
  xai: {
    providerId: 'xai',
    name: 'xAI (Grok)',
    params: [...COMMON_PARAMS],
  },
};

// Default params for providers not explicitly listed — use common params
export const DEFAULT_PROVIDER_PARAMS: ParamSchema[] = [...COMMON_PARAMS];

/**
 * Get the parameter schema for a provider.
 * Falls back to common params if provider has no specific schema.
 */
export function getProviderParams(providerId: string): ParamSchema[] {
  return PROVIDER_PARAMS[providerId]?.params ?? DEFAULT_PROVIDER_PARAMS;
}

/**
 * Get default values for a provider's parameters.
 */
export function getProviderDefaults(providerId: string): Record<string, unknown> {
  const params = getProviderParams(providerId);
  const defaults: Record<string, unknown> = {};
  for (const p of params) {
    if (p.default !== undefined && !p.optional) {
      defaults[p.key] = p.default;
    }
  }
  return defaults;
}
