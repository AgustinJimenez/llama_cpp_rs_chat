/**
 * Unified generation stream hook.
 *
 * Extracts the shared streaming lifecycle from useChat:
 * - Stream sequence tracking (prevents stale updates)
 * - Abort controller management
 * - Auto-continue logic (context full, tool continuation, loop recovery)
 * - Token → message state updates
 * - Timing/stats updates
 * - Error handling
 * - System prompt fetching for new conversations
 *
 * Both local and remote providers go through this single path.
 */
import { useCallback, useRef } from 'react';
import { toast } from 'react-hot-toast';

import type { Message } from '../types';
import type { ChatTransport, TimingInfo } from '../utils/chatTransport';
import {
  createGenerationStream,
  type GenerationRequest,
} from '../utils/generationStream';
import { notifyIfUnfocused } from '../utils/tauri';
import { getConversation } from '../utils/tauriCommands';
import { logToastError } from '../utils/toastLogger';

const TITLE_REFRESH_DELAY_MS = 4000;
const CONTINUE_TASK_PREVIEW_LENGTH = 200;
const CONTINUE_DELAY_MS = 150;
const MAX_AUTO_CONTINUES = 3;
const TOAST_DURATION_MS = 5000;

// Auto-continue: finish reasons that trigger automatic re-generation
const AUTO_CONTINUE_REASONS = new Set([
  'length',
  'yn_continue',
  'loop_recovery',
  'tool_continue',
  'cuda_deadlock',
  'infinite_loop',
]);

function isAbortError(msg: string): boolean {
  return /aborted/i.test(msg);
}

function friendlyError(msg: string): string {
  if (/generation already in progress/i.test(msg)) {
    return 'The model is busy. Click Stop first, then try again.';
  }
  if (/generation still cancelling/i.test(msg)) {
    return 'Still cancelling the previous request. Please wait a moment.';
  }
  if (/worker stdin closed/i.test(msg)) {
    return 'Connection to the model worker was lost. Try reloading the model.';
  }
  if (/context.*full|context.*exceeded/i.test(msg)) {
    return 'The conversation is too long. Start a new conversation or reduce context size.';
  }
  if (/model.*not.*loaded|no model/i.test(msg)) {
    return 'No model is loaded. Please load a model first.';
  }
  if (/failed to load conversation/i.test(msg)) {
    return 'Could not load this conversation. It may have been deleted.';
  }
  return msg;
}

export interface UseGenerationStreamDeps {
  setMessages: React.Dispatch<React.SetStateAction<Message[]>>;
  setIsLoading: (v: boolean) => void;
  setError: (e: string | null) => void;
  setLastTimings: (t: TimingInfo | undefined) => void;
  setTokensUsed: (n: number | undefined) => void;
  setMaxTokens: (n: number | undefined) => void;
  setStreamStatus: (s: string | undefined) => void;
  setCurrentConversationId: (id: string | null) => void;
  currentConversationIdRef: React.MutableRefObject<string | null>;
  messagesRef: React.MutableRefObject<Message[]>;
  providerRef: React.MutableRefObject<{ provider: string; model: string }>;
  providerParamsRef: React.MutableRefObject<Record<string, unknown>>;
  providerSessionRef: React.MutableRefObject<string | null>;
  transportRef: React.MutableRefObject<ChatTransport>;
}

export function useGenerationStream(deps: UseGenerationStreamDeps) {
  const {
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
  } = deps;

  const isStreamingRef = useRef(false);
  const streamSeqRef = useRef(0);
  const abortControllerRef = useRef<AbortController | null>(null);
  const autoContinueCountRef = useRef(0);
  const compactingRef = useRef(false);
  const lastTimingsRef = useRef<Partial<TimingInfo>>({});

  // Fetch system prompt from backend and prepend to messages
  const fetchSystemPrompt = useCallback((conversationId: string) => {
    getConversation(conversationId)
      .then((data) => {
        const firstMsg = data.messages?.[0];
        if (firstMsg?.role === 'system' && firstMsg.content) {
          setMessages((prev) => {
            if (prev.some((m) => m.role === 'system')) return prev;
            return [
              {
                id: `sys_${conversationId}`,
                role: 'system' as const,
                content: firstMsg.content,
                timestamp: Date.now(),
                isSystemPrompt: true,
              },
              ...prev,
            ];
          });
        }
      })
      .catch(() => {});
  }, [setMessages]);

  // Core: start a generation stream (works for both local and remote)
  const startGeneration = useCallback(
    async (request: GenerationRequest, assistantMessageId: string) => {
      const streamSeq = (streamSeqRef.current += 1);
      isStreamingRef.current = true;
      setLastTimings(undefined);
      lastTimingsRef.current = {};

      const stream = createGenerationStream(providerRef.current.provider, {
        transport: transportRef.current,
        model: providerRef.current.model,
        sessionRef: providerSessionRef,
        providerParams: providerParamsRef.current,
      });

      try {
        await stream.start(
          request,
          {
            onToken: (token) => {
              if (streamSeqRef.current !== streamSeq) return;
              setMessages((prev) => {
                const lastMsg = prev[prev.length - 1];
                if (lastMsg && lastMsg.id === assistantMessageId) {
                  return [...prev.slice(0, -1), { ...lastMsg, content: lastMsg.content + token }];
                }
                return prev.map((msg) =>
                  msg.id === assistantMessageId ? { ...msg, content: msg.content + token } : msg,
                );
              });
            },

            onTimingsUpdate: (timings) => {
              if (streamSeqRef.current !== streamSeq) return;
              // Merge with current timings (can't use updater — setLastTimings takes value only)
              setLastTimings({ ...lastTimingsRef.current, ...timings } as TimingInfo);
              lastTimingsRef.current = { ...lastTimingsRef.current, ...timings } as TimingInfo;
            },

            onContextUpdate: (tokensUsed, maxTokens) => {
              if (streamSeqRef.current !== streamSeq) return;
              setTokensUsed(tokensUsed);
              setMaxTokens(maxTokens);
            },

            onStatus: (message) => {
              if (streamSeqRef.current !== streamSeq) return;
              setStreamStatus(message);
              // Compaction reload (local model only)
              if (compactingRef.current && (!message || !message.includes('Compacting'))) {
                compactingRef.current = false;
                const convId = currentConversationIdRef.current;
                if (convId) {
                  getConversation(convId)
                    .then((data) => {
                      if (data.messages) {
                        const mapped: Message[] = (data.messages as Array<Record<string, unknown>>).map((msg) => ({
                          id: String(msg.id),
                          role: String(msg.role) as Message['role'],
                          content: String(msg.content),
                          timestamp: Number(msg.timestamp),
                        }));
                        let systemSeen = false;
                        const filtered = mapped.filter((m) => {
                          if (m.role === 'system') {
                            if (!systemSeen) { systemSeen = true; return true; }
                            return false;
                          }
                          return !m.content.startsWith('[TOOL_RESULTS]');
                        });
                        setMessages(filtered);
                      }
                    })
                    .catch(() => {});
                }
              }
              if (message?.includes('Compacting')) {
                compactingRef.current = true;
              }
            },

            onComplete: (result) => {
              if (streamSeqRef.current !== streamSeq) return;
              const { conversationId, timings, tokensUsed, maxTokens } = result;

              console.warn(
                '[useChat] Streaming complete',
                timings
                  ? `gen=${timings.genTokPerSec?.toFixed(1)} tok/s finish=${timings.finishReason ?? '?'}`
                  : '',
              );

              // Set conversation ID if new
              if (!currentConversationIdRef.current) {
                setCurrentConversationId(conversationId);
                fetchSystemPrompt(conversationId);
              }

              // Refresh sidebar title
              setTimeout(() => {
                window.dispatchEvent(new CustomEvent('conversation-title-updated'));
              }, TITLE_REFRESH_DELAY_MS);

              if (tokensUsed !== undefined) setTokensUsed(tokensUsed);
              if (maxTokens !== undefined) setMaxTokens(maxTokens);
              if (timings) {
                setLastTimings(timings);
                setMessages((prev) =>
                  prev.map((msg) =>
                    msg.id === assistantMessageId
                      ? { ...msg, timings, timestamp: Date.now() }
                      : msg,
                  ),
                );
              }

              // Auto-continue logic
              const finishReason = timings?.finishReason;
              const shouldAutoContinue = AUTO_CONTINUE_REASONS.has(finishReason ?? '');
              const isToolContinue = finishReason === 'tool_continue';

              if (
                shouldAutoContinue &&
                (isToolContinue || autoContinueCountRef.current < MAX_AUTO_CONTINUES)
              ) {
                if (!isToolContinue) autoContinueCountRef.current += 1;
                const reasonMap: Record<string, string> = {
                  loop_recovery: 'loop recovery',
                  tool_continue: 'tool continuation',
                  yn_continue: 'task incomplete',
                  infinite_loop: 'infinite loop — forcing new approach',
                };
                const reason = (finishReason && reasonMap[finishReason]) || 'context full';
                console.warn(
                  `[useChat] Auto-continue ${autoContinueCountRef.current}/${MAX_AUTO_CONTINUES} (${reason})`,
                );

                setIsLoading(true);
                setTimeout(() => {
                  const convId = conversationId || currentConversationIdRef.current;
                  isStreamingRef.current = true;
                  setLastTimings(undefined);
                  abortControllerRef.current = new AbortController();

                  const msgs = messagesRef.current;
                  const firstUserMsg = msgs.find((m) => m.role === 'user');
                  let continueMsg: string;
                  if (finishReason === 'infinite_loop' || finishReason === 'loop_recovery') {
                    continueMsg =
                      '[SYSTEM] Infinite loop detected — you have been repeating similar actions without progress. STOP your current approach entirely. Step back, analyze what went wrong, explain it to the user, and either try a COMPLETELY DIFFERENT strategy or ask the user for guidance. Do NOT repeat any of the previous commands.';
                  } else {
                    continueMsg = firstUserMsg
                      ? `Continue working on this task: "${firstUserMsg.content.slice(0, CONTINUE_TASK_PREVIEW_LENGTH)}". Pick up where you left off.`
                      : 'Continue';
                  }

                  startGeneration(
                    {
                      prompt: continueMsg,
                      conversationId: convId,
                      autoContinue: true,
                    },
                    assistantMessageId,
                  ).catch((err) => {
                    isStreamingRef.current = false;
                    const errorMessage = err instanceof Error ? err.message : 'An unknown error occurred';
                    if (!isAbortError(errorMessage)) {
                      toast.error(friendlyError(errorMessage), { duration: TOAST_DURATION_MS });
                      logToastError('useChat.autoContinue', errorMessage, err);
                    }
                    setIsLoading(false);
                  });
                }, CONTINUE_DELAY_MS);
                return;
              }

              // Check max auto-continues reached
              const hitMax =
                shouldAutoContinue && autoContinueCountRef.current >= MAX_AUTO_CONTINUES;
              if (hitMax && timings) {
                timings.finishReason = 'max_continues';
              }

              // Normal completion
              isStreamingRef.current = false;
              setStreamStatus(undefined);
              autoContinueCountRef.current = 0;
              notifyIfUnfocused('Generation complete', 'Your AI response is ready.');
              setIsLoading(false);
            },

            onError: (errorMsg) => {
              if (streamSeqRef.current !== streamSeq) return;
              isStreamingRef.current = false;
              console.warn('[useChat] Streaming error');
              setError(errorMsg);

              if (!isAbortError(errorMsg)) {
                const display = friendlyError(errorMsg);
                toast.error(display, { duration: TOAST_DURATION_MS });
                logToastError('useChat.streamMessage', `Chat error: ${errorMsg}`);
              }

              setIsLoading(false);
              if (isAbortError(errorMsg)) {
                setMessages((prev) =>
                  prev.filter(
                    (m) => !(m.id === assistantMessageId && m.role === 'assistant' && m.content.length === 0),
                  ),
                );
              }
            },
          },
          abortControllerRef.current?.signal,
        );
      } catch (err) {
        if (streamSeqRef.current !== streamSeq) return;
        isStreamingRef.current = false;
        const errorMessage = err instanceof Error ? err.message : 'An unknown error occurred';
        if (!isAbortError(errorMessage)) {
          setError(errorMessage);
          toast.error(friendlyError(errorMessage), { duration: TOAST_DURATION_MS });
          logToastError('useChat.startGeneration', errorMessage, err);
        }
        setIsLoading(false);
        // Remove empty assistant message on abort
        if (isAbortError(errorMessage)) {
          setMessages((prev) =>
            prev.filter(
              (m) => !(m.id === assistantMessageId && m.role === 'assistant' && m.content.length === 0),
            ),
          );
        }
      }
    },
    [
      setMessages, setIsLoading, setError, setLastTimings, setTokensUsed, setMaxTokens,
      setStreamStatus, setCurrentConversationId, currentConversationIdRef, messagesRef,
      providerRef, providerParamsRef, providerSessionRef, transportRef, fetchSystemPrompt,
    ],
  );

  const abortGeneration = useCallback(() => {
    // Tell local backend to cancel (no-op for remote — channel close is enough)
    if (providerRef.current.provider === 'local') {
      fetch('/api/chat/cancel', { method: 'POST' }).catch(() => {});
    }
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
      abortControllerRef.current = null;
    }
    isStreamingRef.current = false;
    setIsLoading(false);
  }, [providerRef, setIsLoading]);

  const resetAutoContinue = useCallback(() => {
    autoContinueCountRef.current = 0;
  }, []);

  return {
    startGeneration,
    abortGeneration,
    resetAutoContinue,
    isStreamingRef,
    streamSeqRef,
    abortControllerRef,
    autoContinueCountRef,
    fetchSystemPrompt,
  };
}
