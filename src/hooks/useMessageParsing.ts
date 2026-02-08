import { useMemo } from 'react';
import { autoParseToolCalls, stripToolCalls } from '../utils/toolParser';
import type { Message, ToolCall } from '../types';

export interface SystemExecBlock {
  command: string;
  output: string | null;
}

export type MessageSegment =
  | { type: 'text'; content: string }
  | { type: 'command'; command: string; output: string | null }
  | { type: 'tool_call'; toolCall: ToolCall };

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
 * - Ordered segments (text + commands + tool calls interleaved chronologically)
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

  // Strip tool call markers and tool response tags from content
  const cleanContent = useMemo(() => {
    let content = message.content;
    if (toolCalls.length > 0) {
      content = stripToolCalls(content);
    } else {
      // Always strip tool_response tags even if no tool calls parsed
      content = content.replace(/<tool_response>[\s\S]*?<\/tool_response>/g, '');
    }
    return content;
  }, [message.content, toolCalls.length]);

  // Extract thinking content (for reasoning models like Qwen3)
  const thinkingContent = useMemo(() => {
    const thinkMatch = message.content.match(/<think>([\s\S]*?)<\/think>/);
    return thinkMatch ? thinkMatch[1].trim() : null;
  }, [message.content]);

  // Extract command execution blocks (legacy SYSTEM.EXEC format only)
  const systemExecBlocks = useMemo(() => {
    const blocks: SystemExecBlock[] = [];
    const execRegex =
      /(?:<\|\|)?SYSTEM\.EXEC>([\s\S]*?)<SYSTEM\.EXEC\|\|>/g;
    const outputRegex =
      /(?:<\|\|)?SYSTEM\.OUTPUT>([\s\S]*?)<SYSTEM\.OUTPUT\|\|>/g;

    let match;
    while ((match = execRegex.exec(message.content)) !== null) {
      const command = (match[1] || '').trim();
      blocks.push({ command, output: null });
    }

    let outputIndex = 0;
    while ((match = outputRegex.exec(message.content)) !== null) {
      if (outputIndex < blocks.length) {
        const output = (match[1] || '').trim();
        blocks[outputIndex].output = output;
        outputIndex++;
      }
    }

    return blocks;
  }, [message.content]);

  // Build ordered segments: interleave text, SYSTEM.EXEC commands, and tool calls chronologically
  const segments = useMemo(() => {
    // Work on raw content (before stripping) to find positions of all special blocks
    let content = message.content;

    // Remove thinking tags first
    content = content.replace(/<think>[\s\S]*?<\/think>/g, '');

    // Collect all spans (things to extract from the text flow)
    const spans: {
      start: number;
      end: number;
      segment: MessageSegment;
    }[] = [];

    // 1. Find SYSTEM.EXEC + SYSTEM.OUTPUT pairs (legacy format)
    const execRegex =
      /(?:<\|\|)?SYSTEM\.EXEC>([\s\S]*?)<SYSTEM\.EXEC\|\|>/g;
    const sysOutputRegex =
      /(?:<\|\|)?SYSTEM\.OUTPUT>([\s\S]*?)<SYSTEM\.OUTPUT\|\|>/g;

    const execMatches: { start: number; end: number; command: string }[] = [];
    let match;
    while ((match = execRegex.exec(content)) !== null) {
      execMatches.push({
        start: match.index,
        end: match.index + match[0].length,
        command: (match[1] || '').trim(),
      });
    }

    const sysOutputMatches: { start: number; end: number; output: string }[] = [];
    while ((match = sysOutputRegex.exec(content)) !== null) {
      sysOutputMatches.push({
        start: match.index,
        end: match.index + match[0].length,
        output: (match[1] || '').trim(),
      });
    }

    for (let i = 0; i < execMatches.length; i++) {
      const exec = execMatches[i];
      const output = i < sysOutputMatches.length ? sysOutputMatches[i] : null;
      spans.push({
        start: exec.start,
        end: output ? output.end : exec.end,
        segment: { type: 'command', command: exec.command, output: output ? output.output : null },
      });
    }

    // 2. Find tool_call + tool_response pairs (Qwen/native format)
    const toolCallRegex = /<tool_call>([\s\S]*?)<\/tool_call>/g;
    const toolResponseRegex = /<tool_response>([\s\S]*?)<\/tool_response>/g;

    const tcMatches: { start: number; end: number; json: string }[] = [];
    while ((match = toolCallRegex.exec(content)) !== null) {
      tcMatches.push({
        start: match.index,
        end: match.index + match[0].length,
        json: match[1].trim(),
      });
    }

    const trMatches: { start: number; end: number }[] = [];
    while ((match = toolResponseRegex.exec(content)) !== null) {
      trMatches.push({
        start: match.index,
        end: match.index + match[0].length,
      });
    }

    for (let i = 0; i < tcMatches.length; i++) {
      const tc = tcMatches[i];
      // Find the next tool_response that comes after this tool_call
      const tr = trMatches.find(r => r.start > tc.end);
      if (tr) {
        // Remove this response from the pool so it's not reused
        const trIdx = trMatches.indexOf(tr);
        trMatches.splice(trIdx, 1);
      }

      try {
        const parsed = JSON.parse(tc.json);
        spans.push({
          start: tc.start,
          end: tr ? tr.end : tc.end,
          segment: {
            type: 'tool_call',
            toolCall: {
              id: crypto.randomUUID(),
              name: parsed.name,
              arguments: parsed.arguments || {},
            },
          },
        });
      } catch {
        // If JSON parse fails, skip this tool call
      }
    }

    // Sort all spans by position
    spans.sort((a, b) => a.start - b.start);

    // Build segments by splitting content around spans
    const result: MessageSegment[] = [];
    let cursor = 0;

    for (const span of spans) {
      // Text before this span
      if (span.start > cursor) {
        const text = content.slice(cursor, span.start).trim();
        if (text) {
          result.push({ type: 'text', content: text });
        }
      }
      result.push(span.segment);
      cursor = span.end;
    }

    // Remaining text after last span
    if (cursor < content.length) {
      const text = content.slice(cursor).trim();
      if (text) {
        result.push({ type: 'text', content: text });
      }
    }

    return result;
  }, [message.content]);

  // Get content without thinking tags and all execution tags
  const contentWithoutThinking = useMemo(() => {
    let content = cleanContent;
    content = content.replace(/<think>[\s\S]*?<\/think>/g, '');
    content = content.replace(
      /(?:<\|\|)?SYSTEM\.EXEC>[\s\S]*?<SYSTEM\.EXEC\|\|>/g,
      ''
    );
    content = content.replace(/(?:<\|\|)?SYSTEM\.OUTPUT>[\s\S]*?<SYSTEM\.OUTPUT\|\|>/g, '');
    content = content.replace(/<tool_response>[\s\S]*?<\/tool_response>/g, '');
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
