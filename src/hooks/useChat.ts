import { useState, useCallback, useRef, useEffect } from 'react';
import { flushSync } from 'react-dom';
import { unstable_batchedUpdates } from 'react-dom';
import { toast } from 'react-hot-toast';
import { TauriAPI } from '../utils/tauri';
import { autoParseToolCalls } from '../utils/toolParser';
import type { Message, ChatRequest, ToolCall } from '../types';

const MAX_TOOL_ITERATIONS = 20; // Maximum tool calls per user message (safety limit)
const LOOP_DETECTION_WINDOW = 3; // Detect loop if same call repeats this many times

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

// Helper to get conversation ID from URL
function getConversationFromUrl(): string | null {
  const params = new URLSearchParams(window.location.search);
  return params.get('conversation');
}

// Helper to update URL with conversation ID
function updateUrlWithConversation(conversationId: string | null) {
  const url = new URL(window.location.href);
  if (conversationId) {
    url.searchParams.set('conversation', conversationId);
  } else {
    url.searchParams.delete('conversation');
  }
  window.history.replaceState({}, '', url.toString());
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
  const toolCallHistory = useRef<string[]>([]); // Track recent tool calls for loop detection
  const abortControllerRef = useRef<AbortController | null>(null);
  const lastProcessedMessageId = useRef<string | null>(null);
  const isStreamingRef = useRef(false); // Track if we're actively streaming tokens
  const initialLoadDone = useRef(false); // Track if initial URL load was done

  // Helper to process tool calls - defined as ref to avoid stale closures
  const processToolCallsRef = useRef<((toolCalls: ToolCall[], lastMessage: Message) => void) | null>(null);

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

    // Reset tool iteration counter ONLY for NEW user messages
    // Do NOT reset for tool results being sent back to model
    const isToolResult = content.startsWith('[TOOL_RESULTS]');
    if (!isToolResult) {
      toolIterationCount.current = 0;
      toolCallHistory.current = []; // Reset loop detection history
      console.log('[FRONTEND] 🔄 Reset tool iteration counter and history for new user message');
    } else {
      console.log(`[FRONTEND] 🔧 Continuing tool iteration ${toolIterationCount.current}/${MAX_TOOL_ITERATIONS} with tool results`);
    }

    const userMessage: Message = {
      id: crypto.randomUUID(),
      role: 'user',
      content: content.trim(),
      timestamp: Date.now(),
    };

    // Add user message immediately for instant feedback
    // BUT skip adding tool results to UI - they're only for the model
    if (!isToolResult) {
      setMessages(prev => [...prev, userMessage]);
    }
    setIsLoading(true);
    setError(null);

    // Create a placeholder assistant message that we'll stream into
    const assistantMessageId = crypto.randomUUID();
    const assistantMessage: Message = {
      id: assistantMessageId,
      role: 'assistant',
      content: '',
      timestamp: Date.now(),
    };

    // Add empty assistant message to show streaming indicator
    // IMPORTANT: Use flushSync to ensure state is committed before streaming starts
    // Without this, the onToken callback may receive stale state where the assistant
    // message doesn't exist yet, causing tokens to be silently discarded
    flushSync(() => {
      setMessages(prev => [...prev, assistantMessage]);
    });

    try {
      const request: ChatRequest = {
        message: content.trim(),
        conversation_id: currentConversationId || undefined,
      };

      // Mark streaming as active to prevent watcher from overwriting state
      isStreamingRef.current = true;
      console.log('[USECHAR] Streaming started, isStreamingRef = true');

      // Use WebSocket streaming for real-time token updates
      await TauriAPI.sendMessageStream(
        request,
        // onToken - called for each token generated
        (token, tokenCount, maxTokenCount) => {
          // Use flushSync to force immediate DOM update for streaming effect
          flushSync(() => {
            setMessages(prev => {
              const lastMsg = prev[prev.length - 1];
              if (lastMsg && lastMsg.id === assistantMessageId) {
                return [
                  ...prev.slice(0, -1),
                  { ...lastMsg, content: lastMsg.content + token }
                ];
              }
              return prev;
            });
          });
          if (tokenCount !== undefined) setTokensUsed(tokenCount);
          if (maxTokenCount !== undefined) setMaxTokens(maxTokenCount);
        },
        // onComplete - called when generation finishes
        (_messageId, conversationId, tokenCount, maxTokenCount) => {
          // Mark streaming as complete - allow watcher to update again
          isStreamingRef.current = false;
          console.log('[USECHAR] Streaming complete, isStreamingRef = false');

          if (!currentConversationId) {
            setCurrentConversationId(conversationId);
          }
          if (tokenCount !== undefined) setTokensUsed(tokenCount);
          if (maxTokenCount !== undefined) setMaxTokens(maxTokenCount);

          // Check for tool calls in the final message
          setMessages(prev => {
            const lastMsg = prev[prev.length - 1];
            if (lastMsg && lastMsg.role === 'assistant' && lastMsg.content) {
              const toolCalls = autoParseToolCalls(lastMsg.content);
              if (toolCalls.length > 0 && processToolCallsRef.current) {
                // Tool calls detected - process them
                processToolCallsRef.current(toolCalls, lastMsg);
              } else if (toolCalls.length === 0) {
                // No tool calls - we're done
                setIsLoading(false);
              }
            } else {
              setIsLoading(false);
            }
            return prev;
          });
        },
        // onError - called on any error
        (errorMsg) => {
          isStreamingRef.current = false;
          console.log('[USECHAR] Streaming error, isStreamingRef = false');
          setError(errorMsg);
          toast.error(`Chat error: ${errorMsg}`, { duration: 5000 });
          setIsLoading(false);
        },
        abortControllerRef.current?.signal
      );

    } catch (err) {
      isStreamingRef.current = false;
      const errorMessage = err instanceof Error ? err.message : 'An unknown error occurred';
      setError(errorMessage);
      toast.error(`Chat error: ${errorMessage}`, { duration: 5000 });
      setIsLoading(false);
    }
  }, [isLoading, currentConversationId]);

  // Assign processToolCalls function to ref (avoids stale closure issues)
  useEffect(() => {
    processToolCallsRef.current = (toolCalls: ToolCall[], lastMessage: Message) => {
      // Check if context size is critically low
      if (typeof maxTokens === 'number' && maxTokens > 0 && maxTokens < 4096) {
        console.error(`[FRONTEND] ⚠️  Context size too small (${maxTokens} tokens) for reliable tool execution. Stopping.`);
        toast.error(
          `Context size is too small (${maxTokens} tokens) for tool calling. Please reduce GPU layers or unload the model.`,
          { duration: 10000 }
        );
        setIsLoading(false);
        return;
      }

      // Check iteration limit BEFORE executing tools
      if (toolIterationCount.current >= MAX_TOOL_ITERATIONS) {
        console.warn(`[FRONTEND] ⚠️  MAX_TOOL_ITERATIONS (${MAX_TOOL_ITERATIONS}) reached. Stopping tool execution loop.`);
        toast.error(`Maximum tool iterations (${MAX_TOOL_ITERATIONS}) reached. Stopping to prevent infinite loop.`, { duration: 5000 });
        setIsLoading(false);
        return;
      }

      // Smart loop detection: Check if the same tool calls are repeating
      const toolCallSignatures = toolCalls.map(tc => `${tc.name}(${JSON.stringify(tc.arguments)})`).join('|');
      toolCallHistory.current.push(toolCallSignatures);

      // Check for infinite loop pattern
      if (toolCallHistory.current.length >= LOOP_DETECTION_WINDOW) {
        const recentCalls = toolCallHistory.current.slice(-LOOP_DETECTION_WINDOW);
        const allIdentical = recentCalls.every(call => call === recentCalls[0]);

        if (allIdentical) {
          console.error(`[FRONTEND] 🔁 INFINITE LOOP DETECTED: Same tool call repeated ${LOOP_DETECTION_WINDOW} times`);
          console.error(`[FRONTEND] Repeating call: ${recentCalls[0]}`);
          toast.error(`Infinite loop detected! The model is repeating the same tool call. Stopping.`, { duration: 7000 });
          setIsLoading(false);
          return;
        }
      }

      // Increment iteration counter
      toolIterationCount.current += 1;
      console.log(`[FRONTEND] 🔧 Tool iteration: ${toolIterationCount.current}/${MAX_TOOL_ITERATIONS}`);
      console.log(`[FRONTEND] Tool call history (last ${Math.min(5, toolCallHistory.current.length)}):`, toolCallHistory.current.slice(-5));

      // Mark message as processed
      lastProcessedMessageId.current = lastMessage.id;

      toast.success(`Executing ${toolCalls.length} tool call(s)... (iteration ${toolIterationCount.current}/${MAX_TOOL_ITERATIONS})`, { duration: 2000 });

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
          setTimeout(() => {
            sendMessage(formattedResults, true); // bypass loading check for tool results
          }, 10);
        })
        .catch((err) => {
          const errorMessage = err instanceof Error ? err.message : 'Tool execution failed';
          toast.error(`Tool error: ${errorMessage}`, { duration: 5000 });
          setIsLoading(false);
        });
    };
  }, [sendMessage, maxTokens]);

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
          // Skip watcher updates during active streaming - streaming handles its own state
          if (isStreamingRef.current) {
            console.log('[WATCHER] Skipping update - streaming is active');
            return;
          }

          // Parse the file content and update messages
          const content = message.content;

          // Check for context size warnings in the conversation
          if (content.includes('⚠️ Context Size Reduced') && content.includes('Auto-reduced to:')) {
            const match = content.match(/Auto-reduced to: (\d+) tokens/);
            if (match) {
              const reducedSize = parseInt(match[1]);
              if (reducedSize < 4096) {
                toast.error(
                  `⚠️ Context size critically low (${reducedSize} tokens)! Model may not work properly. Reduce GPU layers or use smaller model.`,
                  { duration: 10000 }
                );
              } else {
                toast(
                  `⚠️ Context size reduced to ${reducedSize} tokens due to VRAM limits.`,
                  { duration: 5000, icon: '⚠️' }
                );
              }
            }
          }

          const parsedMessages = parseConversationFile(content);
          setMessages(parsedMessages);
          console.log('[WATCHER] Updated messages from file, count:', parsedMessages.length);

          // Check for generation errors in the content
          if (content.includes('⚠️ Generation Error:')) {
            console.error('[FRONTEND] Generation error detected in conversation');
            toast.error('Model generation failed. Try simplifying your request or reducing context size.', { duration: 7000 });
            setIsLoading(false);
            return;
          }

          // Check if assistant has started responding
          const lastMessage = parsedMessages[parsedMessages.length - 1];
          if (lastMessage && lastMessage.role === 'assistant' && lastMessage.content.length > 0) {
            // Check for tool calls in the assistant's message
            const toolCalls = autoParseToolCalls(lastMessage.content);
            console.log('[FRONTEND] 🔍 Detected', toolCalls.length, 'tool calls in message');

            if (toolCalls.length > 0) {
              // Check if ALL tool calls are complete
              // A tool call is complete if it has both opening and closing tags
              const qwenComplete = (lastMessage.content.match(/<tool_call>/g) || []).length === (lastMessage.content.match(/<\/tool_call>/g) || []).length;
              const llama3Complete = (lastMessage.content.match(/<function=/g) || []).length === (lastMessage.content.match(/<\/function>/g) || []).length;

              // Mistral format has NO closing tag - tool call ends after JSON
              // Format: [TOOL_CALLS]function_name[ARGS]{"arg": "value"}
              // Check if the message contains [TOOL_CALLS] and valid JSON after [ARGS]
              const hasMistralToolCall = lastMessage.content.includes('[TOOL_CALLS]');
              let mistralComplete = false;
              if (hasMistralToolCall) {
                // Try to parse the tool call - if it succeeds, it's complete
                try {
                  const parsed = autoParseToolCalls(lastMessage.content);
                  mistralComplete = parsed.length > 0;
                  console.log('[FRONTEND] 🔧 Mistral tool call complete:', mistralComplete, '(parsed', parsed.length, 'calls)');
                } catch {
                  mistralComplete = false;
                  console.log('[FRONTEND] ❌ Mistral tool call parse failed');
                }
              }

              const hasCompleteToolCall = qwenComplete || llama3Complete || mistralComplete;
              console.log('[FRONTEND] ✅ Tool call completeness:', { qwenComplete, llama3Complete, mistralComplete, hasCompleteToolCall });

              if (hasCompleteToolCall && lastProcessedMessageId.current !== lastMessage.id) {
                // Check if context size is critically low (indicates VRAM issues)
                // Only check if we have a valid positive number that's below threshold
                // Skip check if maxTokens is null/undefined (means we haven't received token counts yet)
                if (typeof maxTokens === 'number' && maxTokens > 0 && maxTokens < 4096) {
                  console.error(`[FRONTEND] ⚠️  Context size too small (${maxTokens} tokens) for reliable tool execution. Stopping.`);
                  toast.error(
                    `Context size is too small (${maxTokens} tokens) for tool calling. Please reduce GPU layers or unload the model.`,
                    { duration: 10000 }
                  );
                  setIsLoading(false);
                  return;
                }

                // Check iteration limit BEFORE executing tools
                if (toolIterationCount.current >= MAX_TOOL_ITERATIONS) {
                  console.warn(`[FRONTEND] ⚠️  MAX_TOOL_ITERATIONS (${MAX_TOOL_ITERATIONS}) reached. Stopping tool execution loop.`);
                  toast.error(`Maximum tool iterations (${MAX_TOOL_ITERATIONS}) reached. Stopping to prevent infinite loop.`, { duration: 5000 });
                  setIsLoading(false);
                  return;
                }

                // Smart loop detection: Check if the same tool calls are repeating
                const toolCallSignatures = toolCalls.map(tc => `${tc.name}(${JSON.stringify(tc.arguments)})`).join('|');
                toolCallHistory.current.push(toolCallSignatures);

                // Check for infinite loop pattern
                if (toolCallHistory.current.length >= LOOP_DETECTION_WINDOW) {
                  const recentCalls = toolCallHistory.current.slice(-LOOP_DETECTION_WINDOW);
                  const allIdentical = recentCalls.every(call => call === recentCalls[0]);

                  if (allIdentical) {
                    console.error(`[FRONTEND] 🔁 INFINITE LOOP DETECTED: Same tool call repeated ${LOOP_DETECTION_WINDOW} times`);
                    console.error(`[FRONTEND] Repeating call: ${recentCalls[0]}`);
                    toast.error(`Infinite loop detected! The model is repeating the same tool call. Stopping.`, { duration: 7000 });
                    setIsLoading(false);
                    return;
                  }
                }

                // Increment iteration counter
                toolIterationCount.current += 1;
                console.log(`[FRONTEND] 🔧 Tool iteration: ${toolIterationCount.current}/${MAX_TOOL_ITERATIONS}`);
                console.log(`[FRONTEND] Tool call history (last ${Math.min(5, toolCallHistory.current.length)}):`, toolCallHistory.current.slice(-5));

                // Tool calls are complete and haven't been processed yet - execute them
                lastProcessedMessageId.current = lastMessage.id;

                toast.success(`Executing ${toolCalls.length} tool call(s)... (iteration ${toolIterationCount.current}/${MAX_TOOL_ITERATIONS})`, { duration: 2000 });

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

  // Sync URL with current conversation ID
  useEffect(() => {
    // Skip URL update during initial load to prevent overwriting URL param
    if (!initialLoadDone.current) return;
    updateUrlWithConversation(currentConversationId);
  }, [currentConversationId]);

  // Load conversation from URL on initial mount
  useEffect(() => {
    if (initialLoadDone.current) return;
    initialLoadDone.current = true;

    const conversationFromUrl = getConversationFromUrl();
    if (conversationFromUrl) {
      console.log('[USECHAT] Loading conversation from URL:', conversationFromUrl);
      // Ensure .txt extension for consistency with sidebar
      const normalizedId = conversationFromUrl.endsWith('.txt')
        ? conversationFromUrl
        : `${conversationFromUrl}.txt`;
      loadConversation(normalizedId);
    }
  }, [loadConversation]);

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