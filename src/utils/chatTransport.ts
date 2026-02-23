import type { ChatRequest } from '../types';
import { isTauriEnv } from './tauri';

export interface TimingInfo {
  promptTokPerSec?: number;
  genTokPerSec?: number;
}

export interface StreamingCallbacks {
  onToken: (token: string, tokensUsed?: number, maxTokens?: number) => void;
  onComplete: (messageId: string, conversationId: string, tokensUsed?: number, maxTokens?: number, timings?: TimingInfo) => void;
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
    onComplete: (messageId: string, conversationId: string, tokensUsed?: number, maxTokens?: number, timings?: TimingInfo) => void;
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
      const timings: TimingInfo = {
        promptTokPerSec: message.prompt_tok_per_sec,
        genTokPerSec: message.gen_tok_per_sec,
      };
      callbacks.onComplete(crypto.randomUUID(), conversationId, state.lastTokensUsed, state.lastMaxTokens, timings);
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
      // Only cancel if generation hasn't already completed — otherwise we'd
      // cancel the NEXT generation that starts shortly after.
      if (!state.isCompleted) {
        fetch('/api/chat/cancel', { method: 'POST' }).catch(() => {});
      }
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

class TauriChatTransport implements ChatTransport {
  async sendMessage(request: ChatRequest): Promise<Response> {
    const { invoke } = await import('@tauri-apps/api/core');
    const payload = await invoke<unknown>('generate_stream', { request });
    return new Response(JSON.stringify(payload), { status: 200, headers: { 'Content-Type': 'application/json' } });
  }

  async streamMessage(
    request: ChatRequest,
    { onToken, onComplete, onError }: StreamingCallbacks,
    abortSignal?: AbortSignal
  ): Promise<void> {
    const { invoke } = await import('@tauri-apps/api/core');
    const { listen } = await import('@tauri-apps/api/event');

    const safeOnToken = onToken ?? (() => {});
    const safeOnComplete = onComplete ?? (() => {});
    const safeOnError = onError ?? (() => {});

    let settled = false;
    let wasAborted = false;
    let settleResolve: () => void;
    let settleReject: (err: Error) => void;
    const promise = new Promise<void>((resolve, reject) => {
      settleResolve = resolve;
      settleReject = reject;
    });

    // Set up listeners before creating the promise callbacks
    const unlistenToken = await listen<{ token: string; tokens_used?: number; max_tokens?: number }>('chat-token', (event) => {
      if (settled || wasAborted) return;
      safeOnToken(event.payload.token, event.payload.tokens_used, event.payload.max_tokens);
    });

    const unlistenDone = await listen<{ type: string; conversation_id?: string; tokens_used?: number; max_tokens?: number; error?: string }>('chat-done', (event) => {
      if (settled || wasAborted) return;
      const payload = event.payload;

      if (payload.type === 'done') {
        const conversationId = payload.conversation_id || request.conversation_id || crypto.randomUUID();
        safeOnComplete(crypto.randomUUID(), conversationId, payload.tokens_used, payload.max_tokens);
        settle();
      } else if (payload.type === 'error') {
        const errorMessage = payload.error || 'Unknown error';
        safeOnError(errorMessage);
        settle(new Error(errorMessage));
      } else if (payload.type === 'cancelled') {
        settle(new Error('Request aborted'));
      }
    });

    const cleanup = () => { unlistenToken(); unlistenDone(); };

    const settle = (error?: Error) => {
      if (settled) return;
      settled = true;
      cleanup();
      if (error) settleReject(error);
      else settleResolve();
    };

    const markAborted = () => {
      wasAborted = true;
      invoke('cancel_generation').catch(() => {});
      settle(new Error('Request aborted'));
    };

    // Handle abort signal
    if (abortSignal) {
      if (abortSignal.aborted) { markAborted(); return promise; }
      abortSignal.addEventListener('abort', markAborted, { once: true });
    }

    // Start generation via invoke
    try {
      await invoke('generate_stream', { request });
    } catch (err) {
      if (!settled && !wasAborted) {
        const errorMessage = err instanceof Error ? err.message : 'Failed to start generation';
        safeOnError(errorMessage);
        settle(new Error(errorMessage));
      }
    }

    return promise;
  }
}

export const createChatTransport = (): ChatTransport =>
  isTauriEnv() ? new TauriChatTransport() : new BrowserChatTransport();
