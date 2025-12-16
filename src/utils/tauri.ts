import type { ChatRequest, ChatResponse, SamplerConfig, Message } from '../types';

// Check if we're running in Tauri or web environment
export const isTauriEnv = (): boolean =>
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export class TauriAPI {
  static async sendMessage(request: ChatRequest): Promise<ChatResponse> {
    if (isTauriEnv()) {
      // Use Tauri invoke for desktop app
      const { invoke } = await import('@tauri-apps/api/core');
      return await invoke('send_message', { request });
    } else {
      // Use HTTP API for web version
      const response = await fetch('/api/chat', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(request),
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      return await response.json();
    }
  }

  static async getConversations(): Promise<Record<string, Message[]>> {
    if (isTauriEnv()) {
      const { invoke } = await import('@tauri-apps/api/core');
      return await invoke('get_conversations');
    } else {
      // For web version, return empty conversations (could implement server-side storage)
      return {};
    }
  }

  static async getConversation(conversationId: string): Promise<Message[]> {
    if (isTauriEnv()) {
      const { invoke } = await import('@tauri-apps/api/core');
      return await invoke('get_conversation', { conversationId });
    } else {
      // For web version, return empty conversation (could implement server-side storage)
      return [];
    }
  }

  static async getSamplerConfig(): Promise<SamplerConfig> {
    if (isTauriEnv()) {
      const { invoke } = await import('@tauri-apps/api/core');
      return await invoke('get_sampler_config');
    } else {
      // Use HTTP API for web version
      const response = await fetch('/api/config');
      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }
      return await response.json();
    }
  }

  static async updateSamplerConfig(config: SamplerConfig): Promise<void> {
    if (isTauriEnv()) {
      const { invoke } = await import('@tauri-apps/api/core');
      return await invoke('update_sampler_config', { config });
    } else {
      // Use HTTP API for web version
      const response = await fetch('/api/config', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(config),
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }
    }
  }
}
