import { toast } from 'react-hot-toast';

const SSE_DATA_PREFIX_LENGTH = 6;

import type { Message } from '../types';
import type { TimingInfo } from '../utils/chatTransport';

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
 * Handles SSE streaming for cloud/CLI-backed providers.
 * Extracted from useChat's sendMessage to separate concerns.
 */
export async function streamCloudProvider(params: StreamCloudProviderParams): Promise<void> {
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
