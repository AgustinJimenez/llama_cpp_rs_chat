/* eslint-disable max-lines */
import { useState, useCallback, useEffect, useRef } from 'react';
import { flushSync } from 'react-dom';
import { toast } from 'react-hot-toast';

const CLOUD_POLL_INTERVAL_MS = 2000;
const CONTINUE_DELAY_AFTER_RECOVERY_MS = 1500;

import type { Message } from '../types';
import { createChatTransport } from '../utils/chatTransport';
import type { TimingInfo } from '../utils/chatTransport';
import { getConversation, getModelStatus, truncateConversation } from '../utils/tauriCommands';
import { logToastError } from '../utils/toastLogger';

import { useConnection } from './useConnection';
import { useConversationUrl } from './useConversationUrl';
import { useConversationWatcher } from './useConversationWatcher';
import { useGenerationStream } from './useGenerationStream';

// Auto-continue reasons (used by the polling reconnect effect)
const AUTO_CONTINUE_REASONS = new Set(['length', 'yn_continue', 'loop_recovery', 'infinite_loop']);

// eslint-disable-next-line max-lines-per-function
export function useChat() {
  const { connected } = useConnection();
  const connectedRef = useRef(connected);
  connectedRef.current = connected;

  // Provider state (synced from ModelContext via App.tsx)
  const providerRef = useRef<{ provider: string; model: string }>({ provider: 'local', model: '' });
  const providerParamsRef = useRef<Record<string, unknown>>({});
  const providerSessionRef = useRef<string | null>(null);

  // Core state
  const [messages, setMessages] = useState<Message[]>([]);
  const [currentConversationId, setCurrentConversationId] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [tokensUsed, setTokensUsed] = useState<number | undefined>(undefined);
  const [maxTokens, setMaxTokens] = useState<number | undefined>(undefined);
  const [lastTimings, setLastTimings] = useState<TimingInfo | undefined>(undefined);
  const [streamStatus, setStreamStatus] = useState<string | undefined>(undefined);
  const messagesRef = useRef<Message[]>([]);
  const currentConversationIdRef = useRef<string | null>(null);

  const transportRef = useRef(createChatTransport());

  // Sync refs
  useEffect(() => {
    messagesRef.current = messages;
  }, [messages]);
  useEffect(() => {
    currentConversationIdRef.current = currentConversationId;
  }, [currentConversationId]);

  // ─── Unified generation stream ─────────────────────────────────────────

  const {
    startGeneration,
    abortGeneration,
    resetAutoContinue,
    isStreamingRef,
    abortControllerRef,
    autoContinueCountRef,
  } = useGenerationStream({
    setMessages,
    setIsLoading,
    setError,
    setLastTimings,
    setTokensUsed,
    setMaxTokens,
    setStreamStatus,
    setCurrentConversationId,
    currentConversationIdRef,
    messagesRef,
    providerRef,
    providerParamsRef,
    providerSessionRef,
    transportRef,
  });

  // ─── Clear ─────────────────────────────────────────────────────────────

  const clearMessages = useCallback(() => {
    setMessages([]);
    setCurrentConversationId(null);
    setError(null);
    setTokensUsed(undefined);
    setMaxTokens(undefined);
    setLastTimings(undefined);
    providerSessionRef.current = null;
  }, []);

  // Derive lastTimings from last assistant message (persists across loads)
  useEffect(() => {
    if (isStreamingRef.current) return;
    for (let i = messages.length - 1; i >= 0; i--) {
      if (messages[i].role === 'assistant' && messages[i].timings) {
        setLastTimings(messages[i].timings);
        return;
      }
    }
  }, [messages, isStreamingRef]);

  // ─── Load conversation ─────────────────────────────────────────────────

  const loadConversation = useCallback(async (filename: string) => {
    if (!connectedRef.current) {
      toast.error('Server is unreachable — please wait for reconnection', {
        duration: 3000,
        id: 'server-down',
      });
      return;
    }
    setIsLoading(true);
    setError(null);
    setTokensUsed(undefined);
    setMaxTokens(undefined);

    try {
      const data = await getConversation(filename);
      providerSessionRef.current =
        data.provider_id === providerRef.current.provider ? data.provider_session_id || null : null;
      if (data.messages && data.messages.length > 0) {
        const mapped = (data.messages as Array<Record<string, unknown>>).map((msg) => {
          const m: Message = {
            id: String(msg.id),
            role: String(msg.role) as Message['role'],
            content: String(msg.content),
            timestamp: Number(msg.timestamp),
          };
          if (msg.gen_tok_per_sec != null) {
            m.timings = {
              promptTokPerSec: msg.prompt_tok_per_sec as number,
              genTokPerSec: msg.gen_tok_per_sec as number,
              genEvalMs: msg.gen_eval_ms as number,
              genTokens: msg.gen_tokens as number,
              promptEvalMs: msg.prompt_eval_ms as number,
              promptTokens: msg.prompt_tokens as number,
            };
          }
          if (msg.compacted) m.compacted = true;
          return m;
        });
        let systemPromptSeen = false;
        const filtered = mapped.filter((msg: Message) => {
          if (msg.role === 'system') {
            // Always show compaction summaries and crash recovery messages
            if (msg.content.startsWith('[Conversation summary')) return true;
            if (msg.content.startsWith('[System:')) return true;
            if (!systemPromptSeen) {
              systemPromptSeen = true;
              msg.isSystemPrompt = true;
              return true;
            }
            return false;
          }
          return !msg.content.startsWith('[TOOL_RESULTS]');
        });
        setMessages(filtered);
        setCurrentConversationId(filename);
      }
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to load conversation';
      setError(errorMessage);
      const display = `Failed to load conversation: ${errorMessage}`;
      toast.error(display, { duration: 5000 });
      logToastError('useChat.loadConversation', display, err);
    } finally {
      setIsLoading(false);
    }
  }, []);

  // ─── Send message ──────────────────────────────────────────────────────

  const sendMessage = useCallback(
    async (content: string, imageData?: string[], bypassLoadingCheck = false) => {
      if (!connectedRef.current) {
        toast.error('Server is unreachable — please wait for reconnection', {
          duration: 3000,
          id: 'server-down',
        });
        return;
      }
      const hasImages = imageData && imageData.length > 0;
      const trimmed = content.trim();
      if (!trimmed && !hasImages) return;

      // Queue message if remote provider is already generating
      if (
        !bypassLoadingCheck &&
        isLoading &&
        currentConversationId &&
        providerRef.current.provider !== 'local'
      ) {
        const { queueMessage } = await import('../utils/tauriCommands');
        try {
          await queueMessage(currentConversationId, trimmed);
          setMessages((prev) => [
            ...prev,
            {
              id: crypto.randomUUID(),
              role: 'user' as const,
              content: trimmed,
              timestamp: Date.now(),
            },
          ]);
          toast.success('Message queued — will be injected on next iteration', { duration: 2000 });
        } catch {
          toast.error('Failed to queue message');
        }
        return;
      }
      if (!bypassLoadingCheck && isLoading) return;

      resetAutoContinue();
      abortControllerRef.current?.abort();
      abortControllerRef.current = new AbortController();

      setMessages((prev) => [
        ...prev,
        {
          id: crypto.randomUUID(),
          role: 'user' as const,
          content: trimmed,
          timestamp: Date.now(),
          image_data: hasImages ? imageData : undefined,
        },
      ]);
      setIsLoading(true);
      setError(null);

      const assistantMessageId = crypto.randomUUID();

      flushSync(() => {
        setMessages((prev) => [
          ...prev,
          { id: assistantMessageId, role: 'assistant' as const, content: '', timestamp: 0 },
        ]);
      });

      await startGeneration(
        {
          prompt: trimmed,
          conversationId: currentConversationId,
          imageData: hasImages ? imageData : undefined,
        },
        assistantMessageId,
      );
    },
    [isLoading, currentConversationId, startGeneration, resetAutoContinue, abortControllerRef],
  );

  // ─── URL persistence + watcher ─────────────────────────────────────────

  useConversationUrl({ currentConversationId, loadConversation });
  useConversationWatcher({
    currentConversationId,
    isStreamingRef,
    currentMessagesRef: messagesRef,
    setMessages,
    setTokensUsed,
    setMaxTokens,
    setIsLoading,
  });

  // ─── Edit message ──────────────────────────────────────────────────────

  const editMessage = useCallback(
    async (messageIndex: number, newContent: string) => {
      if (!connectedRef.current) {
        toast.error('Server is unreachable — please wait for reconnection', {
          duration: 3000,
          id: 'server-down',
        });
        return;
      }
      if (isLoading) return;

      const systemMsgsBefore = messagesRef.current
        .slice(0, messageIndex)
        .filter((m) => m.role === 'system').length;
      const fromSequence = messageIndex - systemMsgsBefore + 1;

      if (currentConversationId) {
        try {
          await truncateConversation(currentConversationId, fromSequence);
          providerSessionRef.current = null;
        } catch (err) {
          const msg = err instanceof Error ? err.message : 'Failed to truncate conversation';
          toast.error(msg, { duration: 5000 });
          return;
        }
      }

      setMessages((prev) => prev.slice(0, messageIndex));
      await sendMessage(newContent, undefined, true);
    },
    [isLoading, currentConversationId, sendMessage],
  );

  // ─── Regenerate ────────────────────────────────────────────────────────

  const regenerateFrom = useCallback(
    async (messageIndex: number) => {
      if (!connectedRef.current) {
        toast.error('Server is unreachable — please wait for reconnection', {
          duration: 3000,
          id: 'server-down',
        });
        return;
      }
      if (isLoading) return;

      const msgs = messagesRef.current;
      let userMsgIndex = messageIndex - 1;
      while (userMsgIndex >= 0 && msgs[userMsgIndex]?.role !== 'user') {
        userMsgIndex--;
      }
      if (userMsgIndex < 0) return;

      const userContent = msgs[userMsgIndex].content;
      const userImages = msgs[userMsgIndex].image_data;

      const systemMsgsBefore = msgs
        .slice(0, messageIndex)
        .filter((m) => m.role === 'system').length;
      const fromSequence = messageIndex - systemMsgsBefore + 1;

      if (currentConversationId) {
        try {
          await truncateConversation(currentConversationId, fromSequence);
          providerSessionRef.current = null;
        } catch (err) {
          const msg = err instanceof Error ? err.message : 'Failed to truncate conversation';
          toast.error(msg, { duration: 5000 });
          return;
        }
      }

      setMessages((prev) => prev.slice(0, messageIndex));
      abortControllerRef.current?.abort();
      abortControllerRef.current = new AbortController();
      setIsLoading(true);
      setError(null);

      const assistantMessageId = crypto.randomUUID();

      flushSync(() => {
        setMessages((prev) => [
          ...prev,
          { id: assistantMessageId, role: 'assistant' as const, content: '', timestamp: 0 },
        ]);
      });

      await startGeneration(
        {
          prompt: userContent,
          conversationId: currentConversationId,
          imageData: userImages,
          autoContinue: true,
        },
        assistantMessageId,
      );
    },
    [isLoading, currentConversationId, startGeneration, abortControllerRef],
  );

  // ─── Continue ──────────────────────────────────────────────────────────

  const continueFrom = useCallback(
    async (messageIndex: number) => {
      if (!connectedRef.current) {
        toast.error('Server is unreachable — please wait for reconnection', {
          duration: 3000,
          id: 'server-down',
        });
        return;
      }
      if (isLoading) return;
      if (!currentConversationId) return;

      const msgs = messagesRef.current;
      const target = msgs[messageIndex];
      if (!target || target.role !== 'assistant') return;

      abortControllerRef.current?.abort();
      abortControllerRef.current = new AbortController();
      setIsLoading(true);
      setError(null);

      const assistantMessageId = crypto.randomUUID();

      flushSync(() => {
        setMessages((prev) => [
          ...prev,
          { id: assistantMessageId, role: 'assistant' as const, content: '', timestamp: 0 },
        ]);
      });

      await startGeneration(
        {
          prompt:
            'Continue from your last response exactly where you left off. Do not repeat previous text unless necessary.',
          conversationId: currentConversationId,
          autoContinue: true,
        },
        assistantMessageId,
      );
    },
    [isLoading, currentConversationId, startGeneration, abortControllerRef],
  );

  // ─── Stop ──────────────────────────────────────────────────────────────

  const stopGeneration = useCallback(() => {
    abortGeneration();
  }, [abortGeneration]);

  // ─── Crash recovery message ─────────────────────────────────────────────

  useEffect(() => {
    const handler = () => {
      setMessages((prev) => [
        ...prev,
        {
          id: crypto.randomUUID(),
          role: 'system' as const,
          content:
            '[System: Generation was interrupted due to a temporary issue. The model is being reloaded and will continue automatically.]',
          timestamp: Date.now(),
        },
      ]);
    };
    window.addEventListener('model-crash-recovery', handler);
    return () => window.removeEventListener('model-crash-recovery', handler);
  }, []);

  // Auto-continue after model crash recovery
  useEffect(() => {
    const handler = () => {
      const convId = currentConversationIdRef.current;
      if (!convId) return;
      console.log('[useChat] Model recovered — auto-continuing generation'); // eslint-disable-line no-console
      // Small delay to let model fully initialize
      setTimeout(() => {
        abortControllerRef.current = new AbortController();
        const aid = crypto.randomUUID();
        flushSync(() => {
          setMessages((prev) => [
            ...prev,
            { id: aid, role: 'assistant' as const, content: '', timestamp: 0 },
          ]);
        });
        setIsLoading(true);
        startGeneration(
          {
            prompt:
              'Continue from where you left off. The system recovered from a temporary interruption.',
            conversationId: convId,
            autoContinue: true,
          },
          aid,
        ).catch(() => setIsLoading(false));
      }, CONTINUE_DELAY_AFTER_RECOVERY_MS);
    };
    window.addEventListener('model-crash-recovered', handler);
    return () => window.removeEventListener('model-crash-recovered', handler);
  }, [startGeneration, abortControllerRef]);

  // ─── Polling reconnect (after page refresh / conversation switch) ──────

  useEffect(() => {
    if (!currentConversationId || isStreamingRef.current) return;

    let active = true;
    let intervalId: ReturnType<typeof setInterval> | null = null;

    const startPolling = async () => {
      try {
        const status = (await getModelStatus()) as {
          generating?: boolean;
          active_conversation_id?: string;
          status_message?: string;
          last_finish_reason?: string;
        };
        if (!active) return;

        const convIdClean = currentConversationId;
        const activeClean = status.active_conversation_id;

        if (!status.generating || activeClean !== convIdClean) {
          const finishReason = status.last_finish_reason;
          if (AUTO_CONTINUE_REASONS.has(finishReason ?? '') && autoContinueCountRef.current < 3) {
            autoContinueCountRef.current += 1;
            console.warn(
              `[useChat] Auto-continue ${autoContinueCountRef.current}/3 (${finishReason}, detected on load)`,
            );
            setIsLoading(true);
            setTimeout(() => {
              if (!currentConversationId) return;
              abortControllerRef.current = new AbortController();
              const aid = crypto.randomUUID();
              flushSync(() => {
                setMessages((prev) => [
                  ...prev,
                  { id: aid, role: 'assistant' as const, content: '', timestamp: 0 },
                ]);
              });
              startGeneration(
                { prompt: 'Continue', conversationId: currentConversationId, autoContinue: true },
                aid,
              ).catch(() => setIsLoading(false));
            }, 1000);
          } else {
            setIsLoading(false);
            setStreamStatus(undefined);
          }
          return;
        }

        // Active generation — start polling DB for updates
        console.warn('[useChat] Reconnecting to active generation via polling');
        setIsLoading(true);

        intervalId = setInterval(async () => {
          if (!active) return;
          try {
            const s = (await getModelStatus()) as {
              generating?: boolean;
              active_conversation_id?: string;
              status_message?: string;
              last_finish_reason?: string;
            };
            const stillActive = s.generating && s.active_conversation_id === convIdClean;
            setStreamStatus(s.status_message || undefined);

            const data = await getConversation(currentConversationId);
            if (!active) return;
            if (data.messages) {
              const mapped = (data.messages as Array<Record<string, unknown>>).map((msg) => ({
                id: String(msg.id),
                role: String(msg.role) as Message['role'],
                content: String(msg.content),
                timestamp: Number(msg.timestamp),
                ...(msg.compacted ? { compacted: true } : {}),
                ...(msg.gen_tok_per_sec != null
                  ? {
                      timings: {
                        promptTokPerSec: msg.prompt_tok_per_sec as number,
                        genTokPerSec: msg.gen_tok_per_sec as number,
                        genEvalMs: msg.gen_eval_ms as number,
                        genTokens: msg.gen_tokens as number,
                        promptEvalMs: msg.prompt_eval_ms as number,
                        promptTokens: msg.prompt_tokens as number,
                      },
                    }
                  : {}),
              }));
              let systemPromptSeen = false;
              const filtered = mapped.filter((msg: Message) => {
                if (msg.role === 'system') {
                  // Always show compaction summaries and crash recovery messages
                  if (msg.content.startsWith('[Conversation summary')) return true;
                  if (msg.content.startsWith('[System:')) return true;
                  if (!systemPromptSeen) {
                    systemPromptSeen = true;
                    return true;
                  }
                  return false;
                }
                return !msg.content.startsWith('[TOOL_RESULTS]');
              });
              setMessages(filtered);
            }

            if (!stillActive) {
              const finishReason = s.last_finish_reason;
              if (
                AUTO_CONTINUE_REASONS.has(finishReason ?? '') &&
                autoContinueCountRef.current < 3
              ) {
                autoContinueCountRef.current += 1;
                if (intervalId) clearInterval(intervalId);
                setTimeout(() => {
                  if (!currentConversationId) return;
                  abortControllerRef.current = new AbortController();
                  const aid = crypto.randomUUID();
                  flushSync(() => {
                    setMessages((prev) => [
                      ...prev,
                      { id: aid, role: 'assistant' as const, content: '', timestamp: 0 },
                    ]);
                  });
                  startGeneration(
                    {
                      prompt: 'Continue',
                      conversationId: currentConversationId,
                      autoContinue: true,
                    },
                    aid,
                  ).catch(() => setIsLoading(false));
                }, 1000);
                return;
              }
              console.warn('[useChat] Generation completed (detected via polling)');
              setIsLoading(false);
              setStreamStatus(undefined);
              if (intervalId) clearInterval(intervalId);
            }
          } catch {
            // ignore polling errors
          }
        }, CLOUD_POLL_INTERVAL_MS);
      } catch {
        // ignore
      }
    };

    const timeout = setTimeout(startPolling, 1000);
    return () => {
      active = false;
      clearTimeout(timeout);
      if (intervalId) clearInterval(intervalId);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentConversationId]);

  // ─── Return ────────────────────────────────────────────────────────────

  return {
    messages,
    isLoading,
    error,
    sendMessage,
    editMessage,
    regenerateFrom,
    continueFrom,
    stopGeneration,
    clearMessages,
    loadConversation,
    currentConversationId,
    tokensUsed,
    maxTokens,
    lastTimings,
    streamStatus,
    providerRef,
    providerParamsRef,
  };
}
