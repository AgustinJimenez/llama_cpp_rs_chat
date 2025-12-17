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

const WS_CONNECT_TIMEOUT_MS = 10000;

type StreamState = {
  isCompleted: boolean;
  wasAborted: boolean;
  lastTokensUsed?: number;
  lastMaxTokens?: number;
};

function handleStreamMessage(
  rawData: string,
  request: ChatRequest,
  state: StreamState,
  callbacks: {
    onToken: (token: string, tokensUsed?: number, maxTokens?: number) => void;
    onComplete: (messageId: string, conversationId: string, tokensUsed?: number, maxTokens?: number) => void;
    onError: (error: string) => void;
  },
  settle: (error?: Error) => void,
  markAborted: () => void
) {
  try {
    const message = JSON.parse(rawData);

    if (message.type === 'token') {
      if (message.tokens_used !== undefined) state.lastTokensUsed = message.tokens_used;
      if (message.max_tokens !== undefined) state.lastMaxTokens = message.max_tokens;
      callbacks.onToken(message.token, message.tokens_used, message.max_tokens);
      return;
    }

    if (message.type === 'done') {
      state.isCompleted = true;
      const conversationId = message.conversation_id || request.conversation_id || crypto.randomUUID();
      callbacks.onComplete(crypto.randomUUID(), conversationId, state.lastTokensUsed, state.lastMaxTokens);
      settle();
      return;
    }

    if (message.type === 'error') {
      const errorMessage = message.error || 'Unknown error';
      callbacks.onError(errorMessage);
      settle(new Error(errorMessage));
      return;
    }

    if (message.type === 'abort') {
      markAborted();
    }
  } catch (err) {
    if (state.wasAborted) {
      markAborted();
      return;
    }
    const errorMessage = 'Failed to parse server message';
    callbacks.onError(errorMessage);
    settle(err instanceof Error ? err : new Error(errorMessage));
  }
}

function streamViaWebSocket(
  request: ChatRequest,
  { onToken, onComplete, onError }: StreamingCallbacks,
  abortSignal?: AbortSignal
): Promise<void> {
  const safeOnToken = onToken ?? (() => {});
  const safeOnComplete = onComplete ?? (() => {});
  const safeOnError = onError ?? (() => {});

  return new Promise((resolve, reject) => {
    const ws = new WebSocket(buildWsUrl('/ws/chat/stream'));
    const state: StreamState = { isCompleted: false, wasAborted: false };
    let connectTimer: ReturnType<typeof setTimeout> | null = null;
    let settled = false;

    const settle = (error?: Error) => {
      if (settled) return;
      settled = true;
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

    const markAborted = () => {
      state.wasAborted = true;
      settle(new Error('Request aborted'));
    };

    if (abortSignal) {
      if (typeof abortSignal.addEventListener === 'function') {
        abortSignal.addEventListener('abort', markAborted, { once: true });
      } else if ((abortSignal as unknown as { aborted?: boolean }).aborted) {
        markAborted();
        return;
      }
    }

    // Connection timeout guard
    connectTimer = setTimeout(() => {
      if (state.wasAborted) return;
      safeOnError('WebSocket connection timed out');
      settle(new Error('WebSocket connection timed out'));
    }, WS_CONNECT_TIMEOUT_MS);

    ws.onopen = () => {
      if (connectTimer) {
        clearTimeout(connectTimer);
        connectTimer = null;
      }
      if (state.wasAborted) {
        markAborted();
        return;
      }
      ws.send(JSON.stringify(request));
    };

    ws.onmessage = (event) => {
      handleStreamMessage(
        event.data,
        request,
        state,
        { onToken: safeOnToken, onComplete: safeOnComplete, onError: safeOnError },
        settle,
        markAborted
      );
    };

    ws.onerror = () => {
      if (state.wasAborted) {
        markAborted();
        return;
      }
      if (!state.isCompleted) {
        const errorMessage = 'WebSocket connection error';
        safeOnError(errorMessage);
        settle(new Error(errorMessage));
      }
    };

    ws.onclose = (event) => {
      if (connectTimer) {
        clearTimeout(connectTimer);
        connectTimer = null;
      }
      if (state.wasAborted) {
        markAborted();
        return;
      }
      if (!state.isCompleted && event.code !== 1000) {
        const errorMessage = `Connection closed unexpectedly: ${event.reason || 'Unknown reason'}`;
        safeOnError(errorMessage);
        settle(new Error(errorMessage));
      } else if (!state.isCompleted) {
        settle();
      }
    };
  });
}

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
    return streamViaWebSocket(request, { onToken, onComplete, onError }, abortSignal);
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
