export interface Message {
  id: string;
  role: 'user' | 'assistant' | 'system';
  content: string;
  timestamp: number;
}

export interface ChatRequest {
  message: string;
  conversation_id?: string;
}

export interface ChatResponse {
  message: Message;
  conversation_id: string;
  tokens_used?: number;
  max_tokens?: number;
}

export interface SamplerConfig {
  sampler_type: string;
  temperature: number;
  top_p: number;
  top_k: number;
  mirostat_tau: number;
  mirostat_eta: number;
  model_path?: string;
  system_prompt?: string;
  context_size?: number;
  gpu_layers?: number;  // Number of layers to offload to GPU
}

export type SamplerType = 
  | 'Greedy'
  | 'Temperature' 
  | 'Mirostat'
  | 'TopP'
  | 'TopK'
  | 'Typical'
  | 'MinP'
  | 'TempExt'
  | 'ChainTempTopP'
  | 'ChainTempTopK'
  | 'ChainFull';

export interface FileItem {
  name: string;
  path: string;
  is_directory: boolean;
  size?: number;
}

export interface BrowseFilesResponse {
  files: FileItem[];
  current_path: string;
  parent_path?: string;
}

export interface ModelMetadata {
  name: string;
  architecture: string;
  parameters: string;
  quantization: string;
  file_size: string;
  file_size_gb?: number;  // File size in GB for calculations
  context_length: string;
  file_path: string;
  estimated_layers?: number;  // Estimated total layers based on model size
  tool_format?: ToolFormat;  // Detected tool calling format

  // Core model information
  general_name?: string;
  author?: string;
  version?: string;
  organization?: string;
  description?: string;
  license?: string;
  url?: string;
  repo_url?: string;
  file_type?: string;
  quantization_version?: string;

  // Architecture details (structured for memory calculations)
  architecture_details?: {
    block_count?: number;  // Total layer count
    embedding_length?: number;  // Embedding dimension
    feed_forward_length?: number;
    attention_head_count?: number;
    attention_head_count_kv?: number;  // KV heads for memory calculation
    layer_norm_epsilon?: number;
    rope_dimension_count?: number;
    rope_freq_base?: number;
  };

  // Legacy architecture details (string format)
  embedding_length?: string;
  block_count?: string;  // Actual layer count
  feed_forward_length?: string;
  attention_head_count?: string;
  attention_head_count_kv?: string;
  layer_norm_epsilon?: string;
  rope_dimension_count?: string;
  rope_freq_base?: string;

  // Tokenizer information
  tokenizer_model?: string;
  bos_token_id?: string;
  eos_token_id?: string;
  padding_token_id?: string;
  chat_template?: string;
  default_system_prompt?: string;  // Extracted from chat_template

  // All GGUF metadata (raw key-value pairs)
  gguf_metadata?: Record<string, any>;
}

// Tool calling types
export type ToolFormat = 'mistral' | 'llama3' | 'openai' | 'qwen' | 'unknown';

export interface ToolParameter {
  name: string;
  type: 'string' | 'number' | 'boolean' | 'object' | 'array';
  description: string;
  required: boolean;
  default?: any;
}

export interface ToolDefinition {
  name: string;
  description: string;
  parameters: ToolParameter[];
}

export interface ToolCall {
  id: string;
  name: string;
  arguments: Record<string, any>;
}

export interface ToolResult {
  id: string;
  name: string;
  result: string;
  error?: string;
}

export type ViewMode = 'text' | 'markdown';