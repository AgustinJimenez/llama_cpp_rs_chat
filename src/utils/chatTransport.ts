import type { ChatRequest } from '../types';
import { isTauriEnv } from './tauri';

export interface StreamingCallbacks {
  onToken: (token: string, tokensUsed?: number, maxTokens?: number) => void;
  onComplete: (messageId: string, conversationId: string, tokensUsed?: number, maxTokens?: number) => void;
  onError: (error: string) => void;
}

export interface ChatTransport {
  sendMessage: (request: ChatRequest) => Promise<Response>;
  streamMessage: (
    request: ChatRequest,
    callbacks: StreamingCallbacks,
    abortSignal?: AbortSignal
  ) => Promise<void>;
}

const buildWsUrl = (path: string): string => {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${protocol}//${window.location.host}${path}`;
};

class BrowserChatTransport implements ChatTransport {
  async sendMessage(request: ChatRequest): Promise<Response> {
    return fetch('/api/chat', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(request),
    });
  }

  async streamMessage(
    request: ChatRequest,
    { onToken, onComplete, onError }: StreamingCallbacks,
    abortSignal?: AbortSignal
  ): Promise<void> {
    const safeOnToken = onToken ?? (() => {});
    const safeOnComplete = onComplete ?? (() => {});
    const safeOnError = onError ?? (() => {});

    return new Promise((resolve, reject) => {
      const ws = new WebSocket(buildWsUrl('/ws/chat/stream'));
      let isCompleted = false;
      let lastTokensUsed: number | undefined;
      let lastMaxTokens: number | undefined;

      const settle = (error?: Error) => {
        try {
          ws.close();
        } catch {
          // no-op
        }
        if (error) {
          reject(error);
        } else {
          resolve();
        }
      };

      if (abortSignal) {
        if (typeof abortSignal.addEventListener === 'function') {
          abortSignal.addEventListener('abort', () => settle(), { once: true });
        } else if ((abortSignal as unknown as { aborted?: boolean }).aborted) {
          settle();
          return;
        }
      }

      ws.onopen = () => {
        ws.send(JSON.stringify(request));
      };

      ws.onmessage = (event) => {
        try {
          const message = JSON.parse(event.data);

          if (message.type === 'token') {
            if (message.tokens_used !== undefined) {
              lastTokensUsed = message.tokens_used;
            }
            if (message.max_tokens !== undefined) {
              lastMaxTokens = message.max_tokens;
            }
            safeOnToken(message.token, message.tokens_used, message.max_tokens);
          } else if (message.type === 'done') {
            isCompleted = true;
            const conversationId = message.conversation_id || request.conversation_id || crypto.randomUUID();
            safeOnComplete(crypto.randomUUID(), conversationId, lastTokensUsed, lastMaxTokens);
            settle();
          } else if (message.type === 'error') {
            const errorMessage = message.error || 'Unknown error';
            safeOnError(errorMessage);
            settle(new Error(errorMessage));
          }
        } catch (err) {
          const errorMessage = 'Failed to parse server message';
          safeOnError(errorMessage);
          settle(err instanceof Error ? err : new Error(errorMessage));
        }
      };

      ws.onerror = () => {
        if (!isCompleted) {
          const errorMessage = 'WebSocket connection error';
          safeOnError(errorMessage);
          settle(new Error(errorMessage));
        }
      };

      ws.onclose = (event) => {
        if (!isCompleted && event.code !== 1000) {
          const errorMessage = `Connection closed unexpectedly: ${event.reason || 'Unknown reason'}`;
          safeOnError(errorMessage);
          settle(new Error(errorMessage));
        } else if (!isCompleted) {
          settle();
        }
      };
    });
  }
}

class TauriChatTransport extends BrowserChatTransport {
  // For now, reuse WebSocket streaming even in Tauri; only sendMessage differs.
  async sendMessage(request: ChatRequest): Promise<Response> {
    const { invoke } = await import('@tauri-apps/api/core');
    const payload = await invoke<unknown>('send_message', { request });
    return new Response(JSON.stringify(payload), { status: 200, headers: { 'Content-Type': 'application/json' } });
  }
}

export const createChatTransport = (): ChatTransport =>
  isTauriEnv() ? new TauriChatTransport() : new BrowserChatTransport();
