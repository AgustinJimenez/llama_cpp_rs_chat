import { toast } from 'react-hot-toast';

const SSE_DATA_PREFIX_LENGTH = 6;

import type { Message } from '../types';
import type { TimingInfo } from '../utils/chatTransport';
import { isTauriEnv } from '../utils/tauri';

export interface StreamCloudProviderParams {
  provider: string;
  model: string;
  prompt: string;
  conversationId: string | null;
  sessionRef: React.MutableRefObject<string | null>;
  abortController: AbortController | null;
  assistantMessageId: string;
  setMessages: React.Dispatch<React.SetStateAction<Message[]>>;
  setError: (e: string | null) => void;
  setIsLoading: (v: boolean) => void;
  setLastTimings: (t: TimingInfo | undefined) => void;
  setCurrentConversationId: (id: string | null) => void;
  isStreamingRef: React.MutableRefObject<boolean>;
}

/**
 * Handles cloud provider streaming via Tauri events (desktop app).
 * Uses `stream_provider` Tauri command + listens for `provider-token` / `provider-done` events.
 */
async function streamCloudProviderTauri(params: StreamCloudProviderParams): Promise<void> {
  const {
    provider,
    model,
    prompt,
    conversationId,
    sessionRef,
    assistantMessageId,
    setMessages,
    setError,
    setIsLoading,
    setLastTimings,
    setCurrentConversationId,
    isStreamingRef,
  } = params;

  // eslint-disable-next-line no-console
  console.log('[useChat] Using Tauri provider command:', provider, model);
  setLastTimings(undefined);
  isStreamingRef.current = true;

  try {
    const { invoke } = await import('@tauri-apps/api/core');
    const { listen } = await import('@tauri-apps/api/event');

    let accumulated = '';
    const unlisteners: Array<() => void> = [];

    await new Promise<void>((resolve, reject) => {
      let settled = false;
      const settle = (fn: () => void) => {
        if (settled) return;
        settled = true;
        unlisteners.forEach((ul) => ul());
        fn();
      };

      // Abort support: stop listening if the controller fires
      if (params.abortController?.signal) {
        const signal = params.abortController.signal;
        if (signal.aborted) {
          settle(() => resolve());
          return;
        }
        signal.addEventListener('abort', () => settle(() => resolve()), { once: true });
      }

      listen<{ token: string }>('provider-token', (event) => {
        accumulated += event.payload.token;
        setMessages((prev) =>
          prev.map((msg) =>
            msg.id === assistantMessageId ? { ...msg, content: accumulated } : msg,
          ),
        );
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
        if (d.session_id) sessionRef.current = d.session_id;
        if (d.conversation_id && !conversationId) {
          setCurrentConversationId(d.conversation_id);
        }
        setTimeout(() => {
          window.dispatchEvent(new CustomEvent('conversation-title-updated'));
        }, 500);

        const outTokens = d.output_tokens ?? Math.round(accumulated.length / 4);
        const durationMs = d.duration_ms ?? 0;
        const timings: TimingInfo = {
          genTokPerSec: durationMs > 0 ? outTokens / (durationMs / 1000) : undefined,
          genEvalMs: durationMs,
          genTokens: outTokens,
          finishReason: d.stop_reason === 'end_turn' ? 'stop' : (d.stop_reason ?? undefined),
          costUsd: d.cost_usd ?? undefined,
        };
        setMessages((prev) =>
          prev.map((msg) =>
            msg.id === assistantMessageId ? { ...msg, timestamp: Date.now(), timings } : msg,
          ),
        );
        setLastTimings(timings);
        settle(resolve);
      })
        .then((ul) => unlisteners.push(ul))
        .catch(reject);

      invoke<{ conversation_id: string }>('stream_provider', {
        provider,
        model: model || null,
        prompt,
        conversationId: conversationId || null,
        sessionId: sessionRef.current || null,
      }).catch((err: unknown) => {
        settle(() =>
          reject(err instanceof Error ? err : new Error(String(err)))
        );
      });
    });
  } catch (err) {
    const msg = err instanceof Error ? err.message : 'Provider request failed';
    setError(msg);
    toast.error(msg, { duration: 5000 });
  } finally {
    isStreamingRef.current = false;
    setIsLoading(false);
  }
}

/**
 * Handles SSE streaming for cloud/CLI-backed providers (web browser).
 * Extracted from useChat's sendMessage to separate concerns.
 */
export async function streamCloudProvider(params: StreamCloudProviderParams): Promise<void> {
  if (isTauriEnv()) {
    return streamCloudProviderTauri(params);
  }

  const {
    provider,
    model,
    prompt,
    conversationId,
    sessionRef,
    abortController,
    assistantMessageId,
    setMessages,
    setError,
    setIsLoading,
    setLastTimings,
    setCurrentConversationId,
    isStreamingRef,
  } = params;

  console.log('[useChat] Using CLI provider (SSE):', provider, model); // eslint-disable-line no-console
  setLastTimings(undefined);
  isStreamingRef.current = true;
  try {
    const resp = await fetch(`/api/providers/${provider}/stream`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      signal: abortController?.signal,
      body: JSON.stringify({
        prompt,
        model,
        max_turns: 50,
        session_id: sessionRef.current || undefined,
        conversation_id: conversationId || undefined,
      }),
    });

    const reader = resp.body?.getReader();
    const decoder = new TextDecoder();
    let accumulated = '';

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
              setMessages((prev) =>
                prev.map((msg) =>
                  msg.id === assistantMessageId ? { ...msg, content: accumulated } : msg,
                ),
              );
            } else if (event.type === 'done') {
              if (event.session_id) sessionRef.current = event.session_id;
              if (event.conversation_id && !conversationId) {
                setCurrentConversationId(event.conversation_id);
              }
              // Trigger sidebar refresh so the new conversation appears in the list
              setTimeout(() => {
                window.dispatchEvent(new CustomEvent('conversation-title-updated'));
              }, 500);
              const outTokens = event.output_tokens || Math.round(accumulated.length / 4);
              const durationMs = event.duration_ms || 0;
              const timings: TimingInfo = {
                genTokPerSec: durationMs > 0 ? outTokens / (durationMs / 1000) : undefined,
                genEvalMs: durationMs,
                genTokens: outTokens,
                finishReason: event.stop_reason === 'end_turn' ? 'stop' : event.stop_reason,
                costUsd: event.cost_usd,
              };
              setMessages((prev) =>
                prev.map((msg) =>
                  msg.id === assistantMessageId ? { ...msg, timestamp: Date.now(), timings } : msg,
                ),
              );
              setLastTimings(timings);
            }
          } catch {
            /* skip unparseable lines */
          }
        }
      }
    }
  } catch (err) {
    const msg = err instanceof Error ? err.message : 'Provider request failed';
    setError(msg);
    toast.error(msg, { duration: 5000 });
  } finally {
    isStreamingRef.current = false;
    setIsLoading(false);
  }
}
