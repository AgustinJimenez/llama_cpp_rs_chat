import { useState, useCallback, useRef, useEffect } from 'react';
import { toast } from 'react-hot-toast';
import { TauriAPI } from '../utils/tauri';
import { autoParseToolCalls } from '../utils/toolParser';
import type { Message, ChatRequest, ToolCall } from '../types';

// const MAX_TOOL_ITERATIONS = 5; // Safety limit to prevent infinite loops

// Helper function to parse conversation file content
function parseConversationFile(content: string): Message[] {
  const messages: Message[] = [];
  let currentRole = '';
  let currentContent = '';

  for (const line of content.split('\n')) {
    if (
      line.endsWith(':') &&
      (line.startsWith('SYSTEM:') || line.startsWith('USER:') || line.startsWith('ASSISTANT:'))
    ) {
      // Save previous message if it exists
      if (currentRole && currentContent.trim()) {
        const role = currentRole === 'USER' ? 'user' : currentRole === 'ASSISTANT' ? 'assistant' : 'system';
        const content = currentContent.trim();

        // Skip system messages, tool results, and tool-only responses in the UI
        const isToolResults = content.startsWith('[TOOL_RESULTS]');
        // Check if message only contains tool calls (and optionally thinking tags)
        const contentWithoutThinking = content.replace(/<think>[\s\S]*?<\/think>/g, '').trim();
        const hasQwenToolCall = contentWithoutThinking.includes('<tool_call>');
        const hasLlama3ToolCall = contentWithoutThinking.includes('<function=');
        const hasMistralToolCall = contentWithoutThinking.includes('[TOOL_CALLS]');
        const hasToolCall = hasQwenToolCall || hasLlama3ToolCall || hasMistralToolCall;
        const contentWithoutTools = contentWithoutThinking
          .replace(/<tool_call>[\s\S]*?<\/tool_call>/g, '')
          .replace(/<function=[\s\S]*?<\/function>/g, '')
          .replace(/\[TOOL_CALLS\][\s\S]*?\[\/ARGS\]/g, '')
          .trim();
        const isToolCallOnly = hasToolCall && !contentWithoutTools;

        if (role !== 'system' && !isToolResults && !isToolCallOnly) {
          messages.push({
            id: crypto.randomUUID(),
            role: role as 'user' | 'assistant',
            content: content,
            timestamp: Date.now(),
          });
        }
      }

      // Start new message
      currentRole = line.replace(':', '');
      currentContent = '';
    } else if (!line.startsWith('[COMMAND:') && line.trim()) {
      // Skip command execution logs, add content
      currentContent += line + '\n';
    }
  }

  // Add the final message
  if (currentRole && currentContent.trim()) {
    const role = currentRole === 'USER' ? 'user' : currentRole === 'ASSISTANT' ? 'assistant' : 'system';
    const content = currentContent.trim();

    // Skip system messages, tool results, and tool-only responses in the UI
    const isToolResults = content.startsWith('[TOOL_RESULTS]');
    // Check if message only contains tool calls (and optionally thinking tags)
    const contentWithoutThinking = content.replace(/<think>[\s\S]*?<\/think>/g, '').trim();
    const hasQwenToolCall = contentWithoutThinking.includes('<tool_call>');
    const hasLlama3ToolCall = contentWithoutThinking.includes('<function=');
    const hasMistralToolCall = contentWithoutThinking.includes('[TOOL_CALLS]');
    const hasToolCall = hasQwenToolCall || hasLlama3ToolCall || hasMistralToolCall;
    const contentWithoutTools = contentWithoutThinking
      .replace(/<tool_call>[\s\S]*?<\/tool_call>/g, '')
      .replace(/<function=[\s\S]*?<\/function>/g, '')
      .replace(/\[TOOL_CALLS\][\s\S]*?\[\/ARGS\]/g, '')
      .trim();
    const isToolCallOnly = hasToolCall && !contentWithoutTools;

    if (role !== 'system' && !isToolResults && !isToolCallOnly) {
      messages.push({
        id: crypto.randomUUID(),
        role: role as 'user' | 'assistant',
        content: content,
        timestamp: Date.now(),
      });
    }
  }

  return messages;
}

// Helper function to execute a tool
async function executeTool(toolCall: ToolCall): Promise<string> {
  try {
    const response = await fetch('/api/tools/execute', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        tool_name: toolCall.name,
        arguments: toolCall.arguments,
      }),
    });

    if (!response.ok) {
      throw new Error(`Tool execution failed: ${response.statusText}`);
    }

    const result = await response.json();

    if (result.success) {
      return result.result || 'Tool executed successfully';
    } else {
      return `Error: ${result.error || 'Unknown error'}`;
    }
  } catch (error) {
    return `Error executing tool: ${error instanceof Error ? error.message : 'Unknown error'}`;
  }
}

export function useChat() {
  const [messages, setMessages] = useState<Message[]>([]);
  const [currentConversationId, setCurrentConversationId] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [tokensUsed, setTokensUsed] = useState<number | undefined>(undefined);
  const [maxTokens, setMaxTokens] = useState<number | undefined>(undefined);
  const [isWsConnected, setIsWsConnected] = useState(false);
  const toolIterationCount = useRef(0);
  const abortControllerRef = useRef<AbortController | null>(null);
  const lastProcessedMessageId = useRef<string | null>(null);

  // const addMessage = useCallback((message: Message) => {
  //   setMessages(prev => [...prev, message]);
  // }, []);

  // const updateMessage = useCallback((messageId: string, content: string) => {
  //   setMessages(prev => prev.map(msg =>
  //     msg.id === messageId ? { ...msg, content } : msg
  //   ));
  // }, []);

  const sendMessage = useCallback(async (content: string, bypassLoadingCheck = false) => {
    if (!bypassLoadingCheck && (isLoading || !content.trim())) {
      return;
    }
    if (!content.trim()) {
      return;
    }

    // Abort any previous request
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
    }

    // Create new AbortController for this request
    abortControllerRef.current = new AbortController();

    // Reset tool iteration counter for new user message
    toolIterationCount.current = 0;

    const userMessage: Message = {
      id: crypto.randomUUID(),
      role: 'user',
      content: content.trim(),
      timestamp: Date.now(),
    };

    // Add user message immediately for instant feedback
    setMessages(prev => [...prev, userMessage]);
    setIsLoading(true);
    setError(null);

    try {
      const request: ChatRequest = {
        message: content.trim(),
        conversation_id: currentConversationId || undefined,
      };

      // Use simple HTTP API instead of WebSocket streaming
      // The file watcher will update the UI in real-time
      const response = await TauriAPI.sendMessage(request);


      // Update conversation ID if this is a new conversation
      if (!currentConversationId) {
        setCurrentConversationId(response.conversation_id);
      }

      // Update token counts
      setTokensUsed(response.tokens_used);
      setMaxTokens(response.max_tokens);

      // Don't check for tool calls from HTTP response - it's empty
      // Tool calls will be detected from WebSocket updates instead
      // (see WebSocket onmessage handler below)

      // Keep loading state active - WebSocket will update with actual content
      // and detect tool calls when generation completes

    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'An unknown error occurred';
      setError(errorMessage);
      toast.error(`Chat error: ${errorMessage}`, { duration: 5000 });
      setIsLoading(false);
    }
  }, [isLoading, currentConversationId]);

  const clearMessages = useCallback(() => {
    setMessages([]);
    setCurrentConversationId(null);
    setError(null);
    setTokensUsed(undefined);
    setMaxTokens(undefined);
  }, []);

  const loadConversation = useCallback(async (filename: string) => {
    setIsLoading(true);
    setError(null);
    setTokensUsed(undefined);
    setMaxTokens(undefined);

    try {
      const response = await fetch(`/api/conversation/${filename}`);
      if (!response.ok) {
        throw new Error('Failed to load conversation');
      }

      const data = await response.json();
      if (data.messages) {
        setMessages(data.messages);
        // Extract conversation ID from filename if needed
        setCurrentConversationId(filename);
      }
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to load conversation';
      setError(errorMessage);
      toast.error(`Failed to load conversation: ${errorMessage}`, { duration: 5000 });
    } finally {
      setIsLoading(false);
    }
  }, []);

  // Watch for file changes when viewing a conversation
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
          // Parse the file content and update messages
          const content = message.content;
          const parsedMessages = parseConversationFile(content);
          setMessages(parsedMessages);

          // Check if assistant has started responding
          const lastMessage = parsedMessages[parsedMessages.length - 1];
          if (lastMessage && lastMessage.role === 'assistant' && lastMessage.content.length > 0) {
            // Check for tool calls in the assistant's message
            const toolCalls = autoParseToolCalls(lastMessage.content);

            if (toolCalls.length > 0) {
              // Check if ALL tool calls are complete
              // A tool call is complete if it has both opening and closing tags
              const qwenComplete = (lastMessage.content.match(/<tool_call>/g) || []).length === (lastMessage.content.match(/<\/tool_call>/g) || []).length;
              const llama3Complete = (lastMessage.content.match(/<function=/g) || []).length === (lastMessage.content.match(/<\/function>/g) || []).length;
              const mistralComplete = (lastMessage.content.match(/\[TOOL_CALLS\]/g) || []).length === (lastMessage.content.match(/\[\/ARGS\]/g) || []).length;

              const hasCompleteToolCall = qwenComplete || llama3Complete || mistralComplete;

              if (hasCompleteToolCall && lastProcessedMessageId.current !== lastMessage.id) {
                // Tool calls are complete and haven't been processed yet - execute them
                lastProcessedMessageId.current = lastMessage.id;

                toast.success(`Executing ${toolCalls.length} tool call(s)...`, { duration: 2000 });

                // Execute tool calls
                Promise.all(toolCalls.map(executeTool))
                  .then((results) => {
                    // Format results for model
                    const formattedResults = results.map((result) =>
                      `[TOOL_RESULTS]${result}[/TOOL_RESULTS]`
                    ).join('\n');

                    // Turn off loading before sending results
                    setIsLoading(false);

                    // Continue generation with tool results
                    // Use setTimeout to ensure state update completes first
                    setTimeout(() => {
                      sendMessage(formattedResults, true); // bypass loading check for tool results
                    }, 10);
                  })
                  .catch((err) => {
                    const errorMessage = err instanceof Error ? err.message : 'Tool execution failed';
                    toast.error(`Tool error: ${errorMessage}`, { duration: 5000 });
                    setIsLoading(false);
                  });
              }
              // If hasCompleteToolCall but already processed, do nothing (keep loading for next iteration)
            } else {
              // No tool calls - turn off loading spinner
              setIsLoading(false);
            }
          }

          // Update token counts if provided
          if (message.tokens_used !== undefined && message.tokens_used !== null) {
            setTokensUsed(message.tokens_used);
          }
          if (message.max_tokens !== undefined && message.max_tokens !== null) {
            setMaxTokens(message.max_tokens);
          }
        }
      } catch (e) {
        console.error('[FRONTEND] Failed to parse file update:', e);
      }
    };

    ws.onerror = (error) => {
      console.error('[FRONTEND] ❌ WebSocket ERROR:', error);
      console.error('[FRONTEND] WebSocket URL was:', wsUrl);
      setIsWsConnected(false);
    };

    ws.onclose = (event) => {
      setIsWsConnected(false);
      console.log('[FRONTEND] WebSocket closed. Code:', event.code, 'Reason:', event.reason, 'Clean:', event.wasClean);
    };

    // Clean up on unmount or when conversation changes
    return () => {
      ws.close();
    };
  }, [currentConversationId]);

  return {
    messages,
    isLoading,
    error,
    sendMessage,
    clearMessages,
    loadConversation,
    currentConversationId,
    tokensUsed,
    maxTokens,
    isWsConnected,
  };
}