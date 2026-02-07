import { useMemo } from 'react';
import { autoParseToolCalls, stripToolCalls } from '../utils/toolParser';
import type { Message, ToolCall } from '../types';

export interface SystemExecBlock {
  command: string;
  output: string | null;
}

export type MessageSegment =
  | { type: 'text'; content: string }
  | { type: 'command'; command: string; output: string | null };

export interface ParsedMessage {
  toolCalls: ToolCall[];
  cleanContent: string;
  thinkingContent: string | null;
  systemExecBlocks: SystemExecBlock[];
  contentWithoutThinking: string;
  segments: MessageSegment[];
  isError: boolean;
}

/**
 * Parse a message and extract various components:
 * - Tool calls
 * - Thinking content (for reasoning models)
 * - SYSTEM.EXEC blocks (command executions)
 * - Ordered segments (text + commands interleaved chronologically)
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
    const execRegex =
      /(?:\[TOOL_CALLS\]\s*)?(?:<\|\|)?SYSTEM\.EXEC>([\s\S]*?)<SYSTEM\.EXEC\|\|>|<tool_call>([\s\S]*?)<\/tool_call>|\[TOOL_CALLS\]([\s\S]*?)\[\/TOOL_CALLS\]/g;
    const outputRegex =
      /(?:<\|\|)?SYSTEM\.OUTPUT>([\s\S]*?)<SYSTEM\.OUTPUT\|\|>|<tool_response>([\s\S]*?)<\/tool_response>|\[TOOL_RESULTS\]([\s\S]*?)\[\/TOOL_RESULTS\]/g;

    let match;
    while ((match = execRegex.exec(message.content)) !== null) {
      const command = (match[1] || match[2] || match[3] || '').trim();
      blocks.push({ command, output: null });
    }

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

  // Build ordered segments: interleave text and command blocks chronologically
  const segments = useMemo(() => {
    // Start from cleanContent (tool calls stripped) and remove thinking tags
    let content = cleanContent.replace(/<think>[\s\S]*?<\/think>/g, '');

    // Collect all exec+output block spans with their positions
    const spans: { start: number; end: number; command: string; output: string | null }[] = [];

    const execRegex =
      /(?:\[TOOL_CALLS\]\s*)?(?:<\|\|)?SYSTEM\.EXEC>([\s\S]*?)<SYSTEM\.EXEC\|\|>|<tool_call>([\s\S]*?)<\/tool_call>|\[TOOL_CALLS\]([\s\S]*?)\[\/TOOL_CALLS\]/g;
    const outputRegex =
      /(?:<\|\|)?SYSTEM\.OUTPUT>([\s\S]*?)<SYSTEM\.OUTPUT\|\|>|<tool_response>([\s\S]*?)<\/tool_response>|\[TOOL_RESULTS\]([\s\S]*?)\[\/TOOL_RESULTS\]/g;

    // Find all exec blocks with positions
    const execMatches: { start: number; end: number; command: string }[] = [];
    let match;
    while ((match = execRegex.exec(content)) !== null) {
      execMatches.push({
        start: match.index,
        end: match.index + match[0].length,
        command: (match[1] || match[2] || match[3] || '').trim(),
      });
    }

    // Find all output blocks with positions
    const outputMatches: { start: number; end: number; output: string }[] = [];
    while ((match = outputRegex.exec(content)) !== null) {
      outputMatches.push({
        start: match.index,
        end: match.index + match[0].length,
        output: (match[1] || match[2] || match[3] || '').trim(),
      });
    }

    // Pair exec blocks with their corresponding output blocks (by order)
    for (let i = 0; i < execMatches.length; i++) {
      const exec = execMatches[i];
      const output = i < outputMatches.length ? outputMatches[i] : null;
      spans.push({
        start: exec.start,
        // If there's a paired output, the span extends to end of output
        end: output ? output.end : exec.end,
        command: exec.command,
        output: output ? output.output : null,
      });
    }

    // Sort spans by position (should already be in order, but be safe)
    spans.sort((a, b) => a.start - b.start);

    // Build segments by splitting content around command spans
    const result: MessageSegment[] = [];
    let cursor = 0;

    for (const span of spans) {
      // Text before this command
      if (span.start > cursor) {
        const text = content.slice(cursor, span.start).trim();
        if (text) {
          result.push({ type: 'text', content: text });
        }
      }
      // The command block
      result.push({ type: 'command', command: span.command, output: span.output });
      cursor = span.end;
    }

    // Remaining text after last command
    if (cursor < content.length) {
      const text = content.slice(cursor).trim();
      if (text) {
        result.push({ type: 'text', content: text });
      }
    }

    return result;
  }, [cleanContent]);

  // Get content without thinking tags and all command execution tags (kept for backward compat)
  const contentWithoutThinking = useMemo(() => {
    let content = cleanContent;
    content = content.replace(/<think>[\s\S]*?<\/think>/g, '');
    content = content.replace(
      /(?:\[TOOL_CALLS\]\s*)?(?:<\|\|)?SYSTEM\.EXEC>[\s\S]*?<SYSTEM\.EXEC\|\|>/g,
      ''
    );
    content = content.replace(/<tool_call>[\s\S]*?<\/tool_call>/g, '');
    content = content.replace(/\[TOOL_CALLS\][\s\S]*?\[\/TOOL_CALLS\]/g, '');
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
    segments,
    isError,
  };
}
