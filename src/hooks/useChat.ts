/* eslint-disable max-lines */
import { useState, useCallback, useEffect, useRef } from 'react';
import { flushSync } from 'react-dom';
import { toast } from 'react-hot-toast';

const CLOUD_POLL_INTERVAL_MS = 2000;

import { useAgentContext } from '../contexts/AgentContext';
import type { Message } from '../types';
import { createChatTransport } from '../utils/chatTransport';
import type { TimingInfo } from '../utils/chatTransport';
import { getConversation, getModelStatus, truncateConversation } from '../utils/tauriCommands';
import { logToastError } from '../utils/toastLogger';
import { generateId } from '../utils/messageUtils';

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
  const { stagedAgent, conversationAgent } = useAgentContext();
  // eslint-disable-next-line react-hooks/refs
  connectedRef.current = connected;

  // Provider state (synced from ModelContext via App.tsx)
  const providerRef = useRef<{ provider: string; model: string }>({ provider: 'local', model: '' });
  const providerParamsRef = useRef<Record<string, unknown>>({});
  const providerSessionRef = useRef<string | null>(null);

  // Core state
  const [messages, setMessages] = useState<Message[]>([]);
  const [currentConversationId, setCurrentConversationId] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [queuedMessage, setQueuedMessage] = useState<{ content: string; images?: string[] } | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const [tokensUsed, setTokensUsed] = useState<number | undefined>(undefined);
  const [maxTokens, setMaxTokens] = useState<number | undefined>(undefined);
  const [lastTimings, setLastTimings] = useState<TimingInfo | undefined>(undefined);
  const [streamStatus, setStreamStatus] = useState<string | undefined>(undefined);
  const [currentConversationWorkerId, setCurrentConversationWorkerId] = useState<string | null>(
    null,
  );
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
    pendingApproval,
    clearPendingApproval,
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
    setQueuedMessage(null);
    providerSessionRef.current = null;
  }, []);

  const cancelQueuedMessage = useCallback(() => setQueuedMessage(null), []);

  // Auto-send queued message when generation finishes (local provider only)
  useEffect(() => {
    if (!isLoading && queuedMessage) {
      const msg = queuedMessage;
      setQueuedMessage(null);
      // eslint-disable-next-line react-hooks/immutability
      sendMessage(msg.content, msg.images, true);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isLoading]);

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

  // When the server reconnects after a disconnect, clear any stale streaming state.
  // The server restarted — there's no active generation to resume — so we abort locally
  // to unblock the UI immediately rather than waiting for a timeout.
  const prevConnectedRef = useRef(connected);
  useEffect(() => {
    const wasDisconnected = !prevConnectedRef.current;
    prevConnectedRef.current = connected;
    if (wasDisconnected && connected && isStreamingRef.current) {
      abortGeneration();
    }
  }, [connected, abortGeneration, isStreamingRef]);

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
          if (msg.sequence_order != null) m.sequenceOrder = msg.sequence_order as number;
          if (msg.title) m.title = String(msg.title);
          return m;
        });
        // Assign persisted tool timings to messages by sequential index.
        // The API returns tool_timings in the order they were executed;
        // each assistant message may contain multiple tool calls (counted by <tool_call> tags).
        const timings =
          (data.tool_timings as Array<{ name: string; duration_ms: number }> | undefined) ?? [];
        if (timings.length > 0) {
          let timingIdx = 0;
          for (const m of mapped) {
            if (m.role === 'assistant' && timingIdx < timings.length) {
              const callCount = (m.content.match(/<tool_call>/g) ?? []).length;
              if (callCount > 0) {
                m.toolCallTimings = timings
                  .slice(timingIdx, timingIdx + callCount)
                  .map((t) => t.duration_ms);
                timingIdx += callCount;
              }
            }
          }
        }
        let systemPromptSeen = false;
        const filtered = mapped.filter((msg: Message) => {
          if (msg.role === 'system') {
            // Always show compaction summaries and crash recovery messages
            if (
              msg.content.startsWith('[Conversation summary') ||
              msg.content.startsWith('[Compacted history')
            ) {
              return true;
            }
            if (msg.content.startsWith('[System:')) {
              return true;
            }
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
    } catch (error_) {
      const errorMessage = error_ instanceof Error ? error_.message : 'Failed to load conversation';
      setError(errorMessage);
      const display = `Failed to load conversation: ${errorMessage}`;
      toast.error(display, { duration: 5000 });
      logToastError('useChat.loadConversation', display, error_);
    } finally {
      setIsLoading(false);
    }
  }, []);

  // ─── Send message ──────────────────────────────────────────────────────

  const applyRemoteAgentProvider = useCallback(
    (effectiveAgent: NonNullable<typeof conversationAgent>) => {
      const saved = { provider: providerRef.current, params: providerParamsRef.current };
      providerRef.current = {
        provider: effectiveAgent.provider_id,
        model: effectiveAgent.provider_model ?? '',
      };
      const agentParams: Record<string, unknown> = {};
      if (effectiveAgent.temperature !== undefined) {
        agentParams.temperature = effectiveAgent.temperature;
      }
      if (effectiveAgent.top_p !== undefined) {
        agentParams.top_p = effectiveAgent.top_p;
      }
      if (effectiveAgent.frequency_penalty !== undefined) {
        agentParams.frequency_penalty = effectiveAgent.frequency_penalty;
      }
      if (effectiveAgent.presence_penalty !== undefined) {
        agentParams.presence_penalty = effectiveAgent.presence_penalty;
      }
      providerParamsRef.current = agentParams;
      return saved;
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  const queueForRemoteProvider = useCallback(
    async (conversationId: string, trimmed: string): Promise<boolean> => {
      const { queueMessage } = await import('../utils/tauriCommands');
      try {
        await queueMessage(conversationId, trimmed);
        setMessages((prev) => [
          ...prev,
          {
            id: generateId(),
            role: 'user' as const,
            content: trimmed,
            timestamp: Date.now(),
          },
        ]);
        toast.success('Message queued — will be injected on next iteration', { duration: 2000 });
      } catch {
        toast.error('Failed to queue message');
      }
      return true;
    },
    [setMessages],
  );

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

      if (!bypassLoadingCheck && isLoading) {
        if (currentConversationId && providerRef.current.provider !== 'local') {
          await queueForRemoteProvider(currentConversationId, trimmed);
          return;
        }
        setQueuedMessage({ content: trimmed, images: hasImages ? imageData : undefined });
        return;
      }

      resetAutoContinue();
      abortControllerRef.current?.abort();
      abortControllerRef.current = new AbortController();

      setMessages((prev) => [
        ...prev,
        {
          id: generateId(),
          role: 'user' as const,
          content: trimmed,
          timestamp: Date.now(),
          image_data: hasImages ? imageData : undefined,
        },
      ]);
      setIsLoading(true);
      setError(null);
      setTokensUsed(undefined);
      setMaxTokens(undefined);

      const assistantMessageId = generateId();

      flushSync(() => {
        setMessages((prev) => [
          ...prev,
          { id: assistantMessageId, role: 'assistant' as const, content: '', timestamp: 0 },
        ]);
      });

      // If this is a new conversation (no ID yet), notify sidebar immediately
      // so it shows the new entry without waiting for generation to finish.
      if (!currentConversationId) {
        const NEW_CONV_SIDEBAR_DELAY_MS = 600; // enough for the DB record to be created
        setTimeout(() => {
          window.dispatchEvent(new CustomEvent('conversation-started'));
        }, NEW_CONV_SIDEBAR_DELAY_MS);
      }

      // When starting a new conversation right after browsing an old one, conversationAgent
      // still holds that old conversation's agent (it's what the header displays as active;
      // see ChatHeader's activeAgent fallback) even though stagedAgent was cleared. Fall back
      // to it so the request actually carries the agent the UI claims is selected.
      const effectiveNewChatAgent = stagedAgent ?? conversationAgent;
      // For new conversations: always pass agentId so backend records the association.
      // For existing conversations: conversationAgent is already set by the backend.
      const agentId = !currentConversationId ? effectiveNewChatAgent?.id : undefined;
      // For remote-provider agents, temporarily override providerRef so the generation
      // stream routes to the provider SSE endpoint instead of the local WS path.
      const effectiveAgent = currentConversationId ? conversationAgent : effectiveNewChatAgent;
      const isRemoteAgent = effectiveAgent && effectiveAgent.provider_id !== 'local';
      const saved = isRemoteAgent ? applyRemoteAgentProvider(effectiveAgent) : null;
      await startGeneration(
        {
          prompt: trimmed,
          conversationId: currentConversationId,
          agentId,
          imageData: hasImages ? imageData : undefined,
        },
        assistantMessageId,
      );
      if (saved) {
        providerRef.current = saved.provider;
        providerParamsRef.current = saved.params;
      }
    },
    [
      isLoading,
      currentConversationId,
      stagedAgent,
      conversationAgent,
      startGeneration,
      resetAutoContinue,
      abortControllerRef,
      queueForRemoteProvider,
      applyRemoteAgentProvider,
    ],
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

  // ─── Reload messages after compaction ─────────────────────────────────

  useEffect(() => {
    const onCompacted = () => {
      if (currentConversationId) loadConversation(currentConversationId);
    };
    window.addEventListener('conversation-compacted', onCompacted);
    return () => window.removeEventListener('conversation-compacted', onCompacted);
  }, [currentConversationId, loadConversation]);

  // Refresh message titles when background title gen completes (fired 4s + 10s after done).
  useEffect(() => {
    const onTitleUpdated = async () => {
      if (!currentConversationId) return;
      try {
        const data = await getConversation(currentConversationId);
        if (!data.messages) return;
        const apiMsgs = data.messages as Array<Record<string, unknown>>;
        setMessages((prev) =>
          prev.map((msg) => {
            if (!msg.sequenceOrder) return msg;
            const api = apiMsgs.find((m) => m.sequence_order === msg.sequenceOrder);
            const newTitle = api?.title ? String(api.title) : undefined;
            if (newTitle && newTitle !== msg.title) return { ...msg, title: newTitle };
            return msg;
          }),
        );
      } catch {
        // ignore
      }
    };
    window.addEventListener('conversation-title-updated', onTitleUpdated);
    return () => window.removeEventListener('conversation-title-updated', onTitleUpdated);
  }, [currentConversationId, setMessages]);

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

      const targetMsg = messagesRef.current[messageIndex];
      const fromSequence =
        targetMsg?.sequenceOrder ??
        (() => {
          const systemMsgsBefore = messagesRef.current
            .slice(0, messageIndex)
            .filter((m) => m.role === 'system').length;
          return messageIndex - systemMsgsBefore + 1;
        })();

      if (currentConversationId) {
        try {
          await truncateConversation(currentConversationId, fromSequence);
          providerSessionRef.current = null;
        } catch (error_) {
          const msg = error_ instanceof Error ? error_.message : 'Failed to truncate conversation';
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

      const targetMsg = msgs[messageIndex];
      const fromSequence =
        targetMsg?.sequenceOrder ??
        (() => {
          const systemMsgsBefore = msgs
            .slice(0, messageIndex)
            .filter((m) => m.role === 'system').length;
          return messageIndex - systemMsgsBefore + 1;
        })();

      if (currentConversationId) {
        try {
          await truncateConversation(currentConversationId, fromSequence);
          providerSessionRef.current = null;
        } catch (error_) {
          const msg = error_ instanceof Error ? error_.message : 'Failed to truncate conversation';
          toast.error(msg, { duration: 5000 });
          return;
        }
      }

      setMessages((prev) => prev.slice(0, messageIndex));
      abortControllerRef.current?.abort();
      abortControllerRef.current = new AbortController();
      setIsLoading(true);
      setError(null);

      const assistantMessageId = generateId();

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

      const assistantMessageId = generateId();

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

  // ─── Polling reconnect (after page refresh / conversation switch) ──────
  // Crash recovery is handled entirely by the backend (worker_bridge.rs CrashRecoveryCtx).
  // The polling reconnect below picks up backend-initiated auto-continue generation.

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
              const aid = generateId();
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
                ...(msg.sequence_order != null
                  ? { sequenceOrder: msg.sequence_order as number }
                  : {}),
                ...(msg.title ? { title: String(msg.title) } : {}),
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
                  if (
                    msg.content.startsWith('[Conversation summary') ||
                    msg.content.startsWith('[Compacted history')
                  ) {
                    return true;
                  }
                  if (msg.content.startsWith('[System:')) {
                    return true;
                  }
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
                  const aid = generateId();
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
    queuedMessage,
    cancelQueuedMessage,
    currentConversationWorkerId,
    setCurrentConversationWorkerId,
    pendingApproval,
    clearPendingApproval,
  };
}
