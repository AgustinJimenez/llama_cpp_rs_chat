import { useEffect, useRef } from 'react';
import { toast } from 'react-hot-toast';
import { autoParseToolCalls } from '../utils/toolParser';
import { parseConversationFile } from '../utils/conversationParser';
import { areToolCallsComplete } from './useToolExecution';
import { logToastError, logToastWarning } from '../utils/toastLogger';
import type { Message, ToolCall } from '../types';

interface UseConversationWatcherOptions {
  currentConversationId: string | null;
  isStreamingRef: React.MutableRefObject<boolean>;
  currentMessagesRef: React.MutableRefObject<Message[]>;
  setMessages: React.Dispatch<React.SetStateAction<Message[]>>;
  setTokensUsed: React.Dispatch<React.SetStateAction<number | undefined>>;
  setMaxTokens: React.Dispatch<React.SetStateAction<number | undefined>>;
  setIsLoading: React.Dispatch<React.SetStateAction<boolean>>;
  processToolCalls: (toolCalls: ToolCall[], lastMessage: Message) => Promise<void>;
  isMessageProcessed: (messageId: string) => boolean;
  shouldStopExecution: (toolCalls: ToolCall[]) => boolean;
}

const WS_RECONNECT_BASE_MS = 500;
const WS_RECONNECT_MAX_MS = 5000;
const WS_STUCK_STREAMING_POLL_MS = 15_000;

function getNextDelay(attempt: number): number {
  const exp = WS_RECONNECT_BASE_MS * Math.pow(2, attempt);
  return Math.min(exp, WS_RECONNECT_MAX_MS);
}

/**
 * Hook to watch conversation updates via WebSocket.
 * Handles real-time updates, tool call detection, and context warnings.
 */
// eslint-disable-next-line max-lines-per-function
export function useConversationWatcher({
  currentConversationId,
  isStreamingRef,
  currentMessagesRef,
  setMessages,
  setTokensUsed,
  setMaxTokens,
  setIsLoading,
  processToolCalls,
  isMessageProcessed,
  shouldStopExecution,
}: UseConversationWatcherOptions) {
  const lastWsUpdateAtRef = useRef<number>(Date.now());
  const lastPolledAssistantContentRef = useRef<string>('');
  const stablePollsRef = useRef(0);

  // Fallback: re-fetch conversation if WS is disconnected but a conversation is active
  useEffect(() => {
    const shouldPollConversation = () => {
      if (!currentConversationId) {
        return false;
      }

      if (!isStreamingRef.current) {
        return false;
      }

      return Date.now() - lastWsUpdateAtRef.current >= WS_STUCK_STREAMING_POLL_MS;
    };

    const reconcileUiStateFromMessages = async (parsedMessages: Message[]) => {
      const lastMessage = parsedMessages[parsedMessages.length - 1];
      if (!lastMessage || lastMessage.role !== 'assistant') {
        return;
      }

      const toolCalls = autoParseToolCalls(lastMessage.content);
      if (toolCalls.length > 0) {
        const hasComplete = areToolCallsComplete(lastMessage.content, toolCalls);
        if (hasComplete && shouldStopExecution(toolCalls)) {
          isStreamingRef.current = false;
          setIsLoading(false);
          return;
        }

        if (hasComplete && !isMessageProcessed(lastMessage.id) && !shouldStopExecution(toolCalls)) {
          await processToolCalls(toolCalls, lastMessage);
        }

        setIsLoading(!hasComplete);
        return;
      }

      if (lastMessage.content.trim().length > 0) {
        isStreamingRef.current = false;
        setIsLoading(false);
      }
    };

    const fetchConversation = async () => {
      if (!shouldPollConversation()) {
        return;
      }
      try {
        const response = await fetch(`/api/conversation/${currentConversationId}`);
        if (!response.ok) {
          logToastWarning(
            'useConversationWatcher.poll',
            `Failed to refetch conversation (${response.status})`
          );
          return;
        }
        const data = await response.json();
        if (data.content) {
          const parsedMessages = parseConversationFile(data.content);
          let nextMessages = parsedMessages;
          const localMessages = currentMessagesRef.current;
          if (isStreamingRef.current && localMessages.length > 0) {
            const localLast = localMessages[localMessages.length - 1];
            const incomingLast = parsedMessages[parsedMessages.length - 1];
            if (localLast?.role === 'assistant') {
              const incomingAssistant = incomingLast?.role === 'assistant' ? incomingLast : null;
              const incomingLength = incomingAssistant?.content.length ?? 0;
              if (incomingLength < localLast.content.length || parsedMessages.length < localMessages.length) {
                nextMessages = localMessages;
              }
            } else if (parsedMessages.length < localMessages.length) {
              nextMessages = localMessages;
            }
          }
          setMessages(nextMessages);

          if (isStreamingRef.current) {
            const lastMessage = nextMessages[nextMessages.length - 1];
            const assistantContent = lastMessage?.role === 'assistant' ? lastMessage.content : '';
            if (!assistantContent) {
              return;
            }

            if (assistantContent === lastPolledAssistantContentRef.current) {
              stablePollsRef.current += 1;
            } else {
              stablePollsRef.current = 0;
              lastPolledAssistantContentRef.current = assistantContent;
            }

            if (stablePollsRef.current < 1) {
              return;
            }
          } else {
            stablePollsRef.current = 0;
            lastPolledAssistantContentRef.current = '';
          }

          await reconcileUiStateFromMessages(nextMessages);
        } else if (data.messages) {
          const parsedMessages: Message[] = data.messages;
          let nextMessages = parsedMessages;
          const localMessages = currentMessagesRef.current;
          if (isStreamingRef.current && localMessages.length > 0) {
            const localLast = localMessages[localMessages.length - 1];
            const incomingLast = parsedMessages[parsedMessages.length - 1];
            if (localLast?.role === 'assistant') {
              const incomingAssistant = incomingLast?.role === 'assistant' ? incomingLast : null;
              const incomingLength = incomingAssistant?.content.length ?? 0;
              if (incomingLength < localLast.content.length || parsedMessages.length < localMessages.length) {
                nextMessages = localMessages;
              }
            } else if (parsedMessages.length < localMessages.length) {
              nextMessages = localMessages;
            }
          }
          setMessages(nextMessages);
          await reconcileUiStateFromMessages(nextMessages);
        }
      } catch (err) {
        logToastWarning('useConversationWatcher.poll', 'Conversation poll failed', err);
      }
    };

    const interval = setInterval(fetchConversation, 5000);
    return () => clearInterval(interval);
  }, [
    currentConversationId,
    isStreamingRef,
    currentMessagesRef,
    processToolCalls,
    isMessageProcessed,
    setIsLoading,
    setMessages,
    shouldStopExecution,
  ]);

  useEffect(() => {
    if (!currentConversationId) {
      return;
    }

    let attempt = 0;
    let shouldReconnect = true;

    // Determine WebSocket URL based on current protocol
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${window.location.host}/ws/conversation/watch/${currentConversationId}`;

    let ws: WebSocket | null = null;

    const connect = () => {
      ws = new WebSocket(wsUrl);

      ws.onopen = () => {
        attempt = 0;
        lastWsUpdateAtRef.current = Date.now();
      };

      ws.onmessage = (event) => {
        try {
          lastWsUpdateAtRef.current = Date.now();
          const message = JSON.parse(event.data);

          if (message.type === 'update') {
            handleUpdate(message);
          }
        } catch (e) {
          console.error('[ConversationWatcher] Failed to parse update:', e);
        }
      };

      ws.onerror = (error) => {
        console.error('[ConversationWatcher] WebSocket ERROR:', error);
      };

      ws.onclose = (event) => {
        console.log('[ConversationWatcher] WebSocket closed:', event.code, event.reason);
        if (shouldReconnect) {
          const delay = getNextDelay(attempt);
          attempt += 1;
          setTimeout(connect, delay);
        }
      };
    };

    connect();

    return () => {
      shouldReconnect = false;
      if (ws) {
        ws.close();
      }
    };

    function handleUpdate(message: { type?: string; content?: string; tokens_used?: number; max_tokens?: number }) {
      // Skip updates during active streaming
      if (isStreamingRef.current) {
        console.log('[ConversationWatcher] Skipping update - streaming is active');
        return;
      }

      const content = message.content;
      if (!content) {
        return;
      }

      // Check for context size warnings
      handleContextWarnings(content);

      // Parse and update messages
      const parsedMessages = parseConversationFile(content);
      setMessages(parsedMessages);
      console.log('[ConversationWatcher] Updated messages, count:', parsedMessages.length);

      // Check for generation errors
      if (content.includes('⚠️ Generation Error:')) {
        console.error('[ConversationWatcher] Generation error detected');
        const display = 'Model generation failed. Try simplifying your request or reducing context size.';
        logToastError('useConversationWatcher.handleUpdate', display);
        toast.error(display, { duration: 7000 });
        setIsLoading(false);
        return;
      }

      // Handle tool calls in assistant messages
      handleToolCalls(parsedMessages);

      // Update token counts
      if (message.tokens_used !== undefined && message.tokens_used !== null) {
        setTokensUsed(message.tokens_used);
      }
      if (message.max_tokens !== undefined && message.max_tokens !== null) {
        setMaxTokens(message.max_tokens);
      }
    }

    function handleContextWarnings(content: string) {
      if (content.includes('⚠️ Context Size Reduced') && content.includes('Auto-reduced to:')) {
        const match = content.match(/Auto-reduced to: (\d+) tokens/);
        if (match) {
          const reducedSize = parseInt(match[1]);
          if (reducedSize < 4096) {
            const display = `⚠️ Context size critically low (${reducedSize} tokens)! Model may not work properly.`;
            logToastWarning('useConversationWatcher.context', display);
            toast.error(display, { duration: 10000 });
          } else {
            const display = `⚠️ Context size reduced to ${reducedSize} tokens due to VRAM limits.`;
            logToastWarning('useConversationWatcher.context', display);
            toast(display, {
              duration: 5000,
              icon: '⚠️',
            });
          }
        }
      }
    }

    function handleToolCalls(parsedMessages: Message[]) {
      const lastMessage = parsedMessages[parsedMessages.length - 1];
      if (!lastMessage || lastMessage.role !== 'assistant' || !lastMessage.content.length) {
        return;
      }

      const toolCalls = autoParseToolCalls(lastMessage.content);
      console.log('[ConversationWatcher] Detected', toolCalls.length, 'tool calls');

      if (toolCalls.length > 0) {
        const hasComplete = areToolCallsComplete(lastMessage.content, toolCalls);
        console.log('[ConversationWatcher] Tool calls complete:', hasComplete);

        if (hasComplete && !isMessageProcessed(lastMessage.id)) {
          // Double-check we shouldn't stop before processing
          if (!shouldStopExecution(toolCalls)) {
            processToolCalls(toolCalls, lastMessage);
          }
        }
      } else {
        // No tool calls - turn off loading spinner
        setIsLoading(false);
      }
    }
  }, [
    currentConversationId,
    isStreamingRef,
    setMessages,
    setTokensUsed,
    setMaxTokens,
    setIsLoading,
    processToolCalls,
    isMessageProcessed,
    shouldStopExecution,
  ]);

  return {};
}
