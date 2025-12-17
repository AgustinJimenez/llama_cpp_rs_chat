import { useMemo } from 'react';
import { autoParseToolCalls, stripToolCalls } from '../utils/toolParser';
import type { Message, ToolCall } from '../types';

export interface SystemExecBlock {
  command: string;
  output: string | null;
}

export interface ParsedMessage {
  toolCalls: ToolCall[];
  cleanContent: string;
  thinkingContent: string | null;
  systemExecBlocks: SystemExecBlock[];
  contentWithoutThinking: string;
  isError: boolean;
}

/**
 * Parse a message and extract various components:
 * - Tool calls
 * - Thinking content (for reasoning models)
 * - SYSTEM.EXEC blocks (command executions)
 * - Clean content without special tags
 */
export function useMessageParsing(message: Message): ParsedMessage {
  // Parse tool calls from assistant messages
  const toolCalls = useMemo(() => {
    if (message.role === 'assistant') {
      return autoParseToolCalls(message.content);
    }
    return [];
  }, [message.content, message.role]);

  // Strip tool call markers from content
  const cleanContent = useMemo(() => {
    let content = message.content;
    if (toolCalls.length > 0) {
      content = stripToolCalls(content);
    }
    return content;
  }, [message.content, toolCalls.length]);

  // Extract thinking content (for reasoning models like Qwen3)
  const thinkingContent = useMemo(() => {
    const thinkMatch = message.content.match(/<think>([\s\S]*?)<\/think>/);
    return thinkMatch ? thinkMatch[1].trim() : null;
  }, [message.content]);

  // Extract SYSTEM.EXEC blocks (command executions)
  const systemExecBlocks = useMemo(() => {
    const blocks: SystemExecBlock[] = [];
    // Accept both canonical tags (<||SYSTEM.EXEC>...<SYSTEM.EXEC||>)
    // and the common malformed variant (SYSTEM.EXEC>...<SYSTEM.EXEC||>) sometimes emitted by models.
    // Also tolerate a stray [TOOL_CALLS] prefix before SYSTEM.EXEC.
    const execRegex =
      /(?:\[TOOL_CALLS\]\s*)?(?:<\|\|)?SYSTEM\.EXEC>([\s\S]*?)<SYSTEM\.EXEC\|\|>/g;
    const outputRegex = /(?:<\|\|)?SYSTEM\.OUTPUT>([\s\S]*?)<SYSTEM\.OUTPUT\|\|>/g;

    let match;
    while ((match = execRegex.exec(message.content)) !== null) {
      blocks.push({ command: match[1].trim(), output: null });
    }

    // Match outputs to commands (in order)
    let outputIndex = 0;
    while ((match = outputRegex.exec(message.content)) !== null) {
      if (outputIndex < blocks.length) {
        blocks[outputIndex].output = match[1].trim();
        outputIndex++;
      }
    }

    return blocks;
  }, [message.content]);

  // Get content without thinking tags and SYSTEM.EXEC/OUTPUT tags
  const contentWithoutThinking = useMemo(() => {
    let content = cleanContent;
    content = content.replace(/<think>[\s\S]*?<\/think>/g, '');
    content = content.replace(
      /(?:\[TOOL_CALLS\]\s*)?(?:<\|\|)?SYSTEM\.EXEC>[\s\S]*?<SYSTEM\.EXEC\|\|>/g,
      ''
    );
    content = content.replace(/(?:<\|\|)?SYSTEM\.OUTPUT>[\s\S]*?<SYSTEM\.OUTPUT\|\|>/g, '');
    return content.trim();
  }, [cleanContent]);

  // Detect error messages
  const isError = message.role === 'system' && (
    message.content.includes('❌') ||
    message.content.includes('Generation Crashed') ||
    message.content.includes('Error')
  );

  return {
    toolCalls,
    cleanContent,
    thinkingContent,
    systemExecBlocks,
    contentWithoutThinking,
    isError,
  };
}
