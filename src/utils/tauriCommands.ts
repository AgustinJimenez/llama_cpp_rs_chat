/**
 * Unified typed wrappers for all backend commands.
 * Each function branches between Tauri invoke() and HTTP fetch().
 */
import { isTauriEnv } from './tauri';
import type { SamplerConfig, ToolCall } from '../types';

// ─── Types ────────────────────────────────────────────────────────────

interface ModelStatus {
  loaded: boolean;
  model_path: string | null;
  last_used: string | null;
  memory_usage_mb: number | null;
}

interface ModelResponse {
  success: boolean;
  message: string;
  status?: ModelStatus;
}

interface ConversationFile {
  name: string;
  display_name: string;
  timestamp: string;
}

interface ConversationsResponse {
  conversations: ConversationFile[];
}

interface ConversationContentResponse {
  content: string;
  messages: Array<{
    id: string;
    role: string;
    content: string;
    timestamp: number;
  }>;
}

interface FileItem {
  name: string;
  path: string;
  is_directory: boolean;
  size?: number;
}

interface BrowseFilesResponse {
  files: FileItem[];
  current_path: string;
  parent_path?: string;
}

export interface SystemUsageData {
  cpu: number;
  gpu: number;
  ram: number;
  total_ram_gb?: number;
  total_vram_gb?: number;
  cpu_cores?: number;
  cpu_ghz?: number;
}

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

// ─── Helper ───────────────────────────────────────────────────────────

async function invokeCmd<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

async function fetchJson<T>(url: string, init?: RequestInit): Promise<T> {
  const response = await fetch(url, init);
  if (!response.ok) {
    const text = await response.text().catch(() => response.statusText);
    throw new Error(text || `HTTP ${response.status}`);
  }
  return response.json();
}

// ─── Configuration ────────────────────────────────────────────────────

export async function getConfig(): Promise<SamplerConfig> {
  if (isTauriEnv()) {
    return invokeCmd<SamplerConfig>('get_config');
  }
  return fetchJson<SamplerConfig>('/api/config');
}

export async function saveConfig(config: SamplerConfig): Promise<void> {
  if (isTauriEnv()) {
    await invokeCmd('save_config', { config });
    return;
  }
  const response = await fetch('/api/config', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(config),
  });
  if (!response.ok) throw new Error('Failed to save configuration');
}

// ─── Per-Conversation Config ──────────────────────────────────────────

export async function getConversationConfig(conversationId: string): Promise<SamplerConfig> {
  return fetchJson<SamplerConfig>(`/api/conversations/${encodeURIComponent(conversationId)}/config`);
}

export async function saveConversationConfig(conversationId: string, config: SamplerConfig): Promise<void> {
  const response = await fetch(`/api/conversations/${encodeURIComponent(conversationId)}/config`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(config),
  });
  if (!response.ok) throw new Error('Failed to save conversation configuration');
}

// ─── Model ────────────────────────────────────────────────────────────

export async function getModelStatus(): Promise<ModelStatus> {
  if (isTauriEnv()) {
    return invokeCmd<ModelStatus>('get_model_status');
  }
  return fetchJson<ModelStatus>('/api/model/status');
}

export async function loadModel(modelPath: string, gpuLayers?: number): Promise<ModelResponse> {
  const payload = { model_path: modelPath, gpu_layers: gpuLayers ?? null };
  if (isTauriEnv()) {
    return invokeCmd<ModelResponse>('load_model', { request: payload });
  }
  return fetchJson<ModelResponse>('/api/model/load', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
}

export async function unloadModel(): Promise<ModelResponse> {
  if (isTauriEnv()) {
    return invokeCmd<ModelResponse>('unload_model');
  }
  return fetchJson<ModelResponse>('/api/model/unload', { method: 'POST' });
}

export async function hardUnload(): Promise<void> {
  if (isTauriEnv()) {
    await invokeCmd('hard_unload');
    return;
  }
  await fetch('/api/model/hard-unload', { method: 'POST' });
}

export async function getModelInfo(modelPath: string): Promise<Record<string, unknown>> {
  if (isTauriEnv()) {
    return invokeCmd<Record<string, unknown>>('get_model_info', { modelPath });
  }
  const encodedPath = encodeURIComponent(modelPath.trim());
  return fetchJson<Record<string, unknown>>(`/api/model/info?path=${encodedPath}`);
}

export async function getModelHistory(): Promise<string[]> {
  if (isTauriEnv()) {
    return invokeCmd<string[]>('get_model_history');
  }
  return fetchJson<string[]>('/api/model/history');
}

export async function addModelHistory(modelPath: string): Promise<void> {
  if (isTauriEnv()) {
    await invokeCmd('add_model_history', { modelPath });
    return;
  }
  await fetch('/api/model/history', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ model_path: modelPath }),
  });
}

// ─── Conversations ────────────────────────────────────────────────────

export async function getConversations(): Promise<ConversationsResponse> {
  if (isTauriEnv()) {
    return invokeCmd<ConversationsResponse>('get_conversations');
  }
  return fetchJson<ConversationsResponse>('/api/conversations');
}

export async function getConversation(filename: string): Promise<ConversationContentResponse> {
  if (isTauriEnv()) {
    return invokeCmd<ConversationContentResponse>('get_conversation', { filename });
  }
  return fetchJson<ConversationContentResponse>(`/api/conversation/${filename}`);
}

export async function deleteConversation(filename: string): Promise<void> {
  if (isTauriEnv()) {
    await invokeCmd('delete_conversation', { filename });
    return;
  }
  const response = await fetch(`/api/conversations/${filename}`, { method: 'DELETE' });
  if (!response.ok) throw new Error('Failed to delete conversation');
}

export interface ConversationMetric {
  level: string;
  message: string;  // JSON string with prompt_tok_per_sec, gen_tok_per_sec, tokens_used, max_tokens
  timestamp: number;
}

export async function getConversationMetrics(conversationId: string): Promise<ConversationMetric[]> {
  const id = conversationId.replace('.txt', '');
  if (isTauriEnv()) {
    return invokeCmd<ConversationMetric[]>('get_conversation_metrics', { conversationId: id });
  }
  return fetchJson<ConversationMetric[]>(`/api/conversations/${id}/metrics`);
}

// ─── Chat ─────────────────────────────────────────────────────────────

export async function cancelGeneration(): Promise<void> {
  if (isTauriEnv()) {
    await invokeCmd('cancel_generation');
    return;
  }
  await fetch('/api/chat/cancel', { method: 'POST' });
}

// ─── Files ────────────────────────────────────────────────────────────

export async function browseFiles(path?: string): Promise<BrowseFilesResponse> {
  if (isTauriEnv()) {
    return invokeCmd<BrowseFilesResponse>('browse_files', path ? { path } : {});
  }
  const query = path ? `?path=${encodeURIComponent(path)}` : '';
  return fetchJson<BrowseFilesResponse>(`/api/browse${query}`);
}

// ─── Tools ────────────────────────────────────────────────────────────

export async function executeTool(toolCall: ToolCall): Promise<Record<string, unknown>> {
  if (isTauriEnv()) {
    return invokeCmd<Record<string, unknown>>('execute_tool', {
      request: {
        tool_name: toolCall.name,
        arguments: toolCall.arguments,
      },
    });
  }
  return fetchJson<Record<string, unknown>>('/api/tools/execute', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      tool_name: toolCall.name,
      arguments: toolCall.arguments,
    }),
  });
}

export async function webFetch(url: string, maxLength?: number): Promise<Record<string, unknown>> {
  if (isTauriEnv()) {
    return invokeCmd<Record<string, unknown>>('web_fetch', { url, maxLength });
  }
  const params = new URLSearchParams({ url });
  if (maxLength) params.set('max_length', String(maxLength));
  return fetchJson<Record<string, unknown>>(`/api/tools/web-fetch?${params}`);
}

// ─── Native Directory Picker ─────────────────────────────────────────

export async function pickDirectory(): Promise<string | null> {
  if (isTauriEnv()) {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const selected = await open({ directory: true, multiple: false });
    return typeof selected === 'string' ? selected : null;
  }
  const result = await fetchJson<{ path: string | null }>('/api/browse/pick-directory', { method: 'POST' });
  return result.path;
}

// ─── Native File Picker ─────────────────────────────────────────────

export async function pickFile(): Promise<string | null> {
  if (isTauriEnv()) {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const selected = await open({
      multiple: false,
      filters: [{ name: 'GGUF Model Files', extensions: ['gguf'] }],
    });
    return selected ?? null;
  }
  const result = await fetchJson<{ path: string | null }>('/api/browse/pick-file', { method: 'POST' });
  return result.path;
}

// ─── HuggingFace Hub ─────────────────────────────────────────────────

export type HubSortField = 'downloads' | 'likes' | 'lastModified' | 'createdAt';

export async function searchHubModels(query: string, limit = 20, sort: HubSortField = 'downloads'): Promise<HubModel[]> {
  const params = `q=${encodeURIComponent(query)}&limit=${limit}&sort=${sort}`;
  if (isTauriEnv()) {
    return invokeCmd<HubModel[]>('search_hub_models', { query, limit, sort });
  }
  return fetchJson<HubModel[]>(`/api/hub/search?${params}`);
}

export async function fetchHubTree(modelId: string): Promise<HubFile[]> {
  if (isTauriEnv()) {
    return invokeCmd<HubFile[]>('fetch_hub_tree', { modelId });
  }
  return fetchJson<HubFile[]>(`/api/hub/tree?id=${encodeURIComponent(modelId)}`);
}

// ─── Hub Download History ────────────────────────────────────────────

export interface HubDownloadRecord {
  id: number;
  model_id: string;
  filename: string;
  dest_path: string;
  file_size: number;
  bytes_downloaded: number;
  status: string; // 'pending' | 'completed'
  etag: string | null;
  downloaded_at: number;
}

/** Verify which downloaded files still exist on disk — prunes missing records from DB */
export async function verifyHubDownloads(): Promise<HubDownloadRecord[]> {
  return fetchJson<HubDownloadRecord[]>('/api/hub/downloads/verify', { method: 'POST' });
}

// ─── Hub Download (SSE) ──────────────────────────────────────────────

export interface DownloadProgress {
  type: 'progress' | 'done' | 'error';
  bytes?: number;
  total?: number;
  speed_kbps?: number;
  path?: string;
  message?: string;
}

export function startHubDownload(
  modelId: string,
  filename: string,
  destination: string,
  onProgress: (event: DownloadProgress) => void,
): AbortController {
  const controller = new AbortController();

  fetch('/api/hub/download', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ model_id: modelId, filename, destination }),
    signal: controller.signal,
  })
    .then(async (response) => {
      if (!response.ok) {
        const text = await response.text().catch(() => response.statusText);
        let msg = text;
        try { msg = JSON.parse(text).error || text; } catch { /* keep raw */ }
        onProgress({ type: 'error', message: msg });
        return;
      }

      const reader = response.body?.getReader();
      if (!reader) {
        onProgress({ type: 'error', message: 'No response body' });
        return;
      }

      const decoder = new TextDecoder();
      let buffer = '';

      // eslint-disable-next-line no-constant-condition
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split('\n');
        buffer = lines.pop() ?? '';

        for (const line of lines) {
          if (line.startsWith('data: ')) {
            try {
              const event = JSON.parse(line.slice(6)) as DownloadProgress;
              onProgress(event);
            } catch { /* skip malformed */ }
          }
        }
      }
    })
    .catch((err: unknown) => {
      if (err instanceof Error && err.name !== 'AbortError') {
        onProgress({ type: 'error', message: String(err) });
      }
    });

  return controller;
}

// ─── System ───────────────────────────────────────────────────────────

export async function getSystemUsage(): Promise<SystemUsageData> {
  if (isTauriEnv()) {
    return invokeCmd<SystemUsageData>('get_system_usage');
  }
  return fetchJson<SystemUsageData>('/api/system/usage');
}
