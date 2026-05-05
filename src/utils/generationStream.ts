/**
 * Unified generation stream interface.
 *
 * Both local (llama.cpp) and remote (OpenAI-compatible) providers implement
 * this interface. The caller doesn't know or care which backend is running.
 */
import { toast } from 'react-hot-toast';

import type { ChatRequest } from '../types';

import type { ChatTransport, TimingInfo } from './chatTransport';
import { isTauriEnv } from './tauri';

const TOAST_DURATION_MS = 5000;
const LIVE_STATS_MIN_ELAPSED_SEC = 0.5;
const SSE_DATA_PREFIX_LENGTH = 6;
const SSE_MAX_TURNS = 50;

// ─── Unified types ───────────────────────────────────────────────────────

export interface GenerationCallbacks {
  /** A new token was generated */
  onToken: (token: string) => void;
  /** Timing/speed stats updated (may fire multiple times during generation) */
  onTimingsUpdate: (timings: Partial<TimingInfo>) => void;
  /** Context token counts updated */
  onContextUpdate: (tokensUsed: number, maxTokens: number) => void;
  /** Status message changed (compaction progress, tool execution, etc.) */
  onStatus: (message: string | undefined) => void;
  /** Generation completed successfully */
  onComplete: (result: GenerationResult) => void;
  /** Generation failed */
  onError: (error: string) => void;
}

export interface GenerationResult {
  conversationId: string;
  timings: TimingInfo;
  tokensUsed?: number;
  maxTokens?: number;
}

export interface GenerationRequest {
  prompt: string;
  conversationId: string | null;
  imageData?: string[];
  autoContinue?: boolean;
}

export interface GenerationStream {
  start(
    request: GenerationRequest,
    callbacks: GenerationCallbacks,
    signal?: AbortSignal,
  ): Promise<void>;
}

// ─── Local model stream (wraps ChatTransport) ────────────────────────────

export class LocalGenerationStream implements GenerationStream {
  constructor(private transport: ChatTransport) {}

  async start(
    request: GenerationRequest,
    callbacks: GenerationCallbacks,
    signal?: AbortSignal,
  ): Promise<void> {
    const chatRequest: ChatRequest = {
      message: request.prompt,
      conversation_id: request.conversationId || undefined,
      image_data: request.imageData,
      auto_continue: request.autoContinue,
    };

    await this.transport.streamMessage(
      chatRequest,
      {
        onToken: (token, tokensUsed, maxTokens, genTokPerSec, genTokens) => {
          callbacks.onToken(token);
          if (tokensUsed !== undefined && maxTokens !== undefined) {
            callbacks.onContextUpdate(tokensUsed, maxTokens);
          }
          if (genTokPerSec !== undefined || genTokens !== undefined) {
            callbacks.onTimingsUpdate({
              genTokPerSec: genTokPerSec ?? undefined,
              genTokens: genTokens ?? undefined,
            });
          }
        },
        onComplete: (_messageId, conversationId, tokensUsed, maxTokens, timings) => {
          callbacks.onComplete({
            conversationId,
            timings: timings ?? {},
            tokensUsed,
            maxTokens,
          });
        },
        onError: (error) => {
          callbacks.onError(error);
        },
        onStatus: (message) => {
          callbacks.onStatus(message);
        },
      },
      signal,
    );
  }
}

// ─── Remote provider stream (SSE or Tauri events) ───────────────────────

interface RemoteStreamConfig {
  provider: string;
  model: string;
  sessionRef: React.MutableRefObject<string | null>;
  providerParams?: Record<string, unknown>;
}

export class RemoteGenerationStream implements GenerationStream {
  constructor(private config: RemoteStreamConfig) {}

  async start(
    request: GenerationRequest,
    callbacks: GenerationCallbacks,
    signal?: AbortSignal,
  ): Promise<void> {
    if (isTauriEnv()) {
      return this.startTauri(request, callbacks, signal);
    }
    return this.startSSE(request, callbacks, signal);
  }

  private async startTauri(
    request: GenerationRequest,
    callbacks: GenerationCallbacks,
    signal?: AbortSignal,
  ): Promise<void> {
    const { provider, model, sessionRef, providerParams } = this.config;

    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const { listen } = await import('@tauri-apps/api/event');

      let accumulated = '';
      let tokenCount = 0;
      const streamStart = Date.now();
      const unlisteners: Array<() => void> = [];

      await new Promise<void>((resolve, reject) => {
        let settled = false;
        const settle = (fn: () => void) => {
          if (settled) return;
          settled = true;
          unlisteners.forEach((ul) => ul());
          fn();
        };

        if (signal) {
          if (signal.aborted) {
            settle(() => resolve());
            return;
          }
          signal.addEventListener('abort', () => settle(() => resolve()), { once: true });
        }

        listen<{ token: string }>('provider-token', (event) => {
          accumulated += event.payload.token;
          tokenCount++;
          callbacks.onToken(event.payload.token);
          const elapsedSec = (Date.now() - streamStart) / 1000;
          if (elapsedSec > LIVE_STATS_MIN_ELAPSED_SEC) {
            callbacks.onTimingsUpdate({
              genTokPerSec: tokenCount / elapsedSec,
              genTokens: tokenCount,
            });
          }
        })
          .then((ul) => unlisteners.push(ul))
          .catch(reject);

        listen<{
          conversation_id: string;
          session_id: string | null;
          stop_reason: string | null;
          cost_usd: number | null;
          duration_ms: number | null;
          input_tokens: number | null;
          output_tokens: number | null;
          model: string | null;
        }>('provider-done', (event) => {
          const d = event.payload;
          if (d.session_id) { sessionRef.current = d.session_id; }

          const outTokens = d.output_tokens ?? Math.round(accumulated.length / 4);
          const durationMs = d.duration_ms ?? 0;
          const timings: TimingInfo = {
            genTokPerSec: durationMs > 0 ? outTokens / (durationMs / 1000) : undefined,
            genEvalMs: durationMs,
            genTokens: outTokens,
            promptTokens: d.input_tokens ?? undefined,
            finishReason: d.stop_reason === 'end_turn' ? 'stop' : (d.stop_reason ?? undefined),
            costUsd: d.cost_usd ?? undefined,
          };

          callbacks.onComplete({
            conversationId: d.conversation_id || request.conversationId || '',
            timings,
          });
          settle(resolve);
        })
          .then((ul) => unlisteners.push(ul))
          .catch(reject);

        invoke<{ conversation_id: string }>('stream_provider', {
          provider,
          model: model || null,
          prompt: request.prompt,
          conversationId: request.conversationId || null,
          sessionId: sessionRef.current || null,
          params: providerParams && Object.keys(providerParams).length > 0
            ? providerParams : null,
        }).catch((err: unknown) => {
          settle(() => reject(err instanceof Error ? err : new Error(String(err))));
        });
      });
    } catch (err) {
      const isAbort =
        (err instanceof DOMException && err.name === 'AbortError') ||
        (err instanceof Error && err.message.includes('aborted'));
      if (!isAbort) {
        const msg = err instanceof Error ? err.message : 'Provider request failed';
        callbacks.onError(msg);
        toast.error(msg, { duration: TOAST_DURATION_MS });
      }
    }
  }

  private async startSSE(
    request: GenerationRequest,
    callbacks: GenerationCallbacks,
    signal?: AbortSignal,
  ): Promise<void> {
    const { provider, model, sessionRef, providerParams } = this.config;

    try {
      const resp = await fetch(`/api/providers/${provider}/stream`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        signal,
        body: JSON.stringify({
          prompt: request.prompt,
          model,
          max_turns: SSE_MAX_TURNS,
          session_id: sessionRef.current || undefined,
          conversation_id: request.conversationId || undefined,
          ...(providerParams && Object.keys(providerParams).length > 0
            ? { params: providerParams } : {}),
        }),
      });

      const reader = resp.body?.getReader();
      const decoder = new TextDecoder();
      let accumulated = '';
      let tokenCount = 0;
      const streamStart = Date.now();

      if (reader) {
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;

          const chunk = decoder.decode(value, { stream: true });
          const lines = chunk.split('\n');

          for (const line of lines) {
            if (!line.startsWith('data: ')) continue;
            const jsonStr = line.slice(SSE_DATA_PREFIX_LENGTH);
            try {
              const event = JSON.parse(jsonStr);
              if (event.type === 'token') {
                accumulated += event.token;
                tokenCount++;
                callbacks.onToken(event.token);
                // Live speed estimate from token count + elapsed time
                const elapsedSec = (Date.now() - streamStart) / 1000;
                if (elapsedSec > LIVE_STATS_MIN_ELAPSED_SEC) {
                  callbacks.onTimingsUpdate({
                    genTokPerSec: tokenCount / elapsedSec,
                    genTokens: tokenCount,
                  });
                }
              } else if (event.type === 'status') {
                const outTok = event.output_tokens || 0;
                const dur = event.duration_ms || 0;
                const tokPerSec = dur > 0 ? outTok / (dur / 1000) : 0;
                const inTok = event.input_tokens || 0;
                const inLabel = inTok > 0 ? `in: ${inTok.toLocaleString()}  ` : '';
                callbacks.onStatus(
                  `${inLabel}out: ${outTok.toLocaleString()} · ${tokPerSec.toFixed(1)} tok/s`,
                );
                callbacks.onTimingsUpdate({
                  genTokPerSec: tokPerSec,
                  genTokens: outTok,
                  genEvalMs: dur,
                  costUsd: event.cost_usd,
                });
              } else if (event.type === 'done') {
                if (event.session_id) { sessionRef.current = event.session_id; }

                const outTokens = event.output_tokens || Math.round(accumulated.length / 4);
                const inTokens = event.input_tokens || 0;
                const durationMs = event.duration_ms || 0;
                const timings: TimingInfo = {
                  genTokPerSec: durationMs > 0 ? outTokens / (durationMs / 1000) : undefined,
                  genEvalMs: durationMs,
                  genTokens: outTokens,
                  promptTokens: inTokens,
                  finishReason: event.stop_reason === 'end_turn' ? 'stop' : event.stop_reason,
                  costUsd: event.cost_usd,
                };

                callbacks.onComplete({
                  conversationId: event.conversation_id || request.conversationId || '',
                  timings,
                });
              }
            } catch {
              /* skip unparseable lines */
            }
          }
        }
      }
    } catch (err) {
      const isAbort =
        (err instanceof DOMException && err.name === 'AbortError') ||
        (err instanceof Error && err.message.includes('aborted'));
      if (!isAbort) {
        const msg = err instanceof Error ? err.message : 'Provider request failed';
        callbacks.onError(msg);
        toast.error(msg, { duration: TOAST_DURATION_MS });
      }
    }
  }
}

// ─── Factory ─────────────────────────────────────────────────────────────

export function createGenerationStream(
  provider: string,
  config: {
    transport: ChatTransport;
    model: string;
    sessionRef: React.MutableRefObject<string | null>;
    providerParams?: Record<string, unknown>;
  },
): GenerationStream {
  if (provider === 'local') {
    return new LocalGenerationStream(config.transport);
  }
  return new RemoteGenerationStream({
    provider,
    model: config.model,
    sessionRef: config.sessionRef,
    providerParams: config.providerParams,
  });
}
