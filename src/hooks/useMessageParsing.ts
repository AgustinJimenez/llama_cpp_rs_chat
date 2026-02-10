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

// --- Harmony format parser (gpt-oss-20b) ---

interface HarmonyParsed {
  thinking: string;
  finalContent: string;
}

const HARMONY_SEGMENT_REGEX = /<\|start\|>assistant<\|channel\|>([\s\S]*?)<\|end\|>/g;
const HARMONY_DETECT = /<\|start\|>assistant<\|channel\|>/;

/**
 * Parse Harmony-format model output (gpt-oss-20b).
 * Segments are delimited by <|start|>assistant<|channel|>...<|end|>.
 * Channel types: analysis, commentary (→ thinking), final (→ display).
 * Returns null if content is not Harmony format.
 */
function parseHarmonyContent(raw: string): HarmonyParsed | null {
  if (!HARMONY_DETECT.test(raw)) return null;

  const thinkingParts: string[] = [];
  const finalParts: string[] = [];

  // Capture any bare text before the first <|start|> as thinking
  const firstStart = raw.indexOf('<|start|>');
  if (firstStart > 0) {
    const prefix = raw.slice(0, firstStart).replace(/<\|end\|>/g, '').trim();
    if (prefix) thinkingParts.push(prefix);
  }

  let match;
  while ((match = HARMONY_SEGMENT_REGEX.exec(raw)) !== null) {
    const body = match[1];

    // Extract channel name (first word after <|channel|>)
    // e.g. "analysis<|message|>..." or "analysis to=execute_command code<|message|>..."
    // or "final<|message|>..."
    const channelMatch = body.match(/^(\w+)/);
    const channel = channelMatch ? channelMatch[1] : '';

    // Extract <|message|> content (last one in the segment for the main text)
    // There can be multiple <|message|> in one segment (e.g. after <|call|>commentary<|message|>...)
    // We want all message content for thinking, or the final message content for final
    const messageParts = body.split('<|message|>');

    if (channel === 'final') {
      // Everything after the last <|message|> is the final response
      const lastPart = messageParts[messageParts.length - 1];
      if (lastPart) finalParts.push(lastPart.trim());
    } else {
      // analysis, commentary — collect message content as thinking
      for (let i = 1; i < messageParts.length; i++) {
        // Strip trailing <|call|> and anything after it (tool call markers within thinking)
        const part = messageParts[i].split('<|call|>')[0].trim();
        if (part) thinkingParts.push(part);
      }
    }
  }

  // Also capture any trailing text after the last <|end|> that's not in a segment
  const lastEnd = raw.lastIndexOf('<|end|>');
  if (lastEnd >= 0) {
    const trailer = raw.slice(lastEnd + 7).trim();
    // Check if trailer has a bare <|start|>...<|channel|>final<|message|> without <|end|>
    const trailingFinal = trailer.match(/<\|start\|>assistant<\|channel\|>final<\|message\|>([\s\S]*)/);
    if (trailingFinal) {
      finalParts.push(trailingFinal[1].trim());
    } else if (trailer) {
      finalParts.push(trailer);
    }
  }

  return {
    thinking: thinkingParts.join('\n'),
    finalContent: finalParts.join('\n') || raw.replace(/<\|[^|]*\|>/g, ' ').trim(),
  };
}

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

  const trMatches: { start: number; end: number; content: string }[] = [];
  while ((match = TOOL_RESPONSE_REGEX.exec(content)) !== null) {
    trMatches.push({ start: match.index, end: match.index + match[0].length, content: (match[1] || '').trim() });
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
          toolCall: {
            id: crypto.randomUUID(),
            name: parsed.name,
            arguments: parsed.arguments || {},
            output: tr ? tr.content : undefined,
          },
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
  // Try Harmony format first (gpt-oss-20b) — transforms content before other parsers run
  const harmony = useMemo(() => parseHarmonyContent(message.content), [message.content]);
  const effectiveContent = harmony ? harmony.finalContent : message.content;

  const toolCalls = useMemo(() => {
    if (message.role === 'assistant') return autoParseToolCalls(effectiveContent);
    return [];
  }, [effectiveContent, message.role]);

  const cleanContent = useMemo(() => {
    let content = effectiveContent;
    if (toolCalls.length > 0) {
      content = stripToolCalls(content);
    } else {
      content = content.replace(/<tool_response>[\s\S]*?<\/tool_response>/g, '');
    }
    return content;
  }, [effectiveContent, toolCalls.length]);

  const thinkingContent = useMemo(() => {
    // Harmony thinking takes priority over <think> blocks
    if (harmony && harmony.thinking) return harmony.thinking;
    const thinkMatch = message.content.match(/<think>([\s\S]*?)<\/think>/);
    return thinkMatch ? thinkMatch[1].trim() : null;
  }, [message.content, harmony]);

  const systemExecBlocks = useMemo(() => {
    const blocks: SystemExecBlock[] = [];
    let match;

    while ((match = EXEC_REGEX.exec(effectiveContent)) !== null) {
      blocks.push({ command: (match[1] || '').trim(), output: null });
    }

    let outputIndex = 0;
    while ((match = SYS_OUTPUT_REGEX.exec(effectiveContent)) !== null) {
      if (outputIndex < blocks.length) {
        blocks[outputIndex].output = (match[1] || '').trim();
        outputIndex++;
      }
    }

    return blocks;
  }, [effectiveContent]);

  const segments = useMemo(() => buildSegments(effectiveContent), [effectiveContent]);

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
