import { useState, useCallback, useEffect, useRef } from 'react';
import { flushSync } from 'react-dom';
import { toast } from 'react-hot-toast';
import { createChatTransport, type ChatTransport, type TimingInfo } from '../utils/chatTransport';
import { autoParseToolCalls } from '../utils/toolParser';
import { useConversationUrl } from './useConversationUrl';
import { useToolExecution } from './useToolExecution';
import { useConversationWatcher } from './useConversationWatcher';
import { logToastError } from '../utils/toastLogger';
import { notifyIfUnfocused } from '../utils/tauri';
import { parseConversationFile } from '../utils/conversationParser';
import { getConversation } from '../utils/tauriCommands';
import type { Message, ChatRequest, ToolCall } from '../types';

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

function canSendMessage(opts: {
  content: string;
  bypassLoadingCheck: boolean;
  isLoading: boolean;
}) {
  const { content, bypassLoadingCheck, isLoading } = opts;
  if (!bypassLoadingCheck && (isLoading || !content.trim())) {
    return false;
  }
  if (!content.trim()) {
    return false;
  }
  return true;
}

/**
 * Main chat hook - orchestrates messaging, streaming, and tool execution.
 *
 * Delegates to specialized hooks:
 * - useConversationUrl: URL param persistence
 * - useToolExecution: Tool call handling and loop detection
 * - useConversationWatcher: WebSocket updates
 */
// eslint-disable-next-line max-lines-per-function
export function useChat() {
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

  // Load a conversation from the backend
  const loadConversation = useCallback(async (filename: string) => {
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
        const filtered = (data.messages as Message[]).filter(
          msg => msg.role !== 'system' && !msg.content.startsWith('[TOOL_RESULTS]')
        );
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

  const toolExecutionRef = useRef<ReturnType<typeof useToolExecution> | null>(null);
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
          console.log('[useChat] Streaming complete', timings ? `gen=${timings.genTokPerSec?.toFixed(1)} tok/s` : '');
          notifyIfUnfocused('Generation complete', 'Your AI response is ready.');

          if (!currentConversationId) {
            setCurrentConversationId(conversationId);
          }
          if (tokenCount !== undefined) setTokensUsed(tokenCount);
          if (maxTokenCount !== undefined) setMaxTokens(maxTokenCount);
          if (timings) setLastTimings(timings);

          setIsLoading(false);
          // Read latest messages via updater (synchronous) but keep side effects outside.
          // Use a wrapper object so TypeScript tracks the mutation from inside the callback.
          const pending: { action: { calls: ToolCall[]; msg: Message } | null } = { action: null };
          setMessages(prev => {
            const lastMsg = prev[prev.length - 1];
            if (lastMsg && lastMsg.role === 'assistant' && lastMsg.content) {
              const toolCalls = autoParseToolCalls(lastMsg.content);
              if (toolCalls.length > 0) {
                pending.action = { calls: toolCalls, msg: lastMsg };
              }
            }
            return prev;
          });
          if (pending.action) {
            toolExecutionRef.current?.processToolCalls(pending.action.calls, pending.action.msg);
          }
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

  // Send message - defined before tool execution hook since it's needed there
  const sendMessage = useCallback(async (content: string, bypassLoadingCheck = false) => {
    if (!canSendMessage({
      content,
      bypassLoadingCheck,
      isLoading,
    })) {
      return;
    }

    // Abort any previous request
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
    }
    abortControllerRef.current = new AbortController();

    // Check if this is a tool result (not a new user message)
    const isToolResult = content.startsWith('[TOOL_RESULTS]');
    if (!isToolResult) {
      toolExecutionRef.current?.resetToolState();
    } else {
      console.log('[useChat] Continuing with tool results');
    }

    // Create user message
    const userMessage: Message = {
      id: crypto.randomUUID(),
      role: 'user',
      content: content.trim(),
      timestamp: Date.now(),
    };

    // Add user message to UI (skip tool results - they're only for the model)
    if (!isToolResult) {
      setMessages(prev => [...prev, userMessage]);
    }
    setIsLoading(true);
    setError(null);

    // Create placeholder assistant message for streaming
    const assistantMessageId = crypto.randomUUID();
    const assistantMessage: Message = {
      id: assistantMessageId,
      role: 'assistant',
      content: '',
      timestamp: Date.now(),
    };

    const streamSeq = (streamSeqRef.current += 1);

    // Use flushSync to ensure state is committed before streaming starts
    flushSync(() => {
      setMessages(prev => [...prev, assistantMessage]);
    });

    try {
      const request: ChatRequest = {
        message: content.trim(),
        conversation_id: currentConversationId || undefined,
      };

      // Mark streaming as active
      isStreamingRef.current = true;
      console.log('[useChat] Streaming started');

      await runStream({ request, assistantMessageId, streamSeq });
    } catch (err) {
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
  }, [isLoading, currentConversationId, runStream]);

  // Tool execution hook
  const toolExecution = useToolExecution({
    maxTokens,
    sendMessage,
    setIsLoading,
  });
  toolExecutionRef.current = toolExecution;

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
    processToolCalls: toolExecution.processToolCalls,
    isMessageProcessed: toolExecution.isMessageProcessed,
    shouldStopExecution: toolExecution.shouldStopExecution,
  });

  // Stop the current generation
  const stopGeneration = useCallback(() => {
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
    stopGeneration,
    clearMessages,
    loadConversation,
    currentConversationId,
    tokensUsed,
    maxTokens,
    lastTimings,
  };
}
