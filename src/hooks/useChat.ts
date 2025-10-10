import { useState, useCallback, useRef } from 'react';
import { toast } from 'react-hot-toast';
import { TauriAPI } from '../utils/tauri';
import { autoParseToolCalls } from '../utils/toolParser';
import type { Message, ChatRequest, ToolCall } from '../types';

const MAX_TOOL_ITERATIONS = 5; // Safety limit to prevent infinite loops

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
  const toolIterationCount = useRef(0);
  const abortControllerRef = useRef<AbortController | null>(null);

  const addMessage = useCallback((message: Message) => {
    setMessages(prev => [...prev, message]);
  }, []);

  const updateMessage = useCallback((messageId: string, content: string) => {
    setMessages(prev => prev.map(msg => 
      msg.id === messageId ? { ...msg, content } : msg
    ));
  }, []);

  const sendMessage = useCallback(async (content: string) => {
    if (isLoading || !content.trim()) return;

    // Abort any previous request
    if (abortControllerRef.current) {
      console.log('[FRONTEND] Aborting previous request');
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

    addMessage(userMessage);
    setIsLoading(true);
    setError(null);

    // Create assistant message placeholder
    const assistantMessageId = crypto.randomUUID();
    const assistantMessage: Message = {
      id: assistantMessageId,
      role: 'assistant',
      content: '',
      timestamp: Date.now(),
    };
    addMessage(assistantMessage);

    // Agentic loop helper
    const continueWithToolResults = async (toolResults: string) => {
      toolIterationCount.current += 1;

      if (toolIterationCount.current >= MAX_TOOL_ITERATIONS) {
        toast.error('Maximum tool iterations reached. Stopping to prevent infinite loop.', { duration: 5000 });
        setIsLoading(false);
        return;
      }

      // Create a new assistant message for continued generation
      const newAssistantMessageId = crypto.randomUUID();
      const newAssistantMessage: Message = {
        id: newAssistantMessageId,
        role: 'assistant',
        content: '',
        timestamp: Date.now(),
      };
      addMessage(newAssistantMessage);

      const request: ChatRequest = {
        message: toolResults, // Send tool results as the "user" message
        conversation_id: currentConversationId || undefined,
      };

      let streamingContent = '';

      await TauriAPI.sendMessageStream(
        request,
        (token: string, tokensUsed?: number, maxTokens?: number) => {
          streamingContent += token;
          updateMessage(newAssistantMessageId, streamingContent);

          if (tokensUsed !== undefined) {
            setTokensUsed(tokensUsed);
          }
          if (maxTokens !== undefined) {
            setMaxTokens(maxTokens);
          }
        },
        async (_messageId: string, conversationId: string, tokensUsed?: number, maxTokens?: number) => {
          if (!currentConversationId) {
            setCurrentConversationId(conversationId);
          }
          setTokensUsed(tokensUsed);
          setMaxTokens(maxTokens);

          // Check for more tool calls
          const toolCalls = autoParseToolCalls(streamingContent);
          if (toolCalls.length > 0) {
            // Execute tools and continue
            try {
              const results = await Promise.all(toolCalls.map(executeTool));
              const formattedResults = results.map((result) =>
                `[TOOL_RESULTS]${result}[/TOOL_RESULTS]`
              ).join('\n');

              await continueWithToolResults(formattedResults);
            } catch (err) {
              const errorMessage = err instanceof Error ? err.message : 'Tool execution failed';
              toast.error(`Tool error: ${errorMessage}`, { duration: 5000 });
              setIsLoading(false);
            }
          } else {
            // No more tool calls, done
            setIsLoading(false);
          }
        },
        (errorMessage: string) => {
          setError(errorMessage);
          toast.error(`Chat error: ${errorMessage}`, { duration: 5000 });
          updateMessage(newAssistantMessageId, `Error: ${errorMessage}`);
          setIsLoading(false);
        },
        abortControllerRef.current?.signal
      );
    };

    try {
      const request: ChatRequest = {
        message: content.trim(),
        conversation_id: currentConversationId || undefined,
      };

      let streamingContent = '';

      await TauriAPI.sendMessageStream(
        request,
        // onToken - append each token to the message and update token count
        (token: string, tokensUsed?: number, maxTokens?: number) => {
          streamingContent += token;
          updateMessage(assistantMessageId, streamingContent);

          // Update token count in real-time during streaming
          if (tokensUsed !== undefined) {
            setTokensUsed(tokensUsed);
          }
          if (maxTokens !== undefined) {
            setMaxTokens(maxTokens);
          }
        },
        // onComplete - check for tool calls and execute if needed
        async (_messageId: string, conversationId: string, tokensUsed?: number, maxTokens?: number) => {
          if (!currentConversationId) {
            setCurrentConversationId(conversationId);
          }
          // Update final token counts
          setTokensUsed(tokensUsed);
          setMaxTokens(maxTokens);

          // Check if response contains tool calls
          const toolCalls = autoParseToolCalls(streamingContent);

          if (toolCalls.length > 0) {
            // Execute tool calls
            try {
              toast.success(`Executing ${toolCalls.length} tool call(s)...`, { duration: 2000 });

              const results = await Promise.all(toolCalls.map(executeTool));

              // Format results for model
              const formattedResults = results.map((result) =>
                `[TOOL_RESULTS]${result}[/TOOL_RESULTS]`
              ).join('\n');

              // Continue generation with tool results
              await continueWithToolResults(formattedResults);
            } catch (err) {
              const errorMessage = err instanceof Error ? err.message : 'Tool execution failed';
              toast.error(`Tool error: ${errorMessage}`, { duration: 5000 });
              setIsLoading(false);
            }
          } else {
            // No tool calls, generation complete
            setIsLoading(false);
          }
        },
        // onError - handle errors
        (errorMessage: string) => {
          setError(errorMessage);
          toast.error(`Chat error: ${errorMessage}`, { duration: 5000 });
          updateMessage(assistantMessageId, `Error: ${errorMessage}`);
          setIsLoading(false);
        },
        abortControllerRef.current?.signal
      );

    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'An unknown error occurred';
      setError(errorMessage);
      toast.error(`Chat error: ${errorMessage}`, { duration: 5000 });
      updateMessage(assistantMessageId, `Error: ${errorMessage}`);
      setIsLoading(false);
    }
  }, [isLoading, currentConversationId, addMessage, updateMessage]);

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
  };
}