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
  context_length: string;
  file_path: string;
  estimated_layers?: number;  // Estimated total layers based on model size
}