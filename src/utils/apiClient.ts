/* eslint-disable */
/**
 * Typed TypeScript API client for llama-chat-web (backend port 18080).
 *
 * Generated from the OpenAPI 3.1 spec at docs/openapi.json.
 * Uses plain fetch() — no external dependencies.
 */

// ---------------------------------------------------------------------------
// Base URL configuration
// ---------------------------------------------------------------------------

let _baseUrl = 'http://localhost:18080';

/** Override the default base URL (http://localhost:18080). */
export function setBaseUrl(url: string): void {
  _baseUrl = url.replace(/\/$/, ''); // strip trailing slash
}

export function getBaseUrl(): string {
  return _baseUrl;
}

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

export interface ErrorResponse {
  error: string;
}

export interface SuccessResponse {
  success: boolean;
}

// ---------------------------------------------------------------------------
// Chat types
// ---------------------------------------------------------------------------

export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: string;
  /** Unix timestamp in seconds. */
  timestamp: number;
  prompt_tok_per_sec?: number;
  gen_tok_per_sec?: number;
  gen_eval_ms?: number;
  gen_tokens?: number;
  prompt_eval_ms?: number;
  prompt_tokens?: number;
  /** True if this message has been compacted (summarised). */
  compacted: boolean;
  /** DB sequence order for precise truncation on edit/regenerate. */
  sequence_order?: number;
}

export interface ChatRequest {
  message: string;
  conversation_id?: string;
  /** Base64-encoded image data URIs for vision models. */
  image_data?: string[];
  /**
   * When true the user message is NOT logged to the conversation.
   * The model simply resumes an interrupted generation.
   */
  auto_continue?: boolean;
}

/** Immediate response from POST /api/chat. Real content arrives via WebSocket. */
export interface ChatResponse {
  message: ChatMessage;
  conversation_id: string;
  tokens_used?: number;
  max_tokens?: number;
}

export interface ToolTimingLive {
  name: string;
  duration_ms: number;
}

/** A single SSE token event emitted by /api/chat/stream. */
export interface TokenData {
  token: string;
  tokens_used: number;
  max_tokens: number;
  status?: string;
  gen_tok_per_sec?: number;
  gen_tokens?: number;
  tool_timing?: ToolTimingLive;
}

// ---------------------------------------------------------------------------
// Conversation types
// ---------------------------------------------------------------------------

export interface ConversationFile {
  /** Unique conversation ID (e.g. 'chat_2024-01-01-12-00-00-000'). */
  name: string;
  display_name: string;
  timestamp: string;
  title?: string;
  provider_id?: string;
}

export interface ConversationsResponse {
  conversations: ConversationFile[];
}

export interface ToolTiming {
  name: string;
  duration_ms: number;
}

export interface ConversationContentResponse {
  /** Raw conversation text (empty for remote provider conversations). */
  content: string;
  messages: ChatMessage[];
  provider_id?: string;
  provider_session_id?: string;
  tool_timings: ToolTiming[];
}

export interface TokenAnalysisBreakdownEntry {
  chars: number;
  tokens: number;
  pct: number;
}

export interface TokenAnalysisResponse {
  total_messages: number;
  compacted_messages: number;
  total_chars: number;
  total_tokens_estimate: number;
  breakdown: {
    system: TokenAnalysisBreakdownEntry;
    user: TokenAnalysisBreakdownEntry;
    assistant: TokenAnalysisBreakdownEntry;
    tool_responses: TokenAnalysisBreakdownEntry;
  };
  tool_calls: number;
}

// ---------------------------------------------------------------------------
// Model types
// ---------------------------------------------------------------------------

export interface ToolTags {
  exec_open: string;
  exec_close: string;
  output_open: string;
  output_close: string;
}

export interface ModelStatus {
  loaded: boolean;
  loading?: boolean;
  loading_progress?: number;
  generating?: boolean;
  active_conversation_id?: string;
  status_message?: string;
  model_path?: string;
  last_used?: string;
  memory_usage_mb?: number;
  has_vision?: boolean;
  tool_tags?: ToolTags;
  gpu_layers?: number;
  block_count?: number;
  system_prompt_tokens?: number;
  tool_definitions_tokens?: number;
  context_size?: number;
  /** Finish reason of the last generation: 'stop' | 'length' | 'cancelled' | 'tool_calls' | 'error'. */
  last_finish_reason?: string;
  supports_thinking?: boolean;
}

export interface ModelLoadRequest {
  model_path: string;
  gpu_layers?: number;
  mmproj_path?: string;
  context_size?: number;
  flash_attention?: boolean;
  cache_type_k?: string;
  cache_type_v?: string;
}

export interface ModelResponse {
  success: boolean;
  message: string;
  status?: ModelStatus;
}

export interface BackendDeviceInfo {
  name: string;
  description: string;
  vram_mb?: number;
}

export interface BackendInfo {
  name: string;
  available: boolean;
  devices: BackendDeviceInfo[];
}

export interface BackendsResponse {
  backends: BackendInfo[];
  nvidia_gpu_detected: boolean;
  cuda_backend_loaded: boolean;
}

export interface ModelInfo {
  name: string;
  architecture?: string;
  parameters?: string;
  quantization?: string;
  file_size?: string;
  context_length?: string;
  path: string;
  estimated_layers?: number;
  general_name?: string;
  has_vision?: boolean;
  mmproj_files?: Array<{ name: string; path: string; file_size: string }>;
  recommended_params?: Record<string, unknown>;
  gguf_metadata?: Record<string, unknown>;
  tool_format?: string;
  chat_template?: string;
  default_system_prompt?: string;
  block_count?: number;
  // Directory error case
  error?: string;
  is_directory?: boolean;
  suggestions?: string[];
}

// ---------------------------------------------------------------------------
// Sampler / Config types
// ---------------------------------------------------------------------------

export type SystemPromptType = 'Custom';

export interface TagPair {
  open: string;
  close: string;
}

export interface SamplerConfig {
  sampler_type: string;
  temperature: number;
  top_p: number;
  top_k: number;
  mirostat_tau: number;
  mirostat_eta: number;
  repeat_penalty?: number;
  min_p?: number;
  typical_p?: number;
  frequency_penalty?: number;
  presence_penalty?: number;
  penalty_last_n?: number;
  dry_multiplier?: number;
  dry_base?: number;
  dry_allowed_length?: number;
  dry_penalty_last_n?: number;
  top_n_sigma?: number;
  flash_attention?: boolean;
  cache_type_k?: string;
  cache_type_v?: string;
  n_batch?: number;
  model_path?: string;
  system_prompt?: string;
  system_prompt_type?: SystemPromptType;
  context_size?: number;
  stop_tokens?: string[];
  model_history?: string[];
  disable_file_logging?: boolean;
  safe_tool_injection?: boolean;
  tool_tag_exec_open?: string;
  tool_tag_exec_close?: string;
  tool_tag_output_open?: string;
  tool_tag_output_close?: string;
  web_browser_backend?: string;
  models_directory?: string;
  seed?: number;
  n_ubatch?: number;
  n_threads?: number;
  n_threads_batch?: number;
  rope_freq_base?: number;
  rope_freq_scale?: number;
  use_mlock?: boolean;
  use_mmap?: boolean;
  main_gpu?: number;
  split_mode?: string;
  use_htmd?: boolean;
  tag_pairs?: TagPair[];
  proactive_compaction?: boolean;
  telegram_bot_token?: string;
  telegram_chat_id?: string;
  provider_api_keys?: string;
  max_tool_calls?: number;
  loop_detection_limit?: number;
  thinking_mode?: boolean;
}

// ---------------------------------------------------------------------------
// File types
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// HuggingFace Hub types
// ---------------------------------------------------------------------------

export interface HubFile {
  name: string;
  size: number;
}

export interface HubModel {
  id: string;
  author: string;
  downloads: number;
  likes: number;
  last_modified: string;
  pipeline_tag: string;
  files: HubFile[];
}

export interface DownloadRequest {
  /** HuggingFace repo ID (e.g. 'unsloth/Llama-3.2-1B-GGUF'). */
  model_id: string;
  /** Filename within the repo (e.g. 'model.Q4_K_M.gguf'). */
  filename: string;
  /** Absolute path to local destination directory. */
  destination: string;
}

/** SSE event from the download stream. */
export type DownloadEvent =
  | { type: 'progress'; bytes: number; total: number; speed_kbps: number }
  | { type: 'done'; path: string; bytes: number }
  | { type: 'error'; message: string };

// ---------------------------------------------------------------------------
// MCP types
// ---------------------------------------------------------------------------

export type McpTransport =
  | { type: 'Stdio'; command: string; args: string[]; env_vars?: Record<string, string> }
  | { type: 'Http'; url: string };

export interface McpServerConfig {
  id: string;
  name: string;
  transport: McpTransport;
  enabled: boolean;
}

export interface McpServerStatus {
  id: string;
  name: string;
  connected: boolean;
  tool_count: number;
  tools: string[];
}

export interface McpToolsResponse {
  servers: McpServerStatus[];
}

export interface McpRefreshResponse {
  success: boolean;
  connected_servers: string[];
  total_tools: number;
}

// ---------------------------------------------------------------------------
// Provider types
// ---------------------------------------------------------------------------

export interface ProviderRequest {
  prompt: string;
  model?: string;
  max_turns?: number;
  cwd?: string;
  session_id?: string;
  conversation_id?: string;
  params?: Record<string, unknown>;
}

export interface ProviderGenerateResponse {
  response: string;
  cost_usd?: number;
  duration_ms?: number;
  stop_reason?: string;
  provider: string;
  model: string;
  session_id?: string;
  conversation_id: string;
  input_tokens?: number;
  output_tokens?: number;
}

/** SSE event from a provider stream. */
export type ProviderStreamEvent =
  | { type: 'token'; token: string }
  | {
      type: 'status';
      input_tokens?: number;
      output_tokens?: number;
      cached_tokens?: number;
      duration_ms?: number;
    }
  | {
      type: 'done';
      provider: string;
      session_id?: string;
      stop_reason?: string;
      cost_usd?: number;
      duration_ms?: number;
      input_tokens?: number;
      output_tokens?: number;
      cached_tokens?: number;
      model?: string;
      conversation_id: string;
    };

// ---------------------------------------------------------------------------
// Tool types
// ---------------------------------------------------------------------------

export interface ToolExecuteRequest {
  tool_name: string;
  arguments: Record<string, unknown>;
}

export interface ToolExecuteResponse {
  success: boolean;
  result?: string;
  error?: string;
}

export interface WebFetchResponse {
  success: boolean;
  result?: string;
  url?: string;
  status_code?: number;
  content_length?: number;
  error?: string;
  body_preview?: string;
}

export interface ExtractTextResponse {
  success: boolean;
  filename?: string;
  text?: string;
  chars?: number;
}

// ---------------------------------------------------------------------------
// System types
// ---------------------------------------------------------------------------

export interface SystemUsageResponse {
  cpu: number;
  gpu: number;
  ram: number;
  total_ram_gb: number;
  total_vram_gb: number;
  cpu_cores: number;
  cpu_ghz: number;
}

export interface BackgroundProcess {
  pid: number;
  command: string;
  conversationId?: string;
  startedAt: number;
  alive: boolean;
}

export interface AppErrorRecord {
  level: string;
  source: string;
  message: string;
  details?: string;
  timestamp?: number;
}

export interface FrontendLogEntry {
  level: 'info' | 'warn' | 'warning' | 'error' | 'debug';
  message: string;
  timestamp?: string;
}

export interface AppInfo {
  app: string;
  version: string;
  platform: string;
  arch: string;
  features: { vision: boolean; cuda: boolean };
}

export interface ProviderKeyEntry {
  api_key?: string;
  configured: boolean;
  base_url?: string;
}

export interface ActiveProviderResponse {
  provider: string;
  model?: string;
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

async function request<T>(
  method: string,
  path: string,
  body?: unknown,
  customHeaders?: Record<string, string>,
): Promise<T> {
  const url = `${_baseUrl}${path}`;
  const headers: Record<string, string> = { ...customHeaders };

  let fetchBody: BodyInit | undefined;
  if (body !== undefined) {
    if (body instanceof Uint8Array || body instanceof ArrayBuffer) {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      fetchBody = body as any;
    } else {
      headers['Content-Type'] = 'application/json';
      fetchBody = JSON.stringify(body);
    }
  }

  const res = await fetch(url, { method, headers, body: fetchBody });
  if (!res.ok) {
    let errMsg = `HTTP ${res.status}`;
    try {
      const json = await res.json();
      if (json.error) errMsg = json.error;
      else if (json.message) errMsg = json.message;
    } catch {
      // ignore parse error
    }
    throw new Error(errMsg);
  }
  return res.json() as Promise<T>;
}

function buildQuery(params: Record<string, string | number | boolean | undefined>): string {
  const parts: string[] = [];
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined) parts.push(`${encodeURIComponent(k)}=${encodeURIComponent(String(v))}`);
  }
  return parts.length > 0 ? `?${parts.join('&')}` : '';
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

/** GET /health — confirm the server is alive. */
export async function getHealth(): Promise<{ status: string; service: string }> {
  return request('GET', '/health');
}

// ---------------------------------------------------------------------------
// System
// ---------------------------------------------------------------------------

/** GET /api/info — app version, platform, and compiled features. */
export async function getAppInfo(): Promise<AppInfo> {
  return request('GET', '/api/info');
}

/** GET /api/docs — list all API endpoints. */
export async function getApiDocs(): Promise<{
  endpoints: Array<{ method: string; path: string; description: string }>;
}> {
  return request('GET', '/api/docs');
}

/** GET /api/system/usage — CPU / RAM / GPU usage. */
export async function getSystemUsage(): Promise<SystemUsageResponse> {
  return request('GET', '/api/system/usage');
}

/** GET /api/system/processes — list alive background processes. */
export async function listBackgroundProcesses(): Promise<BackgroundProcess[]> {
  return request('GET', '/api/system/processes');
}

/** POST /api/system/processes/kill — terminate a background process by PID. */
export async function killBackgroundProcess(
  pid: number,
): Promise<{ success: boolean; message: string }> {
  return request('POST', '/api/system/processes/kill', { pid });
}

/** POST /api/desktop/abort — send abort signal to desktop automation. */
export async function desktopAbort(): Promise<{ success: boolean; message: string }> {
  return request('POST', '/api/desktop/abort');
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

/** POST /api/logs/frontend — ingest frontend log entries (max 200). */
export async function postFrontendLogs(logs: FrontendLogEntry[]): Promise<SuccessResponse> {
  return request('POST', '/api/logs/frontend', { logs });
}

/** GET /api/errors — get recorded app errors. */
export async function getAppErrors(limit = 100): Promise<AppErrorRecord[]> {
  return request('GET', `/api/errors${buildQuery({ limit })}`);
}

/** POST /api/errors — record an app-level error. */
export async function recordAppError(payload: AppErrorRecord): Promise<SuccessResponse> {
  return request('POST', '/api/errors', payload);
}

/** DELETE /api/errors — clear all stored app errors. */
export async function clearAppErrors(): Promise<{ success: boolean; deleted: number }> {
  return request('DELETE', '/api/errors');
}

// ---------------------------------------------------------------------------
// Chat
// ---------------------------------------------------------------------------

/**
 * POST /api/chat — submit a user message to the local model.
 *
 * Returns immediately with the conversation_id. Real streaming content
 * arrives via the WebSocket at ws://host/ws/conversation/watch/{id}.
 */
export async function postChat(req: ChatRequest): Promise<ChatResponse> {
  return request('POST', '/api/chat', req);
}

/**
 * POST /api/chat/cancel — cancel the currently active generation.
 */
export async function cancelGeneration(): Promise<{ success: boolean; message: string }> {
  return request('POST', '/api/chat/cancel');
}

/**
 * POST /api/chat/stream — stream tokens via SSE (local model).
 *
 * Calls the callback for each token. Returns a Promise that resolves
 * when the stream ends. Rejects on network or server error.
 *
 * @example
 * await streamChat({ message: "hello" }, (tok) => console.log(tok.token));
 */
export async function streamChat(
  req: ChatRequest,
  onToken: (data: TokenData) => void,
): Promise<void> {
  const res = await fetch(`${_baseUrl}/api/chat/stream`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
  if (!res.ok || !res.body) throw new Error(`HTTP ${res.status}`);

  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buf = '';

  while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });
    const lines = buf.split('\n');
    buf = lines.pop() ?? '';
    for (const line of lines) {
      if (!line.startsWith('data: ')) continue;
      const payload = line.slice(6).trim();
      if (payload === '[DONE]') return;
      try {
        onToken(JSON.parse(payload) as TokenData);
      } catch {
        // ignore malformed events
      }
    }
  }
}

// ---------------------------------------------------------------------------
// Conversations
// ---------------------------------------------------------------------------

/** GET /api/conversations — list all conversations, optionally filtered by query. */
export async function listConversations(q?: string): Promise<ConversationsResponse> {
  return request('GET', `/api/conversations${buildQuery({ q })}`);
}

/** POST /api/conversations — create a new empty conversation. */
export async function createConversation(
  title = 'New conversation',
): Promise<{ id: string; title: string }> {
  return request('POST', '/api/conversations', { title });
}

/** DELETE /api/conversations/batch — delete multiple conversations by ID. */
export async function batchDeleteConversations(
  ids: string[],
): Promise<{ deleted: number; failed: number; total: number }> {
  return request('DELETE', '/api/conversations/batch', { ids });
}

/** DELETE /api/conversations/{id} — delete a single conversation. */
export async function deleteConversation(id: string): Promise<SuccessResponse> {
  return request('DELETE', `/api/conversations/${encodeURIComponent(id)}`);
}

/** PATCH /api/conversations/{id}/title — rename a conversation. */
export async function renameConversation(
  id: string,
  title: string,
): Promise<{ success: boolean; title: string }> {
  return request('PATCH', `/api/conversations/${encodeURIComponent(id)}/title`, { title });
}

/**
 * POST /api/conversations/{id}/truncate — delete messages from sequence_order onward.
 * Used for message editing and regeneration.
 */
export async function truncateConversation(
  id: string,
  fromSequence: number,
): Promise<{ success: boolean; deleted: number }> {
  return request('POST', `/api/conversations/${encodeURIComponent(id)}/truncate`, {
    from_sequence: fromSequence,
  });
}

/** POST /api/conversations/{id}/compact — force compact (summarise) a conversation. */
export async function compactConversation(id: string): Promise<{ ok: boolean }> {
  return request('POST', `/api/conversations/${encodeURIComponent(id)}/compact`);
}

/** PATCH /api/conversations/{id}/summary — edit the compaction summary text. */
export async function updateSummary(id: string, text: string): Promise<{ ok: boolean }> {
  return request('PATCH', `/api/conversations/${encodeURIComponent(id)}/summary`, { text });
}

/** DELETE /api/conversations/{id}/summary — remove the compaction summary. */
export async function deleteSummary(id: string): Promise<{ ok: boolean }> {
  return request('DELETE', `/api/conversations/${encodeURIComponent(id)}/summary`);
}

/** GET /api/conversations/{id}/events — in-memory debug event log. */
export async function getConversationEvents(id: string): Promise<unknown[]> {
  return request('GET', `/api/conversations/${encodeURIComponent(id)}/events`);
}

/** GET /api/conversations/{id}/metrics — persisted generation metrics. */
export async function getConversationMetrics(id: string): Promise<unknown[]> {
  return request('GET', `/api/conversations/${encodeURIComponent(id)}/metrics`);
}

/** GET /api/conversations/{id}/token-analysis — character/token breakdown by role. */
export async function getConversationTokenAnalysis(id: string): Promise<TokenAnalysisResponse> {
  return request('GET', `/api/conversations/${encodeURIComponent(id)}/token-analysis`);
}

/** GET /api/conversations/{id}/config — get conversation-level sampler override. */
export async function getConversationConfig(id: string): Promise<SamplerConfig> {
  return request('GET', `/api/conversations/${encodeURIComponent(id)}/config`);
}

/** POST /api/conversations/{id}/config — save conversation-level sampler override. */
export async function setConversationConfig(
  id: string,
  config: SamplerConfig,
): Promise<SuccessResponse> {
  return request('POST', `/api/conversations/${encodeURIComponent(id)}/config`, config);
}

/** GET /api/conversation/{id} — get full message history for a conversation. */
export async function getConversation(id: string): Promise<ConversationContentResponse> {
  return request('GET', `/api/conversation/${encodeURIComponent(id)}`);
}

/**
 * GET /api/conversation/{id}/export — export conversation as Markdown or JSON.
 *
 * Returns the raw Response so the caller can stream the download or extract
 * the Content-Disposition filename.
 */
export async function exportConversation(
  id: string,
  format: 'md' | 'json' = 'md',
): Promise<Response> {
  return fetch(`${_baseUrl}/api/conversation/${encodeURIComponent(id)}/export?format=${format}`);
}

/** POST /api/conversation/{id}/queue — inject a message during active generation. */
export async function queueMessage(
  id: string,
  content: string,
): Promise<{ success: boolean; queued: number }> {
  return request('POST', `/api/conversation/${encodeURIComponent(id)}/queue`, { content });
}

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

/** GET /api/model/status — current model load/generation state. */
export async function getModelStatus(): Promise<ModelStatus> {
  return request('GET', '/api/model/status');
}

/** GET /api/model/info?path=... — GGUF metadata for a model file. */
export async function getModelInfo(path: string): Promise<ModelInfo> {
  return request('GET', `/api/model/info?path=${encodeURIComponent(path)}`);
}

/** POST /api/model/load — load a GGUF model in the worker process. */
export async function loadModel(req: ModelLoadRequest): Promise<ModelResponse> {
  return request('POST', '/api/model/load', req);
}

/** POST /api/model/unload — unload the current model (force-kills worker). */
export async function unloadModel(): Promise<ModelResponse> {
  return request('POST', '/api/model/unload');
}

/** POST /api/model/hard-unload — kill worker to instantly reclaim all VRAM. */
export async function hardUnloadModel(): Promise<{ success: boolean; message: string }> {
  return request('POST', '/api/model/hard-unload');
}

/** GET /api/model/history — recently used model paths. */
export async function getModelHistory(): Promise<string[]> {
  return request('GET', '/api/model/history');
}

/** POST /api/model/history — add a path to recent model history. */
export async function addModelHistory(modelPath: string): Promise<SuccessResponse> {
  return request('POST', '/api/model/history', { model_path: modelPath });
}

/** GET /api/backends — list available compute backends (CUDA, Vulkan, CPU). */
export async function getBackends(): Promise<BackendsResponse> {
  return request('GET', '/api/backends');
}

/**
 * POST /api/backends/install — download GPU backend DLLs (SSE progress stream).
 *
 * @example
 * await installBackend((ev) => console.log(ev));
 */
export async function installBackend(onEvent: (event: DownloadEvent) => void): Promise<void> {
  const res = await fetch(`${_baseUrl}/api/backends/install`, { method: 'POST' });
  if (!res.ok || !res.body) throw new Error(`HTTP ${res.status}`);
  await consumeSseStream(res.body, onEvent as (data: unknown) => void);
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/** GET /api/config — current global sampler configuration. */
export async function getConfig(): Promise<SamplerConfig> {
  return request('GET', '/api/config');
}

/** POST /api/config — save global sampler configuration. */
export async function saveConfig(config: SamplerConfig): Promise<SuccessResponse> {
  return request('POST', '/api/config', config);
}

/** GET /api/config/provider-keys — masked provider API key entries. */
export async function getProviderKeys(): Promise<Record<string, ProviderKeyEntry>> {
  return request('GET', '/api/config/provider-keys');
}

/** POST /api/config/provider-keys — set a provider API key and optional base URL. */
export async function setProviderKey(
  provider: string,
  apiKey: string,
  baseUrl?: string,
): Promise<{ success: boolean; provider: string }> {
  return request('POST', '/api/config/provider-keys', {
    provider,
    api_key: apiKey,
    base_url: baseUrl,
  });
}

/** GET /api/config/active-provider — currently selected provider and model. */
export async function getActiveProvider(): Promise<ActiveProviderResponse> {
  return request('GET', '/api/config/active-provider');
}

/** POST /api/config/active-provider — switch the active provider. */
export async function setActiveProvider(
  provider: string,
  model?: string,
): Promise<{ success: boolean; provider: string; model?: string }> {
  return request('POST', '/api/config/active-provider', { provider, model });
}

// ---------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------

/** GET /api/providers — list all providers with availability status. */
export async function listProviders(): Promise<{ providers: unknown[] }> {
  return request('GET', '/api/providers');
}

/** GET /api/providers/configured — list configured non-CLI providers. */
export async function listConfiguredProviders(): Promise<{ providers: unknown[] }> {
  return request('GET', '/api/providers/configured');
}

/** GET /api/providers/cli-status — check availability of CLI providers. */
export async function listCliProviders(): Promise<{ providers: unknown[] }> {
  return request('GET', '/api/providers/cli-status');
}

/** GET /api/providers/{provider}/models — fetch model list from a provider. */
export async function getProviderModels(provider: string): Promise<{ models: string[] }> {
  return request('GET', `/api/providers/${encodeURIComponent(provider)}/models`);
}

/** POST /api/providers/{provider}/generate — blocking generation via a remote provider. */
export async function providerGenerate(
  provider: string,
  req: ProviderRequest,
): Promise<ProviderGenerateResponse> {
  return request('POST', `/api/providers/${encodeURIComponent(provider)}/generate`, req);
}

/**
 * POST /api/providers/{provider}/stream — streaming generation via a remote provider (SSE).
 *
 * @example
 * await providerStream("openai", { prompt: "hello" }, (ev) => console.log(ev));
 */
export async function providerStream(
  provider: string,
  req: ProviderRequest,
  onEvent: (event: ProviderStreamEvent) => void,
): Promise<void> {
  const res = await fetch(`${_baseUrl}/api/providers/${encodeURIComponent(provider)}/stream`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
  if (!res.ok || !res.body) throw new Error(`HTTP ${res.status}`);
  await consumeSseStream(res.body, onEvent as (data: unknown) => void);
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

/** GET /api/tools/available — list all native tools with schemas. */
export async function getAvailableTools(): Promise<{ core_tools: number; tools: unknown[] }> {
  return request('GET', '/api/tools/available');
}

/** POST /api/tools/execute — execute a named tool. */
export async function executeTool(req: ToolExecuteRequest): Promise<ToolExecuteResponse> {
  return request('POST', '/api/tools/execute', req);
}

/** GET /api/tools/web-fetch?url=... — fetch a web page as plain text. */
export async function webFetch(url: string, maxLength?: number): Promise<WebFetchResponse> {
  return request('GET', `/api/tools/web-fetch${buildQuery({ url, max_length: maxLength })}`);
}

/**
 * POST /api/file/extract-text?filename=report.pdf — extract text from an uploaded file.
 *
 * @param filename Filename with extension (used for format detection).
 * @param bytes    Raw file bytes.
 */
export async function extractFileText(
  filename: string,
  bytes: Uint8Array,
): Promise<ExtractTextResponse> {
  const res = await fetch(
    `${_baseUrl}/api/file/extract-text?filename=${encodeURIComponent(filename)}`,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/octet-stream' },
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      body: bytes as any,
    },
  );
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  return res.json();
}

// ---------------------------------------------------------------------------
// MCP
// ---------------------------------------------------------------------------

/** GET /api/mcp/servers — list configured MCP server definitions. */
export async function listMcpServers(): Promise<McpServerConfig[]> {
  return request('GET', '/api/mcp/servers');
}

/** POST /api/mcp/servers — add or update an MCP server configuration. */
export async function saveMcpServer(config: McpServerConfig): Promise<SuccessResponse> {
  return request('POST', '/api/mcp/servers', config);
}

/** DELETE /api/mcp/servers/{id} — remove an MCP server configuration. */
export async function deleteMcpServer(id: string): Promise<SuccessResponse> {
  return request('DELETE', `/api/mcp/servers/${encodeURIComponent(id)}`);
}

/** POST /api/mcp/servers/{id}/toggle — enable or disable an MCP server. */
export async function toggleMcpServer(id: string, enabled: boolean): Promise<SuccessResponse> {
  return request('POST', `/api/mcp/servers/${encodeURIComponent(id)}/toggle`, { enabled });
}

/** POST /api/mcp/refresh — reconnect to all MCP servers and rediscover tools. */
export async function refreshMcp(): Promise<McpRefreshResponse> {
  return request('POST', '/api/mcp/refresh');
}

/** GET /api/mcp/tools — list tools discovered from connected MCP servers. */
export async function listMcpTools(): Promise<McpToolsResponse> {
  return request('GET', '/api/mcp/tools');
}

// ---------------------------------------------------------------------------
// File system
// ---------------------------------------------------------------------------

/** GET /api/browse?path=... — list files and directories. */
export async function browseFiles(path?: string): Promise<BrowseFilesResponse> {
  return request('GET', `/api/browse${buildQuery({ path })}`);
}

/** POST /api/browse/pick-directory — open native OS directory picker. */
export async function pickDirectory(): Promise<{ path: string | null }> {
  return request('POST', '/api/browse/pick-directory');
}

/** POST /api/browse/pick-file — open native OS GGUF file picker. */
export async function pickFile(): Promise<{ path: string | null }> {
  return request('POST', '/api/browse/pick-file');
}

/**
 * POST /api/upload — upload a GGUF model file (max 100MB).
 *
 * @param filename Destination filename.
 * @param bytes    Raw file bytes.
 */
export async function uploadModel(
  filename: string,
  bytes: Uint8Array,
): Promise<{ success: boolean; message: string; file_path: string }> {
  const res = await fetch(`${_baseUrl}/api/upload?filename=${encodeURIComponent(filename)}`, {
    method: 'POST',
    headers: {
      'Content-Disposition': `attachment; filename="${filename}"`,
      'Content-Type': 'application/octet-stream',
    },
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    body: bytes as any,
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  return res.json();
}

// ---------------------------------------------------------------------------
// HuggingFace Hub
// ---------------------------------------------------------------------------

/** GET /api/hub/search?q=...&limit=20&sort=downloads — search GGUF models on HF Hub. */
export async function hubSearch(
  q: string,
  limit = 20,
  sort: 'downloads' | 'likes' | 'lastModified' | 'createdAt' = 'downloads',
): Promise<HubModel[]> {
  return request('GET', `/api/hub/search${buildQuery({ q, limit, sort })}`);
}

/** GET /api/hub/tree?id=user/repo — list GGUF files in a HF repo. */
export async function hubTree(id: string): Promise<HubFile[]> {
  return request('GET', `/api/hub/tree${buildQuery({ id })}`);
}

/**
 * POST /api/hub/download — download a GGUF model from HuggingFace (SSE progress).
 *
 * @example
 * await hubDownload({ model_id: "unsloth/Llama-3.2-1B-GGUF", filename: "model.Q4_K_M.gguf", destination: "/models" }, console.log);
 */
export async function hubDownload(
  req: DownloadRequest,
  onEvent: (event: DownloadEvent) => void,
): Promise<void> {
  const res = await fetch(`${_baseUrl}/api/hub/download`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
  if (!res.ok || !res.body) throw new Error(`HTTP ${res.status}`);
  await consumeSseStream(res.body, onEvent as (data: unknown) => void);
}

/** GET /api/hub/downloads — list all download records. */
export async function listDownloads(): Promise<unknown[]> {
  return request('GET', '/api/hub/downloads');
}

/** DELETE /api/hub/downloads?id=123 — cancel/delete a download by record ID. */
export async function deleteDownload(id: number): Promise<{ ok: boolean }> {
  return request('DELETE', `/api/hub/downloads?id=${id}`);
}

/** POST /api/hub/downloads/verify — prune stale download records and return clean list. */
export async function verifyDownloads(): Promise<unknown[]> {
  return request('POST', '/api/hub/downloads/verify');
}

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

/** GET /api/images/{path} — URL for a persisted screenshot image. */
export function getImageUrl(relativePath: string): string {
  return `${_baseUrl}/api/images/${relativePath}`;
}

// ---------------------------------------------------------------------------
// SSE utility (internal)
// ---------------------------------------------------------------------------

async function consumeSseStream(
  body: ReadableStream<Uint8Array>,
  onEvent: (data: unknown) => void,
): Promise<void> {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buf = '';

  try {
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      buf += decoder.decode(value, { stream: true });
      const lines = buf.split('\n');
      buf = lines.pop() ?? '';
      for (const line of lines) {
        if (!line.startsWith('data: ')) continue;
        const payload = line.slice(6).trim();
        if (payload === '[DONE]') return;
        try {
          onEvent(JSON.parse(payload));
        } catch {
          // ignore malformed events
        }
      }
    }
  } finally {
    reader.releaseLock();
  }
}
