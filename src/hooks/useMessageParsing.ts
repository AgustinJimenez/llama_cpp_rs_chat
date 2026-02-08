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

type Span = { start: number; end: number; segment: MessageSegment };

const EXEC_REGEX = /(?:<\|\|)?SYSTEM\.EXEC>([\s\S]*?)<SYSTEM\.EXEC\|\|>/g;
const SYS_OUTPUT_REGEX = /(?:<\|\|)?SYSTEM\.OUTPUT>([\s\S]*?)<SYSTEM\.OUTPUT\|\|>/g;
const TOOL_CALL_REGEX = /<tool_call>([\s\S]*?)<\/tool_call>/g;
const TOOL_RESPONSE_REGEX = /<tool_response>([\s\S]*?)<\/tool_response>/g;
const THINKING_REGEX = /<think>[\s\S]*?<\/think>/g;

function collectExecSpans(content: string): Span[] {
  const spans: Span[] = [];
  const execMatches: { start: number; end: number; command: string }[] = [];
  let match;

  while ((match = EXEC_REGEX.exec(content)) !== null) {
    execMatches.push({ start: match.index, end: match.index + match[0].length, command: (match[1] || '').trim() });
  }

  const outputMatches: { start: number; end: number; output: string }[] = [];
  while ((match = SYS_OUTPUT_REGEX.exec(content)) !== null) {
    outputMatches.push({ start: match.index, end: match.index + match[0].length, output: (match[1] || '').trim() });
  }

  for (let i = 0; i < execMatches.length; i++) {
    const exec = execMatches[i];
    const output = i < outputMatches.length ? outputMatches[i] : null;
    spans.push({
      start: exec.start,
      end: output ? output.end : exec.end,
      segment: { type: 'command', command: exec.command, output: output ? output.output : null },
    });
  }

  return spans;
}

function collectToolCallSpans(content: string): Span[] {
  const spans: Span[] = [];
  let match;

  const tcMatches: { start: number; end: number; json: string }[] = [];
  while ((match = TOOL_CALL_REGEX.exec(content)) !== null) {
    tcMatches.push({ start: match.index, end: match.index + match[0].length, json: match[1].trim() });
  }

  const trMatches: { start: number; end: number }[] = [];
  while ((match = TOOL_RESPONSE_REGEX.exec(content)) !== null) {
    trMatches.push({ start: match.index, end: match.index + match[0].length });
  }

  for (const tc of tcMatches) {
    const tr = trMatches.find(r => r.start > tc.end);
    if (tr) trMatches.splice(trMatches.indexOf(tr), 1);

    try {
      const parsed = JSON.parse(tc.json);
      spans.push({
        start: tc.start,
        end: tr ? tr.end : tc.end,
        segment: {
          type: 'tool_call',
          toolCall: { id: crypto.randomUUID(), name: parsed.name, arguments: parsed.arguments || {} },
        },
      });
    } catch {
      // Skip unparseable tool calls
    }
  }

  return spans;
}

function buildSegments(content: string): MessageSegment[] {
  const cleaned = content.replace(THINKING_REGEX, '');
  const spans = [...collectExecSpans(cleaned), ...collectToolCallSpans(cleaned)]
    .sort((a, b) => a.start - b.start);

  const result: MessageSegment[] = [];
  let cursor = 0;

  for (const span of spans) {
    if (span.start > cursor) {
      const text = cleaned.slice(cursor, span.start).trim();
      if (text) result.push({ type: 'text', content: text });
    }
    result.push(span.segment);
    cursor = span.end;
  }

  if (cursor < cleaned.length) {
    const text = cleaned.slice(cursor).trim();
    if (text) result.push({ type: 'text', content: text });
  }

  return result;
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
  const toolCalls = useMemo(() => {
    if (message.role === 'assistant') return autoParseToolCalls(message.content);
    return [];
  }, [message.content, message.role]);

  const cleanContent = useMemo(() => {
    let content = message.content;
    if (toolCalls.length > 0) {
      content = stripToolCalls(content);
    } else {
      content = content.replace(/<tool_response>[\s\S]*?<\/tool_response>/g, '');
    }
    return content;
  }, [message.content, toolCalls.length]);

  const thinkingContent = useMemo(() => {
    const thinkMatch = message.content.match(/<think>([\s\S]*?)<\/think>/);
    return thinkMatch ? thinkMatch[1].trim() : null;
  }, [message.content]);

  const systemExecBlocks = useMemo(() => {
    const blocks: SystemExecBlock[] = [];
    let match;

    while ((match = EXEC_REGEX.exec(message.content)) !== null) {
      blocks.push({ command: (match[1] || '').trim(), output: null });
    }

    let outputIndex = 0;
    while ((match = SYS_OUTPUT_REGEX.exec(message.content)) !== null) {
      if (outputIndex < blocks.length) {
        blocks[outputIndex].output = (match[1] || '').trim();
        outputIndex++;
      }
    }

    return blocks;
  }, [message.content]);

  const segments = useMemo(() => buildSegments(message.content), [message.content]);

  const contentWithoutThinking = useMemo(() => {
    return cleanContent
      .replace(THINKING_REGEX, '')
      .replace(EXEC_REGEX, '')
      .replace(SYS_OUTPUT_REGEX, '')
      .replace(/<tool_response>[\s\S]*?<\/tool_response>/g, '')
      .trim();
  }, [cleanContent]);

  const isError = message.role === 'system' && (
    message.content.includes('\u274C') ||
    message.content.includes('Generation Crashed') ||
    message.content.includes('Error')
  );

  return { toolCalls, cleanContent, thinkingContent, systemExecBlocks, contentWithoutThinking, segments, isError };
}
