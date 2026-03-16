import { useState, useCallback, useEffect, useRef } from 'react';
import { flushSync } from 'react-dom';
import { toast } from 'react-hot-toast';
import { createChatTransport, type ChatTransport, type TimingInfo } from '../utils/chatTransport';
import { useConversationUrl } from './useConversationUrl';
import { useConversationWatcher } from './useConversationWatcher';
import { logToastError } from '../utils/toastLogger';
import { notifyIfUnfocused } from '../utils/tauri';
import { parseConversationFile } from '../utils/conversationParser';
import { getConversation, truncateConversation, cancelGeneration } from '../utils/tauriCommands';
import { useConnection } from '../contexts/ConnectionContext';
import type { Message, ChatRequest } from '../types';

function isAbortErrorMessage(message: string): boolean {
  return /aborted/i.test(message);
}

function removeEmptyAssistantMessage(
  setMessages: React.Dispatch<React.SetStateAction<Message[]>>,
  assistantMessageId: string
) {
  setMessages(prev =>
    prev.filter(m => !(m.id === assistantMessageId && m.role === 'assistant' && m.content.length === 0))
  );
}

function handleStreamError(
  err: unknown,
  streamSeq: number,
  streamSeqRef: React.MutableRefObject<number>,
  isStreamingRef: React.MutableRefObject<boolean>,
  setError: (e: string | null) => void,
  setIsLoading: (v: boolean) => void,
  setMessages: React.Dispatch<React.SetStateAction<Message[]>>,
  assistantMessageId: string,
) {
  if (streamSeqRef.current !== streamSeq) return;
  isStreamingRef.current = false;
  const errorMessage = err instanceof Error ? err.message : 'An unknown error occurred';
  const isAbort = isAbortErrorMessage(errorMessage);
  setError(errorMessage);
  if (!isAbort) {
    const display = `Chat error: ${errorMessage}`;
    toast.error(display, { duration: 5000 });
    logToastError('useChat.sendMessage', display, err);
  }
  setIsLoading(false);
  if (isAbort) {
    removeEmptyAssistantMessage(setMessages, assistantMessageId);
  }
}

/**
 * Main chat hook - orchestrates messaging and streaming.
 *
 * Tool execution is handled entirely by the backend inline during generation.
 * The frontend only handles token display and conversation management.
 *
 * Delegates to specialized hooks:
 * - useConversationUrl: URL param persistence
 * - useConversationWatcher: WebSocket updates
 */
// eslint-disable-next-line max-lines-per-function
export function useChat() {
  const { connected } = useConnection();
  const connectedRef = useRef(connected);
  connectedRef.current = connected;

  // Core state
  const [messages, setMessages] = useState<Message[]>([]);
  const [currentConversationId, setCurrentConversationId] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [tokensUsed, setTokensUsed] = useState<number | undefined>(undefined);
  const [maxTokens, setMaxTokens] = useState<number | undefined>(undefined);
  const [lastTimings, setLastTimings] = useState<TimingInfo | undefined>(undefined);
  const messagesRef = useRef<Message[]>([]);

  // Refs for streaming state
  const abortControllerRef = useRef<AbortController | null>(null);
  const isStreamingRef = useRef(false);
  const streamSeqRef = useRef(0);
  const transportRef = useRef<ChatTransport>(createChatTransport());

  // Auto-continue: when generation hits max_tokens (not EOS), re-send "Continue"
  const MAX_AUTO_CONTINUES = 3;
  const autoContinueCountRef = useRef(0);

  // Clear all messages and reset state
  const clearMessages = useCallback(() => {
    setMessages([]);
    setCurrentConversationId(null);
    setError(null);
    setTokensUsed(undefined);
    setMaxTokens(undefined);
    setLastTimings(undefined);
  }, []);

  useEffect(() => {
    messagesRef.current = messages;
  }, [messages]);

  // Derive lastTimings from the last assistant message so stats persist
  // across conversation loads and page refreshes.
  useEffect(() => {
    for (let i = messages.length - 1; i >= 0; i--) {
      if (messages[i].role === 'assistant' && messages[i].timings) {
        setLastTimings(messages[i].timings);
        return;
      }
    }
  }, [messages]);

  // Load a conversation from the backend
  const loadConversation = useCallback(async (filename: string) => {
    if (!connectedRef.current) {
      toast.error('Server is unreachable — please wait for reconnection', { duration: 3000, id: 'server-down' });
      return;
    }
    setIsLoading(true);
    setError(null);
    setTokensUsed(undefined);
    setMaxTokens(undefined);

    try {
      const data = await getConversation(filename);
      if (data.content) {
        setMessages(parseConversationFile(data.content));
        setCurrentConversationId(filename);
      } else if (data.messages) {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const mapped = (data.messages as any[]).map(msg => {
          const m: Message = {
            id: msg.id,
            role: msg.role,
            content: msg.content,
            timestamp: msg.timestamp,
          };
          // Map flat backend timing fields into nested timings object
          if (msg.gen_tok_per_sec != null) {
            m.timings = {
              promptTokPerSec: msg.prompt_tok_per_sec,
              genTokPerSec: msg.gen_tok_per_sec,
              genEvalMs: msg.gen_eval_ms,
              genTokens: msg.gen_tokens,
              promptEvalMs: msg.prompt_eval_ms,
              promptTokens: msg.prompt_tokens,
            };
          }
          return m;
        });
        // Keep the first system message (the system prompt) for the UI widget,
        // filter out subsequent system messages (tool results, etc.)
        let systemPromptSeen = false;
        const filtered = mapped.filter(msg => {
          if (msg.role === 'system') {
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

  const hasLoggedFirstTokenRef = useRef(false);

  const runStream = useCallback(async (params: {
    request: ChatRequest;
    assistantMessageId: string;
    streamSeq: number;
  }) => {
    const { request, assistantMessageId, streamSeq } = params;
    hasLoggedFirstTokenRef.current = false;

    await transportRef.current.streamMessage(
      request,
      {
        onToken: (token, tokenCount, maxTokenCount) => {
          if (streamSeqRef.current !== streamSeq) return;
          if (!hasLoggedFirstTokenRef.current) {
            hasLoggedFirstTokenRef.current = true;
            console.log('[useChat] First token received');
          }
          setMessages(prev => {
            const lastMsg = prev[prev.length - 1];
            if (lastMsg && lastMsg.id === assistantMessageId) {
              return [
                ...prev.slice(0, -1),
                { ...lastMsg, content: lastMsg.content + token },
              ];
            }
            return prev.map(msg => (msg.id === assistantMessageId ? { ...msg, content: msg.content + token } : msg));
          });
          if (tokenCount !== undefined) setTokensUsed(tokenCount);
          if (maxTokenCount !== undefined) setMaxTokens(maxTokenCount);
        },
        onComplete: (_messageId, conversationId, tokenCount, maxTokenCount, timings) => {
          if (streamSeqRef.current !== streamSeq) return;
          isStreamingRef.current = false;
          console.log('[useChat] Streaming complete', timings ? `gen=${timings.genTokPerSec?.toFixed(1)} tok/s finish=${timings.finishReason ?? '?'}` : '');

          if (!currentConversationId) {
            setCurrentConversationId(conversationId);
            // New conversation — fetch system prompt from backend to show widget
            getConversation(conversationId)
              .then(data => {
                const firstMsg = data.messages?.[0];
                if (firstMsg?.role === 'system' && firstMsg.content) {
                  setMessages(prev => {
                    // Guard: don't prepend if a system message already exists
                    // (the conversation watcher may have already set it)
                    if (prev.some(m => m.role === 'system')) return prev;
                    return [{
                      id: `sys_${conversationId}`,
                      role: 'system' as const,
                      content: firstMsg.content,
                      timestamp: Date.now(),
                      isSystemPrompt: true,
                    }, ...prev];
                  });
                }
              })
              .catch(() => {});
          }
          // Trigger delayed sidebar refresh to pick up auto-generated/updated title
          setTimeout(() => {
            window.dispatchEvent(new CustomEvent('conversation-title-updated'));
          }, 4000);
          if (tokenCount !== undefined) setTokensUsed(tokenCount);
          if (maxTokenCount !== undefined) setMaxTokens(maxTokenCount);
          if (timings) {
            setLastTimings(timings);
            setMessages(prev => prev.map(msg =>
              msg.id === assistantMessageId ? { ...msg, timings, timestamp: Date.now() } : msg
            ));
          }

          // Auto-continue: if generation was cut off by max_tokens, re-send "Continue"
          const finishReason = timings?.finishReason;
          if (finishReason === 'length' && autoContinueCountRef.current < MAX_AUTO_CONTINUES) {
            autoContinueCountRef.current += 1;
            const continueNum = autoContinueCountRef.current;
            console.log(`[useChat] Auto-continue ${continueNum}/${MAX_AUTO_CONTINUES} (finish_reason=length)`);

            // Append a visual indicator to the current assistant message
            setMessages(prev => prev.map(msg =>
              msg.id === assistantMessageId
                ? { ...msg, content: msg.content + `\n\n*[Continuing... (${continueNum}/${MAX_AUTO_CONTINUES})]*\n\n` }
                : msg
            ));

            // Brief delay then re-send
            setTimeout(() => {
              const convId = conversationId || currentConversationId;
              // Re-use the same assistant message — create a new stream sequence
              const newStreamSeq = (streamSeqRef.current += 1);
              isStreamingRef.current = true;
              abortControllerRef.current = new AbortController();

              runStream({
                request: { message: 'Continue', conversation_id: convId || undefined },
                assistantMessageId,
                streamSeq: newStreamSeq,
              }).catch(err => {
                handleStreamError(err, newStreamSeq, streamSeqRef, isStreamingRef, setError, setIsLoading, setMessages, assistantMessageId);
              });
            }, 150);
            return; // Don't set isLoading=false — we're continuing
          }

          // Normal completion — reset auto-continue counter
          autoContinueCountRef.current = 0;
          notifyIfUnfocused('Generation complete', 'Your AI response is ready.');
          setIsLoading(false);
        },
        onError: (errorMsg) => {
          if (streamSeqRef.current !== streamSeq) return;
          isStreamingRef.current = false;
          console.log('[useChat] Streaming error');
          setError(errorMsg);

          const isAbort = isAbortErrorMessage(errorMsg);
          if (!isAbort) {
            const display = `Chat error: ${errorMsg}`;
            toast.error(display, { duration: 5000 });
            logToastError('useChat.streamMessage', display);
          }

          setIsLoading(false);
          if (isAbort) {
            removeEmptyAssistantMessage(setMessages, assistantMessageId);
          }
        },
      },
      abortControllerRef.current?.signal
    );
  }, [currentConversationId, setMessages]);

  // Send message (with optional image attachments)
  const sendMessage = useCallback(async (content: string, imageData?: string[], bypassLoadingCheck = false) => {
    if (!connectedRef.current) {
      toast.error('Server is unreachable — please wait for reconnection', { duration: 3000, id: 'server-down' });
      return;
    }
    const hasImages = imageData && imageData.length > 0;
    const trimmed = content.trim();
    if (!trimmed && !hasImages) return;

    // If a generation is in progress, cancel it first before sending new message
    if (isLoading && !bypassLoadingCheck) {
      abortControllerRef.current?.abort();
      await cancelGeneration().catch(() => {});
      isStreamingRef.current = false;
      setIsLoading(false);
      // Wait for backend to fully cancel (kill subprocess + release generation slot)
      await new Promise(resolve => setTimeout(resolve, 1500));
    }

    // Reset auto-continue counter on new user message
    autoContinueCountRef.current = 0;

    // Abort any previous request
    abortControllerRef.current?.abort();
    abortControllerRef.current = new AbortController();

    setMessages(prev => [...prev, {
      id: crypto.randomUUID(),
      role: 'user' as const,
      content: trimmed,
      timestamp: Date.now(),
      image_data: hasImages ? imageData : undefined,
    }]);
    setIsLoading(true);
    setError(null);

    const assistantMessageId = crypto.randomUUID();
    const streamSeq = (streamSeqRef.current += 1);

    flushSync(() => {
      setMessages(prev => [...prev, {
        id: assistantMessageId,
        role: 'assistant' as const,
        content: '',
        timestamp: 0, // set when response completes
      }]);
    });

    try {
      isStreamingRef.current = true;
      console.log('[useChat] Streaming started');
      await runStream({
        request: { message: trimmed, conversation_id: currentConversationId || undefined, image_data: hasImages ? imageData : undefined },
        assistantMessageId,
        streamSeq,
      });
    } catch (err) {
      handleStreamError(err, streamSeq, streamSeqRef, isStreamingRef, setError, setIsLoading, setMessages, assistantMessageId);
    }
  }, [isLoading, currentConversationId, runStream]);

  // URL persistence hook
  useConversationUrl({
    currentConversationId,
    loadConversation,
  });

  // Conversation watcher hook (WebSocket updates)
  useConversationWatcher({
    currentConversationId,
    isStreamingRef,
    currentMessagesRef: messagesRef,
    setMessages,
    setTokensUsed,
    setMaxTokens,
    setIsLoading,
  });

  // Edit a user message: truncate from that point and re-send
  const editMessage = useCallback(async (messageIndex: number, newContent: string) => {
    if (!connectedRef.current) {
      toast.error('Server is unreachable — please wait for reconnection', { duration: 3000, id: 'server-down' });
      return;
    }
    if (isLoading) return;

    // DB sequence: system prompt at 0, user/assistant messages start at 1.
    // The messages array may or may not include system messages (depends on
    // whether the conversation was loaded from backend or built locally).
    // Count system messages before the target to compute the correct DB offset.
    const systemMsgsBefore = messagesRef.current.slice(0, messageIndex)
      .filter(m => m.role === 'system').length;
    const fromSequence = messageIndex - systemMsgsBefore + 1;

    // Truncate backend DB from the edited message onward
    if (currentConversationId) {
      try {
        await truncateConversation(currentConversationId, fromSequence);
      } catch (err) {
        const msg = err instanceof Error ? err.message : 'Failed to truncate conversation';
        toast.error(msg, { duration: 5000 });
        return;
      }
    }

    // Truncate local messages array (remove from editedIndex onward)
    setMessages(prev => prev.slice(0, messageIndex));

    // Re-send the edited content
    await sendMessage(newContent, undefined, true);
  }, [isLoading, currentConversationId, sendMessage]);

  // Stop the current generation
  const stopGeneration = useCallback(() => {
    // Always tell the backend to cancel, even if WS is already closed
    fetch('/api/chat/cancel', { method: 'POST' }).catch(() => {});
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
      abortControllerRef.current = null;
    }
    isStreamingRef.current = false;
    setIsLoading(false);
  }, []);

  return {
    messages,
    isLoading,
    error,
    sendMessage,
    editMessage,
    stopGeneration,
    clearMessages,
    loadConversation,
    currentConversationId,
    tokensUsed,
    maxTokens,
    lastTimings,
  };
}
