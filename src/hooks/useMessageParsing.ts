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

  // Extract command execution blocks (supports all model-specific tag formats)
  const systemExecBlocks = useMemo(() => {
    const blocks: SystemExecBlock[] = [];
    // Match ALL known exec tag formats:
    // 1. Default SYSTEM.EXEC: <||SYSTEM.EXEC>cmd<SYSTEM.EXEC||>
    // 2. Qwen: <tool_call>cmd</tool_call>
    // 3. Mistral: [TOOL_CALLS]cmd[/TOOL_CALLS]
    // Also tolerate malformed variants (missing <|| prefix, stray [TOOL_CALLS] prefix)
    const execRegex =
      /(?:\[TOOL_CALLS\]\s*)?(?:<\|\|)?SYSTEM\.EXEC>([\s\S]*?)<SYSTEM\.EXEC\|\|>|<tool_call>([\s\S]*?)<\/tool_call>|\[TOOL_CALLS\]([\s\S]*?)\[\/TOOL_CALLS\]/g;
    const outputRegex =
      /(?:<\|\|)?SYSTEM\.OUTPUT>([\s\S]*?)<SYSTEM\.OUTPUT\|\|>|<tool_response>([\s\S]*?)<\/tool_response>|\[TOOL_RESULTS\]([\s\S]*?)\[\/TOOL_RESULTS\]/g;

    let match;
    while ((match = execRegex.exec(message.content)) !== null) {
      // One of the capture groups will have the command (others are undefined)
      const command = (match[1] || match[2] || match[3] || '').trim();
      blocks.push({ command, output: null });
    }

    // Match outputs to commands (in order)
    let outputIndex = 0;
    while ((match = outputRegex.exec(message.content)) !== null) {
      if (outputIndex < blocks.length) {
        const output = (match[1] || match[2] || match[3] || '').trim();
        blocks[outputIndex].output = output;
        outputIndex++;
      }
    }

    return blocks;
  }, [message.content]);

  // Get content without thinking tags and all command execution tags
  const contentWithoutThinking = useMemo(() => {
    let content = cleanContent;
    content = content.replace(/<think>[\s\S]*?<\/think>/g, '');
    // Strip all exec tag formats
    content = content.replace(
      /(?:\[TOOL_CALLS\]\s*)?(?:<\|\|)?SYSTEM\.EXEC>[\s\S]*?<SYSTEM\.EXEC\|\|>/g,
      ''
    );
    content = content.replace(/<tool_call>[\s\S]*?<\/tool_call>/g, '');
    content = content.replace(/\[TOOL_CALLS\][\s\S]*?\[\/TOOL_CALLS\]/g, '');
    // Strip all output tag formats
    content = content.replace(/(?:<\|\|)?SYSTEM\.OUTPUT>[\s\S]*?<SYSTEM\.OUTPUT\|\|>/g, '');
    content = content.replace(/<tool_response>[\s\S]*?<\/tool_response>/g, '');
    content = content.replace(/\[TOOL_RESULTS\][\s\S]*?\[\/TOOL_RESULTS\]/g, '');
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
