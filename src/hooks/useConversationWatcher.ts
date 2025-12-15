import { useEffect, useState } from 'react';
import { toast } from 'react-hot-toast';
import { autoParseToolCalls } from '../utils/toolParser';
import { parseConversationFile } from '../utils/conversationParser';
import { areToolCallsComplete } from './useToolExecution';
import type { Message, ToolCall } from '../types';

interface UseConversationWatcherOptions {
  currentConversationId: string | null;
  isStreamingRef: React.MutableRefObject<boolean>;
  setMessages: React.Dispatch<React.SetStateAction<Message[]>>;
  setTokensUsed: React.Dispatch<React.SetStateAction<number | undefined>>;
  setMaxTokens: React.Dispatch<React.SetStateAction<number | undefined>>;
  setIsLoading: React.Dispatch<React.SetStateAction<boolean>>;
  processToolCalls: (toolCalls: ToolCall[], lastMessage: Message) => Promise<void>;
  isMessageProcessed: (messageId: string) => boolean;
  shouldStopExecution: (toolCalls: ToolCall[]) => boolean;
}

/**
 * Hook to watch conversation updates via WebSocket.
 * Handles real-time updates, tool call detection, and context warnings.
 */
export function useConversationWatcher({
  currentConversationId,
  isStreamingRef,
  setMessages,
  setTokensUsed,
  setMaxTokens,
  setIsLoading,
  processToolCalls,
  isMessageProcessed,
  shouldStopExecution,
}: UseConversationWatcherOptions) {
  const [isWsConnected, setIsWsConnected] = useState(false);

  useEffect(() => {
    if (!currentConversationId) {
      setIsWsConnected(false);
      return;
    }

    // Determine WebSocket URL based on current protocol
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${window.location.host}/ws/conversation/watch/${currentConversationId}`;

    const ws = new WebSocket(wsUrl);

    ws.onopen = () => {
      setIsWsConnected(true);
    };

    ws.onmessage = (event) => {
      try {
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
      setIsWsConnected(false);
    };

    ws.onclose = (event) => {
      setIsWsConnected(false);
      console.log('[ConversationWatcher] WebSocket closed:', event.code, event.reason);
    };

    return () => {
      ws.close();
    };

    function handleUpdate(message: any) {
      // Skip updates during active streaming
      if (isStreamingRef.current) {
        console.log('[ConversationWatcher] Skipping update - streaming is active');
        return;
      }

      const content = message.content;

      // Check for context size warnings
      handleContextWarnings(content);

      // Parse and update messages
      const parsedMessages = parseConversationFile(content);
      setMessages(parsedMessages);
      console.log('[ConversationWatcher] Updated messages, count:', parsedMessages.length);

      // Check for generation errors
      if (content.includes('⚠️ Generation Error:')) {
        console.error('[ConversationWatcher] Generation error detected');
        toast.error('Model generation failed. Try simplifying your request or reducing context size.', { duration: 7000 });
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
            toast.error(
              `⚠️ Context size critically low (${reducedSize} tokens)! Model may not work properly.`,
              { duration: 10000 }
            );
          } else {
            toast(`⚠️ Context size reduced to ${reducedSize} tokens due to VRAM limits.`, {
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

  return { isWsConnected };
}
