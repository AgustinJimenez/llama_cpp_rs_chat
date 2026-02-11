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
  | { type: 'tool_call'; toolCall: ToolCall }
  | { type: 'thinking'; content: string };

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
  /** Ordered segments: text, commands, and final text in chronological order */
  segments: MessageSegment[];
  /** Final user-facing content (for cleanContent/toolCalls extraction) */
  finalContent: string;
}

interface HarmonyAccumulator {
  segments: MessageSegment[];
  finalParts: string[];
  pendingCommand: string | null;
}

const HARMONY_DETECT = /<\|start\|>assistant<\|channel\|>/;
const HARMONY_SEGMENT_REGEX = /<\|start\|>([\s\S]*?)(?=<\|start\|>|$)/g;
const HARMONY_TOOL_CALL_REGEX = /to=\s*(\w+)\s+code<\|message\|>([\s\S]*?)<\|call\|>/;

/** Push a text segment, merging with previous text to avoid tiny blocks. */
function harmonyPushText(segments: MessageSegment[], text: string) {
  const trimmed = text.trim();
  if (!trimmed) return;
  const last = segments[segments.length - 1];
  if (last && last.type === 'text') {
    last.content += '\n\n' + trimmed;
  } else {
    segments.push({ type: 'text', content: trimmed });
  }
}

/** Extract bare reasoning text after the first <|end|> in a segment body. */
function harmonyExtractTrailing(segments: MessageSegment[], segBody: string) {
  const idx = segBody.indexOf('<|end|>');
  if (idx < 0) return;
  const trailing = segBody.slice(idx + 7).replace(/<\|end\|>/g, '').trim();
  if (trailing) harmonyPushText(segments, trailing);
}

/** Extract a human-readable command string from tool call JSON. */
function harmonyParseCommand(toolName: string, argsJson: string): string {
  try {
    const parsed = JSON.parse(argsJson);
    if (parsed.command) return parsed.command;
    if (parsed.path) return `${toolName}: ${parsed.path}`;
    return `${toolName}(${JSON.stringify(parsed)})`;
  } catch {
    return `${toolName}: ${argsJson}`;
  }
}

/** Process a tool output segment: <|start|>tool<|message|>...<|end|> */
function harmonyProcessToolOutput(acc: HarmonyAccumulator, body: string) {
  const outputText = body
    .replace(/^tool<\|message\|>/, '')
    .replace(/<\|end\|>[\s\S]*$/, '')
    .trim();

  if (acc.pendingCommand) {
    acc.segments.push({ type: 'command', command: acc.pendingCommand, output: outputText });
    acc.pendingCommand = null;
  }

  harmonyExtractTrailing(acc.segments, body);
}

/** Process an assistant channel segment (analysis/commentary/final). */
function harmonyProcessAssistant(acc: HarmonyAccumulator, body: string) {
  const channelBody = body.replace(/^assistant<\|channel\|>/, '');
  const channelMatch = channelBody.match(/^(\w+)/);
  const channel = channelMatch ? channelMatch[1] : '';

  if (channel === 'final') {
    const msgIdx = channelBody.indexOf('<|message|>');
    if (msgIdx >= 0) {
      const text = channelBody.slice(msgIdx + 11).replace(/<\|end\|>[\s\S]*$/, '').trim();
      if (text) acc.finalParts.push(text);
    }
  } else {
    const toolCallMatch = channelBody.match(HARMONY_TOOL_CALL_REGEX);

    if (toolCallMatch) {
      acc.pendingCommand = harmonyParseCommand(toolCallMatch[1], toolCallMatch[2].trim());

      // Post-<|call|> commentary (e.g. "commentary<|message|>Let's call it.")
      const postCall = channelBody.split('<|call|>').slice(1).join('<|call|>');
      const commentaryMatch = postCall.match(/<\|message\|>([\s\S]*?)(?:<\|end\|>|$)/);
      if (commentaryMatch) harmonyPushText(acc.segments, commentaryMatch[1]);
    } else {
      // Pure reasoning — extract message content
      const messageParts = channelBody.split('<|message|>');
      for (let i = 1; i < messageParts.length; i++) {
        const part = messageParts[i]
          .replace(/<\|call\|>[\s\S]*$/, '')
          .replace(/<\|end\|>[\s\S]*$/, '')
          .trim();
        if (part) harmonyPushText(acc.segments, part);
      }
    }
  }

  harmonyExtractTrailing(acc.segments, body);
}

/**
 * Parse Harmony-format model output (gpt-oss-20b).
 * Returns ordered segments preserving the chronological flow:
 *   text → command → text → command → ... → text (final)
 */
function parseHarmonyContent(raw: string): HarmonyParsed | null {
  if (!HARMONY_DETECT.test(raw)) return null;

  const acc: HarmonyAccumulator = { segments: [], finalParts: [], pendingCommand: null };

  // Bare text before the first <|start|>
  const firstStart = raw.indexOf('<|start|>');
  if (firstStart > 0) {
    harmonyPushText(acc.segments, raw.slice(0, firstStart).replace(/<\|end\|>/g, ''));
  }

  let match;
  while ((match = HARMONY_SEGMENT_REGEX.exec(raw)) !== null) {
    const body = match[1];
    if (body.startsWith('tool<|message|>')) {
      harmonyProcessToolOutput(acc, body);
    } else if (body.startsWith('assistant<|channel|>')) {
      harmonyProcessAssistant(acc, body);
    }
  }
  // Reset regex state for potential re-use
  HARMONY_SEGMENT_REGEX.lastIndex = 0;

  // Flush pending command with no output
  if (acc.pendingCommand) {
    acc.segments.push({ type: 'command', command: acc.pendingCommand, output: null });
  }

  // Trailing unterminated final segment
  if (acc.finalParts.length === 0) {
    const lastEnd = raw.lastIndexOf('<|end|>');
    if (lastEnd >= 0) {
      const trailer = raw.slice(lastEnd + 7).trim();
      const trailingFinal = trailer.match(/<\|start\|>assistant<\|channel\|>final<\|message\|>([\s\S]*)/);
      if (trailingFinal) acc.finalParts.push(trailingFinal[1].trim());
    }
  }

  const finalContent = acc.finalParts.join('\n') || raw.replace(/<\|[^|]*\|>/g, ' ').trim();
  if (finalContent.trim()) {
    acc.segments.push({ type: 'text', content: finalContent });
  }

  return { segments: acc.segments, finalContent };
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
    // Harmony uses inline thinking segments — no top-level thinking block
    if (harmony) return null;
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

  const segments = useMemo(() => {
    // Harmony provides its own ordered segments (thinking + commands + final text interleaved)
    if (harmony) return harmony.segments;
    return buildSegments(effectiveContent);
  }, [effectiveContent, harmony]);

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
