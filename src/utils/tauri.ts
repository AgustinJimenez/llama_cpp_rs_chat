import type { ChatRequest, ChatResponse, SamplerConfig, Message } from '../types';

// Check if we're running in Tauri or web environment
const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export class TauriAPI {
  static async sendMessage(request: ChatRequest): Promise<ChatResponse> {
    if (isTauri) {
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
    if (isTauri) {
      const { invoke } = await import('@tauri-apps/api/core');
      return await invoke('get_conversations');
    } else {
      // For web version, return empty conversations (could implement server-side storage)
      return {};
    }
  }

  static async getConversation(conversationId: string): Promise<Message[]> {
    if (isTauri) {
      const { invoke } = await import('@tauri-apps/api/core');
      return await invoke('get_conversation', { conversationId });
    } else {
      // For web version, return empty conversation (could implement server-side storage)
      return [];
    }
  }

  static async getSamplerConfig(): Promise<SamplerConfig> {
    if (isTauri) {
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
    if (isTauri) {
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

  static async sendMessageStream(
    request: ChatRequest,
    onToken: (token: string) => void,
    onComplete: (messageId: string, conversationId: string) => void,
    onError: (error: string) => void
  ): Promise<void> {
    // For demo purposes, simulate streaming by getting the full response and streaming it
    try {
      const response = await this.sendMessage(request);
      const fullContent = response.message.content;
      
      // Split the response into words and stream them
      const words = fullContent.split(' ');
      
      for (let i = 0; i < words.length; i++) {
        const token = i === 0 ? words[i] : ' ' + words[i];
        onToken(token);
        
        // Add delay to simulate streaming
        await new Promise(resolve => setTimeout(resolve, 100));
      }
      
      onComplete(response.message.id, response.conversation_id);
    } catch (error) {
      onError(error instanceof Error ? error.message : 'Unknown error');
    }
  }
}