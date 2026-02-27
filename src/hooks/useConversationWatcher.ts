import { useEffect, useRef } from 'react';
import { toast } from 'react-hot-toast';
import { parseConversationFile } from '../utils/conversationParser';
import { logToastError, logToastWarning } from '../utils/toastLogger';
import { isTauriEnv } from '../utils/tauri';
import { getConversation } from '../utils/tauriCommands';
import type { Message } from '../types';

/**
 * Check if the content has an unclosed tool response/output tag, indicating
 * a command is still executing. Used to avoid prematurely declaring generation done.
 */
function hasUnclosedToolExecution(content: string): boolean {
  // Qwen/generic: <tool_response> without matching </tool_response>
  const lastTrOpen = content.lastIndexOf('<tool_response>');
  const lastTrClose = content.lastIndexOf('</tool_response>');
  if (lastTrOpen !== -1 && lastTrOpen > lastTrClose) return true;

  // SYSTEM.EXEC format: <||SYSTEM.OUTPUT> without closing
  const lastSoOpen = content.lastIndexOf('SYSTEM.OUTPUT>');
  const lastSoClose = content.lastIndexOf('SYSTEM.OUTPUT||>');
  if (lastSoOpen !== -1 && lastSoOpen > lastSoClose) return true;

  // Mistral: [TOOL_RESULTS] without [/TOOL_RESULTS]
  const lastMrOpen = content.lastIndexOf('[TOOL_RESULTS]');
  const lastMrClose = content.lastIndexOf('[/TOOL_RESULTS]');
  if (lastMrOpen !== -1 && lastMrOpen > lastMrClose) return true;

  // Unclosed tool call (model still generating the call, before execution)
  const lastTcOpen = content.lastIndexOf('<tool_call>');
  const lastTcClose = content.lastIndexOf('</tool_call>');
  if (lastTcOpen !== -1 && lastTcOpen > lastTcClose) return true;

  return false;
}

interface UseConversationWatcherOptions {
  currentConversationId: string | null;
  isStreamingRef: React.MutableRefObject<boolean>;
  currentMessagesRef: React.MutableRefObject<Message[]>;
  setMessages: React.Dispatch<React.SetStateAction<Message[]>>;
  setTokensUsed: React.Dispatch<React.SetStateAction<number | undefined>>;
  setMaxTokens: React.Dispatch<React.SetStateAction<number | undefined>>;
  setIsLoading: React.Dispatch<React.SetStateAction<boolean>>;
}

const WS_RECONNECT_BASE_MS = 500;
const WS_RECONNECT_MAX_MS = 5000;
const WS_STUCK_STREAMING_POLL_MS = 15_000;

/**
 * Pick the best message list between incoming (server) and local (UI).
 * During streaming, prefer whichever has more content to avoid flickering.
 */
function reconcileMessages(incoming: Message[], local: Message[], isStreaming: boolean): Message[] {
  if (!isStreaming || local.length === 0) {
    // Preserve timings from local messages (attached by onComplete) that
    // the file-parsed incoming messages don't have.
    // Match by index+role since IDs differ between local and parsed messages.
    if (local.length > 0 && incoming.length > 0) {
      let hasTimings = false;
      for (const msg of local) {
        if (msg.timings) { hasTimings = true; break; }
      }
      if (hasTimings) {
        return incoming.map((msg, i) => {
          const localMsg = local[i];
          if (localMsg?.timings && localMsg.role === msg.role) {
            return { ...msg, timings: localMsg.timings };
          }
          return msg;
        });
      }
    }
    return incoming;
  }

  const localLast = local[local.length - 1];
  const incomingLast = incoming[incoming.length - 1];

  if (localLast?.role === 'assistant') {
    const incomingAssistant = incomingLast?.role === 'assistant' ? incomingLast : null;
    const incomingLength = incomingAssistant?.content.length ?? 0;
    if (incomingLength < localLast.content.length || incoming.length < local.length) {
      return local;
    }
  } else if (incoming.length < local.length) {
    return local;
  }

  return incoming;
}

function getNextDelay(attempt: number): number {
  const exp = WS_RECONNECT_BASE_MS * Math.pow(2, attempt);
  return Math.min(exp, WS_RECONNECT_MAX_MS);
}

/**
 * Hook to watch conversation updates via WebSocket.
 * Handles real-time updates and context warnings.
 *
 * Tool execution is handled entirely by the backend inline during generation.
 * This hook only manages message display and loading state.
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

      const content = lastMessage.content.trim();
      if (content.length === 0) return;

      // Don't declare generation "done" if there's an unclosed tool response tag —
      // that means a command is still executing and we're waiting for output.
      if (hasUnclosedToolExecution(content)) return;

      isStreamingRef.current = false;
      setIsLoading(false);
    };

    const fetchConversation = async () => {
      if (!shouldPollConversation()) {
        return;
      }
      try {
        const data = await getConversation(currentConversationId!);
        const parsedMessages = data.content
          ? parseConversationFile(data.content)
          : data.messages
            ? (data.messages as unknown as Message[])
            : null;

        if (!parsedMessages) return;

        const nextMessages = reconcileMessages(parsedMessages, currentMessagesRef.current, isStreamingRef.current);
        setMessages(nextMessages);

        if (isStreamingRef.current) {
          const lastMessage = nextMessages[nextMessages.length - 1];
          const assistantContent = lastMessage?.role === 'assistant' ? lastMessage.content : '';
          if (!assistantContent) return;

          if (assistantContent === lastPolledAssistantContentRef.current) {
            stablePollsRef.current += 1;
          } else {
            stablePollsRef.current = 0;
            lastPolledAssistantContentRef.current = assistantContent;
          }

          if (stablePollsRef.current < 1) return;
        } else {
          stablePollsRef.current = 0;
          lastPolledAssistantContentRef.current = '';
        }

        await reconcileUiStateFromMessages(nextMessages);
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
    setIsLoading,
    setMessages,
  ]);

  useEffect(() => {
    if (!currentConversationId) {
      return;
    }

    // In Tauri mode, token streaming is handled by events from generate_stream.
    // No WebSocket needed — just skip.
    if (isTauriEnv()) {
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
      if (content.includes('\u26a0\ufe0f Generation Error:')) {
        console.error('[ConversationWatcher] Generation error detected');
        const display = 'Model generation failed. Try simplifying your request or reducing context size.';
        logToastError('useConversationWatcher.handleUpdate', display);
        toast.error(display, { duration: 7000 });
        setIsLoading(false);
        return;
      }

      // No tool calls to handle — backend does this inline during generation.
      // Just turn off loading spinner.
      setIsLoading(false);

      // Update token counts
      if (message.tokens_used !== undefined && message.tokens_used !== null) {
        setTokensUsed(message.tokens_used);
      }
      if (message.max_tokens !== undefined && message.max_tokens !== null) {
        setMaxTokens(message.max_tokens);
      }
    }

    function handleContextWarnings(content: string) {
      if (content.includes('\u26a0\ufe0f Context Size Reduced') && content.includes('Auto-reduced to:')) {
        const match = content.match(/Auto-reduced to: (\d+) tokens/);
        if (match) {
          const reducedSize = parseInt(match[1]);
          if (reducedSize < 4096) {
            const display = `\u26a0\ufe0f Context size critically low (${reducedSize} tokens)! Model may not work properly.`;
            logToastWarning('useConversationWatcher.context', display);
            toast.error(display, { duration: 10000 });
          } else {
            const display = `\u26a0\ufe0f Context size reduced to ${reducedSize} tokens due to VRAM limits.`;
            logToastWarning('useConversationWatcher.context', display);
            toast(display, {
              duration: 5000,
              icon: '\u26a0\ufe0f',
            });
          }
        }
      }
    }
  }, [
    currentConversationId,
    isStreamingRef,
    setMessages,
    setTokensUsed,
    setMaxTokens,
    setIsLoading,
  ]);

  return {};
}
