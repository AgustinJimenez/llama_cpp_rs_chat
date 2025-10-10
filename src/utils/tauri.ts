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
    try {
      console.log('[FRONTEND] Calling /api/chat/stream endpoint');
      const response = await fetch('/api/chat/stream', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(request),
        signal: abortSignal,
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      if (!response.body) {
        throw new Error('Response body is null');
      }

      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = '';
      let lastTokensUsed: number | undefined = undefined;
      let lastMaxTokens: number | undefined = undefined;

      while (true) {
        const { done, value } = await reader.read();

        if (done) break;

        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split('\n');
        buffer = lines.pop() || ''; // Keep incomplete line in buffer

        for (const line of lines) {
          if (line.startsWith('data: ')) {
            const data = line.substring(6);

            if (data === '[DONE]') {
              // Stream complete - pass the last known token counts
              onComplete(crypto.randomUUID(), request.conversation_id || crypto.randomUUID(), lastTokensUsed, lastMaxTokens);
              return;
            }

            try {
              const tokenData = JSON.parse(data);
              console.log('[FRONTEND] Received token data:', tokenData);

              // Check if it's the new TokenData format with metadata
              if (typeof tokenData === 'object' && tokenData.token !== undefined) {
                // Track the last token counts
                if (tokenData.tokens_used !== undefined) {
                  lastTokensUsed = tokenData.tokens_used;
                }
                if (tokenData.max_tokens !== undefined) {
                  lastMaxTokens = tokenData.max_tokens;
                }
                onToken(tokenData.token, tokenData.tokens_used, tokenData.max_tokens);
              } else if (typeof tokenData === 'string') {
                // Fallback for old format (just a string token)
                onToken(tokenData, undefined, undefined);
              }
            } catch (e) {
              console.error('Failed to parse token:', e);
            }
          }
        }
      }

      onComplete(crypto.randomUUID(), request.conversation_id || crypto.randomUUID(), lastTokensUsed, lastMaxTokens);
    } catch (error) {
      onError(error instanceof Error ? error.message : 'Unknown error');
    }
  }
}