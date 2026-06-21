import type { ChatRequest } from '../types';

import { isTauriEnv, notifyIfUnfocused } from './tauri';

export interface TokenBreakdown {
  system_prompt: number;
  tool_definitions: number;
  conversation_messages: number;
  tool_calls_and_results: number;
  model_response: number;
}

export interface TimingInfo {
  promptTokPerSec?: number;
  genTokPerSec?: number;
  genEvalMs?: number;
  genTokens?: number;
  promptEvalMs?: number;
  promptTokens?: number;
  cachedTokens?: number;
  finishReason?: string;
  tokenBreakdown?: TokenBreakdown;
  costUsd?: number;
}

export interface StreamingCallbacks {
  onToken: (
    token: string,
    tokensUsed?: number,
    maxTokens?: number,
    genTokPerSec?: number,
    genTokens?: number,
  ) => void;
  onComplete: (
    messageId: string,
    conversationId: string,
    tokensUsed?: number,
    maxTokens?: number,
    timings?: TimingInfo,
  ) => void;
  onError: (error: string) => void;
  onStatus?: (message: string) => void;
  onToolTiming?: (name: string, durationMs: number) => void;
}

export interface ChatTransport {
  sendMessage: (request: ChatRequest) => Promise<Response>;
  streamMessage: (
    request: ChatRequest,
    callbacks: StreamingCallbacks,
    abortSignal?: AbortSignal,
  ) => Promise<void>;
}

const buildWsUrl = (path: string): string => {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${protocol}//${window.location.host}${path}`;
};

const WS_CONNECT_TIMEOUT_MS = 10000;
const WS_GENERATION_TIMEOUT_MS = 300000; // No tokens/heartbeat received for 5min → error
const WS_MAX_RECONNECTS = 4;
const WS_RECONNECT_BASE_MS = 1000;
const WS_MAX_BACKOFF_MS = 8000;

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
    onToken: (
      token: string,
      tokensUsed?: number,
      maxTokens?: number,
      genTokPerSec?: number,
      genTokens?: number,
    ) => void;
    onComplete: (
      messageId: string,
      conversationId: string,
      tokensUsed?: number,
      maxTokens?: number,
      timings?: TimingInfo,
    ) => void;
    onError: (error: string) => void;
    onStatus?: (message: string) => void;
    onToolTiming?: (name: string, durationMs: number) => void;
  },
  settle: (error?: Error) => void,
  markAborted: () => void,
) {
  try {
    const message = JSON.parse(rawData);

    if (message.type === 'status') {
      callbacks.onStatus?.(message.message);
      return;
    }

    if (message.type === 'tool_timing') {
      callbacks.onToolTiming?.(message.name, message.duration_ms);
      return;
    }

    if (message.type === 'token') {
      if (message.tokens_used !== undefined) state.lastTokensUsed = message.tokens_used;
      if (message.max_tokens !== undefined) state.lastMaxTokens = message.max_tokens;
      callbacks.onToken(
        message.token,
        message.tokens_used,
        message.max_tokens,
        message.gen_tok_per_sec,
        message.gen_tokens,
      );
      return;
    }

    if (message.type === 'done') {
      state.isCompleted = true;
      const conversationId =
        message.conversation_id || request.conversation_id || crypto.randomUUID();
      const timings: TimingInfo = {
        promptTokPerSec: message.prompt_tok_per_sec,
        genTokPerSec: message.gen_tok_per_sec,
        genEvalMs: message.gen_eval_ms,
        genTokens: message.gen_tokens,
        promptEvalMs: message.prompt_eval_ms,
        promptTokens: message.prompt_tokens,
        finishReason: message.finish_reason,
        tokenBreakdown: message.token_breakdown,
      };
      callbacks.onComplete(
        crypto.randomUUID(),
        conversationId,
        state.lastTokensUsed,
        state.lastMaxTokens,
        timings,
      );
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
  { onToken, onComplete, onError, onStatus, onToolTiming }: StreamingCallbacks,
  abortSignal?: AbortSignal,
): Promise<void> {
  const safeOnToken = onToken ?? (() => {});
  const safeOnComplete = onComplete ?? (() => {});
  const safeOnError = onError ?? (() => {});

  return new Promise((resolve, reject) => {
    const state: StreamState = { isCompleted: false, wasAborted: false };
    let settled = false;
    let reconnectAttempt = 0;
    let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
    let currentWs: WebSocket | null = null;

    const settle = (error?: Error) => {
      if (settled) return;
      settled = true;
      if (reconnectTimer) clearTimeout(reconnectTimer);
      try {
        currentWs?.close();
      } catch {
        /* no-op */
      }
      if (error) reject(error);
      else resolve();
    };

    const markAborted = () => {
      state.wasAborted = true;
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

    const connect = (isReconnect: boolean) => {
      if (settled || state.wasAborted) return;

      const ws = new WebSocket(buildWsUrl('/ws/chat/stream'));
      currentWs = ws;
      let connectTimer: ReturnType<typeof setTimeout> | null = null;
      let genTimer: ReturnType<typeof setTimeout> | null = null;

      const resetGenTimer = () => {
        if (genTimer) clearTimeout(genTimer);
        genTimer = setTimeout(() => {
          if (state.isCompleted || state.wasAborted || settled) return;
          const errorMessage = 'Generation timeout: no response from model for 5 minutes';
          safeOnError(errorMessage);
          settle(new Error(errorMessage));
        }, WS_GENERATION_TIMEOUT_MS);
      };

      // Connection timeout guard
      connectTimer = setTimeout(() => {
        if (state.wasAborted || settled) return;
        safeOnError('WebSocket connection timed out');
        settle(new Error('WebSocket connection timed out'));
      }, WS_CONNECT_TIMEOUT_MS);

      ws.onopen = () => {
        if (connectTimer) {
          clearTimeout(connectTimer);
          connectTimer = null;
        }
        if (state.wasAborted || settled) {
          markAborted();
          return;
        }
        console.log(`[WS_STREAM] Connected (reconnect=${isReconnect}), sending request`); // eslint-disable-line no-console
        ws.send(JSON.stringify(isReconnect ? { ...request, reconnect: true } : request));
        resetGenTimer();
      };

      ws.onmessage = (event) => {
        resetGenTimer();
        handleStreamMessage(
          event.data,
          request,
          state,
          {
            onToken: safeOnToken,
            onComplete: safeOnComplete,
            onError: safeOnError,
            onStatus,
            onToolTiming,
          },
          settle,
          markAborted,
        );
      };

      ws.onerror = () => {
        if (state.wasAborted || settled) return;
        // onerror is always followed by onclose — handle there
      };

      ws.onclose = (event) => {
        if (connectTimer) {
          clearTimeout(connectTimer);
          connectTimer = null;
        }
        if (genTimer) {
          clearTimeout(genTimer);
          genTimer = null;
        }
        if (settled || state.wasAborted || state.isCompleted) return;

        if (event.code !== 1000 && reconnectAttempt < WS_MAX_RECONNECTS) {
          const delay = Math.min(
            WS_RECONNECT_BASE_MS * Math.pow(2, reconnectAttempt),
            WS_MAX_BACKOFF_MS,
          );
          reconnectAttempt += 1;
          console.warn(
            `[WS_STREAM] Disconnected (code=${event.code}), reconnecting in ${delay}ms (attempt ${reconnectAttempt}/${WS_MAX_RECONNECTS})`,
          ); // eslint-disable-line no-console
          reconnectTimer = setTimeout(() => connect(true), delay);
        } else if (event.code !== 1000) {
          const errorMessage = `Connection lost after ${WS_MAX_RECONNECTS} reconnect attempts`;
          console.error(`[WS_STREAM] ${errorMessage}`); // eslint-disable-line no-console
          safeOnError(errorMessage);
          settle(new Error(errorMessage));
        } else {
          console.log('[WS_STREAM] Closed normally (no completion received)'); // eslint-disable-line no-console
          settle();
        }
      };
    };

    connect(false);
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
    callbacks: StreamingCallbacks,
    abortSignal?: AbortSignal,
  ): Promise<void> {
    return streamViaWebSocket(request, callbacks, abortSignal);
  }
}

class TauriChatTransport implements ChatTransport {
  async sendMessage(request: ChatRequest): Promise<Response> {
    const { invoke } = await import('@tauri-apps/api/core');
    const payload = await invoke<unknown>('generate_stream', { request });
    return new Response(JSON.stringify(payload), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  }

  async streamMessage(
    request: ChatRequest,
    { onToken, onComplete, onError }: StreamingCallbacks,
    abortSignal?: AbortSignal,
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
    const unlistenToken = await listen<{
      token: string;
      tokens_used?: number;
      max_tokens?: number;
    }>('chat-token', (event) => {
      if (settled || wasAborted) return;
      safeOnToken(event.payload.token, event.payload.tokens_used, event.payload.max_tokens);
    });

    const unlistenDone = await listen<{
      type: string;
      conversation_id?: string;
      tokens_used?: number;
      max_tokens?: number;
      error?: string;
      prompt_tok_per_sec?: number;
      gen_tok_per_sec?: number;
      gen_eval_ms?: number;
      gen_tokens?: number;
      prompt_eval_ms?: number;
      prompt_tokens?: number;
      finish_reason?: string;
      token_breakdown?: TokenBreakdown;
    }>('chat-done', (event) => {
      if (settled || wasAborted) return;
      const { payload } = event;

      if (payload.type === 'done') {
        const conversationId =
          payload.conversation_id || request.conversation_id || crypto.randomUUID();
        const timings: TimingInfo = {
          promptTokPerSec: payload.prompt_tok_per_sec,
          genTokPerSec: payload.gen_tok_per_sec,
          genEvalMs: payload.gen_eval_ms,
          genTokens: payload.gen_tokens,
          promptEvalMs: payload.prompt_eval_ms,
          promptTokens: payload.prompt_tokens,
          finishReason: payload.finish_reason,
          tokenBreakdown: payload.token_breakdown,
        };
        safeOnComplete(
          crypto.randomUUID(),
          conversationId,
          payload.tokens_used,
          payload.max_tokens,
          timings,
        );
        const tokenCount = payload.gen_tokens ?? payload.tokens_used;
        const body = tokenCount ? `${tokenCount} tokens` : 'Response ready';
        notifyIfUnfocused('LLaMA Chat', body);
        settle();
      } else if (payload.type === 'error') {
        const errorMessage = payload.error || 'Unknown error';
        safeOnError(errorMessage);
        settle(new Error(errorMessage));
      } else if (payload.type === 'cancelled') {
        settle(new Error('Request aborted'));
      }
    });

    const cleanup = () => {
      unlistenToken();
      unlistenDone();
    };

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
      if (abortSignal.aborted) {
        markAborted();
        return promise;
      }
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
