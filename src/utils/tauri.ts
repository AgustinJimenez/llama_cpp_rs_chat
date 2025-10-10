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
    onToken: (token: string, tokensUsed?: number, maxTokens?: number) => void,
    onComplete: (messageId: string, conversationId: string, tokensUsed?: number, maxTokens?: number) => void,
    onError: (error: string) => void,
    abortSignal?: AbortSignal
  ): Promise<void> {
    return new Promise((resolve, reject) => {
      // Determine WebSocket URL based on current protocol
      const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
      const wsUrl = `${protocol}//${window.location.host}/ws/chat/stream`;

      const ws = new WebSocket(wsUrl);
      let lastTokensUsed: number | undefined = undefined;
      let lastMaxTokens: number | undefined = undefined;
      let isCompleted = false;

      // Handle abort signal
      if (abortSignal) {
        abortSignal.addEventListener('abort', () => {
          ws.close();
          resolve();
        });
      }

      ws.onopen = () => {
        ws.send(JSON.stringify(request));
      };

      ws.onmessage = (event) => {
        try {
          const message = JSON.parse(event.data);

          if (message.type === 'token') {
            // Update token counts
            if (message.tokens_used !== undefined) {
              lastTokensUsed = message.tokens_used;
            }
            if (message.max_tokens !== undefined) {
              lastMaxTokens = message.max_tokens;
            }
            onToken(message.token, message.tokens_used, message.max_tokens);
          } else if (message.type === 'done') {
            isCompleted = true;
            onComplete(crypto.randomUUID(), request.conversation_id || crypto.randomUUID(), lastTokensUsed, lastMaxTokens);
            ws.close();
            resolve();
          } else if (message.type === 'error') {
            console.error('[FRONTEND] Stream error:', message.error);
            onError(message.error || 'Unknown error');
            ws.close();
            reject(new Error(message.error || 'Unknown error'));
          }
        } catch (e) {
          console.error('[FRONTEND] Failed to parse WebSocket message:', e);
          onError('Failed to parse server message');
          ws.close();
          reject(e);
        }
      };

      ws.onerror = (error) => {
        console.error('[FRONTEND] WebSocket error:', error);
        if (!isCompleted) {
          onError('WebSocket connection error');
          reject(new Error('WebSocket connection error'));
        }
      };

      ws.onclose = (event) => {
        if (!isCompleted && event.code !== 1000) {
          // Abnormal closure
          onError(`Connection closed unexpectedly: ${event.reason || 'Unknown reason'}`);
          reject(new Error('Connection closed unexpectedly'));
        } else if (!isCompleted) {
          // Normal closure but not completed
          resolve();
        }
      };
    });
  }
}