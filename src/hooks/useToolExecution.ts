import { useRef, useCallback } from 'react';
import { toast } from 'react-hot-toast';
import { executeTool as executeToolCmd } from '../utils/tauriCommands';
import type { ToolCall, Message } from '../types';

// Configuration constants
export const MAX_TOOL_ITERATIONS = 20;
export const LOOP_DETECTION_WINDOW = 3;

/**
 * Execute a single tool call via the API
 */
export async function executeTool(toolCall: ToolCall): Promise<string> {
  try {
    const result = await executeToolCmd(toolCall);

    if (result.success) {
      return (result.result as string) || 'Tool executed successfully';
    } else {
      return `Error: ${(result.error as string) || 'Unknown error'}`;
    }
  } catch (error) {
    return `Error executing tool: ${error instanceof Error ? error.message : 'Unknown error'}`;
  }
}

/**
 * Format tool results for sending back to the model
 */
export function formatToolResults(results: string[]): string {
  return results.map((result) => `[TOOL_RESULTS]${result}[/TOOL_RESULTS]`).join('\n');
}

/**
 * Check if tool calls are complete (have closing tags)
 */
export function areToolCallsComplete(content: string, toolCalls: ToolCall[]): boolean {
  if (toolCalls.length === 0) return false;

  const qwenComplete =
    (content.match(/<tool_call>/g) || []).length ===
    (content.match(/<\/tool_call>/g) || []).length;

  const llama3Complete =
    (content.match(/<function=/g) || []).length ===
    (content.match(/<\/function>/g) || []).length;

  // Mistral format has no closing tag - check if parsing succeeds
  const hasMistralToolCall = content.includes('[TOOL_CALLS]');
  const mistralComplete = hasMistralToolCall && toolCalls.length > 0;

  return qwenComplete || llama3Complete || mistralComplete;
}

interface UseToolExecutionOptions {
  maxTokens: number | undefined;
  sendMessage: (content: string, bypassLoadingCheck?: boolean) => Promise<void>;
  setIsLoading: (loading: boolean) => void;
}

interface ToolExecutionState {
  iterationCount: React.MutableRefObject<number>;
  callHistory: React.MutableRefObject<string[]>;
  lastProcessedMessageId: React.MutableRefObject<string | null>;
}

/**
 * Hook for managing tool execution state and logic
 */
export function useToolExecution({
  maxTokens,
  sendMessage,
  setIsLoading,
}: UseToolExecutionOptions) {
  const iterationCount = useRef(0);
  const callHistory = useRef<string[]>([]);
  const lastProcessedMessageId = useRef<string | null>(null);

  /**
   * Reset tool execution state (call when starting new user message)
   */
  const resetToolState = useCallback(() => {
    iterationCount.current = 0;
    callHistory.current = [];
    console.log('[useToolExecution] Reset tool iteration counter and history');
  }, []);

  /**
   * Check if we should stop tool execution (limits, loops, context size)
   */
  const shouldStopExecution = useCallback((toolCalls: ToolCall[]): boolean => {
    // Check context size
    if (typeof maxTokens === 'number' && maxTokens > 0 && maxTokens < 4096) {
      console.error(`[useToolExecution] Context size too small (${maxTokens} tokens)`);
      toast.error(
        `Context size is too small (${maxTokens} tokens) for tool calling. Please reduce GPU layers or unload the model.`,
        { duration: 10000 }
      );
      setIsLoading(false);
      return true;
    }

    // Check iteration limit
    if (iterationCount.current >= MAX_TOOL_ITERATIONS) {
      console.warn(`[useToolExecution] MAX_TOOL_ITERATIONS (${MAX_TOOL_ITERATIONS}) reached`);
      toast.error(`Maximum tool iterations (${MAX_TOOL_ITERATIONS}) reached. Stopping to prevent infinite loop.`, { duration: 5000 });
      setIsLoading(false);
      return true;
    }

    // Check for infinite loop
    const toolCallSignatures = toolCalls.map(tc => `${tc.name}(${JSON.stringify(tc.arguments)})`).join('|');
    callHistory.current.push(toolCallSignatures);

    if (callHistory.current.length >= LOOP_DETECTION_WINDOW) {
      const recentCalls = callHistory.current.slice(-LOOP_DETECTION_WINDOW);
      const allIdentical = recentCalls.every(call => call === recentCalls[0]);

      if (allIdentical) {
        console.error(`[useToolExecution] INFINITE LOOP DETECTED: Same tool call repeated ${LOOP_DETECTION_WINDOW} times`);
        toast.error(`Infinite loop detected! The model is repeating the same tool call. Stopping.`, { duration: 7000 });
        setIsLoading(false);
        return true;
      }
    }

    return false;
  }, [maxTokens, setIsLoading]);

  /**
   * Process and execute tool calls
   */
  const processToolCalls = useCallback(async (
    toolCalls: ToolCall[],
    lastMessage: Message
  ): Promise<void> => {
    if (shouldStopExecution(toolCalls)) return;

    // Increment iteration counter
    iterationCount.current += 1;
    console.log(`[useToolExecution] Tool iteration: ${iterationCount.current}/${MAX_TOOL_ITERATIONS}`);

    // Mark message as processed
    lastProcessedMessageId.current = lastMessage.id;

    toast.success(
      `Executing ${toolCalls.length} tool call(s)... (iteration ${iterationCount.current}/${MAX_TOOL_ITERATIONS})`,
      { duration: 2000 }
    );

    try {
      const results = await Promise.all(toolCalls.map(executeTool));
      const formattedResults = formatToolResults(results);

      // Turn off loading before sending results
      setIsLoading(false);

      // Continue generation with tool results
      setTimeout(() => {
        sendMessage(formattedResults, true);
      }, 10);
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Tool execution failed';
      toast.error(`Tool error: ${errorMessage}`, { duration: 5000 });
      setIsLoading(false);
    }
  }, [shouldStopExecution, sendMessage, setIsLoading]);

  /**
   * Check if a message has already been processed
   */
  const isMessageProcessed = useCallback((messageId: string): boolean => {
    return lastProcessedMessageId.current === messageId;
  }, []);

  const state: ToolExecutionState = {
    iterationCount,
    callHistory,
    lastProcessedMessageId,
  };

  return {
    resetToolState,
    processToolCalls,
    isMessageProcessed,
    shouldStopExecution,
    state,
  };
}
