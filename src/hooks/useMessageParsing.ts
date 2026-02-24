import { useMemo } from 'react';
import { autoParseToolCalls, stripToolCalls } from '../utils/toolParser';
import { collectLlama3Spans, collectMistralSpans } from '../utils/toolSpanCollectors';
import type { Message, ToolCall } from '../types';

export type MessageSegment =
  | { type: 'text'; content: string }
  | { type: 'tool_call'; toolCall: ToolCall }
  | { type: 'thinking'; content: string };

export interface ParsedMessage {
  toolCalls: ToolCall[];
  cleanContent: string;
  thinkingContent: string | null;
  isThinkingStreaming: boolean;
  contentWithoutThinking: string;
  segments: MessageSegment[];
  isError: boolean;
}

type Span = { start: number; end: number; segment: MessageSegment };

const EXEC_REGEX = /(?:<\|\|)?SYSTEM\.EXEC>([\s\S]*?)<SYSTEM\.EXEC\|\|>/g;
const SYS_OUTPUT_REGEX = /(?:<\|\|)?SYSTEM\.OUTPUT>([\s\S]*?)<SYSTEM\.OUTPUT\|\|>/g;
const TOOL_CALL_REGEX = /<tool_call>([\s\S]*?)<\/tool_call>/g;
const TOOL_RESPONSE_REGEX = /<tool_response>([\s\S]*?)<\/tool_response>/g;
// Mistral format cleanup regexes
const MISTRAL_CALL_REGEX = /\[TOOL_CALLS\][\s\S]*?\[\/TOOL_CALLS\]/g;
const MISTRAL_RESULT_REGEX = /\[TOOL_RESULTS\][\s\S]*?\[\/TOOL_RESULTS\]/g;
const THINKING_REGEX = /<think>[\s\S]*?<\/think>/g;
// Also match an unclosed <think> tag (streaming in progress)
const THINKING_UNCLOSED_REGEX = /<think>([\s\S]*)$/;

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
const HARMONY_SEGMENT_PATTERN = /<\|start\|>([\s\S]*?)(?=<\|start\|>|$)/g;
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
    const { name, args } = parseExecCommand(acc.pendingCommand);
    acc.segments.push({ type: 'tool_call', toolCall: {
      id: crypto.randomUUID(), name, arguments: args, output: outputText,
    } });
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

  // Create local regex each call to avoid shared lastIndex state corruption
  const segmentRegex = new RegExp(HARMONY_SEGMENT_PATTERN.source, 'g');
  let match;
  while ((match = segmentRegex.exec(raw)) !== null) {
    const body = match[1];
    if (body.startsWith('tool<|message|>')) {
      harmonyProcessToolOutput(acc, body);
    } else if (body.startsWith('assistant<|channel|>')) {
      harmonyProcessAssistant(acc, body);
    }
  }

  // Flush pending command with no output
  if (acc.pendingCommand) {
    const { name: pName, args: pArgs } = parseExecCommand(acc.pendingCommand);
    acc.segments.push({ type: 'tool_call', toolCall: {
      id: crypto.randomUUID(), name: pName, arguments: pArgs, isPending: true,
    } });
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

/** Convert a raw command string to a ToolCall-compatible name + arguments. */
function parseExecCommand(command: string): { name: string; args: Record<string, unknown> } {
  // JSON format: {"name":"tool","arguments":{...}}
  try {
    const parsed = JSON.parse(command);
    if (parsed.name) return { name: parsed.name, args: parsed.arguments || {} };
  } catch { /* not JSON */ }
  // Function call: tool_name({"arg":"val"})
  const funcMatch = command.match(/^(\w+)\((\{[\s\S]*\})\)$/);
  if (funcMatch) {
    try {
      return { name: funcMatch[1], args: JSON.parse(funcMatch[2]) };
    } catch { /* fall through */ }
  }
  // Raw shell command
  return { name: 'execute_command', args: { command } };
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
    const { name, args } = parseExecCommand(exec.command);
    const output = i < outputMatches.length ? outputMatches[i] : null;
    if (output) {
      spans.push({
        start: exec.start, end: output.end,
        segment: { type: 'tool_call', toolCall: {
          id: crypto.randomUUID(), name, arguments: args,
          output: output.output,
        } },
      });
    } else {
      const partialMatch = content.match(/(?:<\|\|)?SYSTEM\.OUTPUT>([\s\S]*)$/);
      const lastCompleteEnd = content.lastIndexOf('<SYSTEM.OUTPUT||>');
      const partialStart = content.lastIndexOf('SYSTEM.OUTPUT>');
      if (partialMatch && partialStart > lastCompleteEnd) {
        spans.push({
          start: exec.start, end: content.length,
          segment: { type: 'tool_call', toolCall: {
            id: crypto.randomUUID(), name, arguments: args,
            output: partialMatch[1] || '', isStreaming: true, isPending: true,
          } },
        });
      } else {
        spans.push({
          start: exec.start, end: exec.end,
          segment: { type: 'tool_call', toolCall: {
            id: crypto.randomUUID(), name, arguments: args, isPending: true,
          } },
        });
      }
    }
  }

  return spans;
}

/** Check if there's an unclosed <tool_response> after a given position in the content. */
function findStreamingResponse(content: string, afterPos: number): { output: string; end: number } | null {
  const afterTc = content.slice(afterPos);
  const partialTrMatch = afterTc.match(/^[\s\S]*?<tool_response>([\s\S]*)$/);
  if (!partialTrMatch) return null;

  const lastCompleteTrEnd = content.lastIndexOf('</tool_response>');
  const partialTrStart = content.lastIndexOf('<tool_response>');
  if (partialTrStart <= lastCompleteTrEnd) return null;

  return { output: partialTrMatch[1] || '', end: content.length };
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

  // Pair tool calls with their responses
  type TcMatch = typeof tcMatches[number];
  type TrMatch = typeof trMatches[number];
  const paired: { tc: TcMatch; tr: TrMatch | null }[] = [];

  for (const tc of tcMatches) {
    const tr = trMatches.find(r => r.start > tc.end);
    if (tr) trMatches.splice(trMatches.indexOf(tr), 1);
    paired.push({ tc, tr: tr || null });
  }

  // Only the LAST unmatched tool call can claim an unclosed streaming response
  const lastUnmatchedIdx = paired.reduce(
    (acc, p, i) => (p.tr === null ? i : acc), -1
  );

  for (let i = 0; i < paired.length; i++) {
    const { tc, tr } = paired[i];
    try {
      const parsed = JSON.parse(tc.json);

      const isLastUnmatched = !tr && i === lastUnmatchedIdx;
      let output: string | undefined = tr ? tr.content : undefined;
      let isStreaming = false;
      let spanEnd = tr ? tr.end : tc.end;

      if (isLastUnmatched) {
        const streaming = findStreamingResponse(content, tc.end);
        if (streaming) {
          output = streaming.output;
          isStreaming = true;
          spanEnd = streaming.end;
        }
      }

      spans.push({
        start: tc.start,
        end: spanEnd,
        segment: {
          type: 'tool_call',
          toolCall: {
            id: crypto.randomUUID(),
            name: parsed.name,
            arguments: parsed.arguments || {},
            output,
            isStreaming,
            isPending: isLastUnmatched,
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
  const cleaned = content.replace(THINKING_REGEX, '').replace(THINKING_UNCLOSED_REGEX, '');
  const toolCallSpans = collectToolCallSpans(cleaned);
  // Try format-specific collectors in priority order (first non-empty wins)
  const mistralSpans = toolCallSpans.length > 0 ? [] : collectMistralSpans(cleaned);
  const toolSpans = toolCallSpans.length > 0 ? toolCallSpans
    : mistralSpans.length > 0 ? mistralSpans : collectLlama3Spans(cleaned);
  const spans = [...collectExecSpans(cleaned), ...toolSpans]
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
    // Match closed <think>...</think> first
    const thinkMatch = message.content.match(/<think>([\s\S]*?)<\/think>/);
    if (thinkMatch) return thinkMatch[1].trim();
    // Match unclosed <think>... (streaming in progress)
    const unclosedMatch = message.content.match(THINKING_UNCLOSED_REGEX);
    return unclosedMatch ? unclosedMatch[1].trim() || null : null;
  }, [message.content, harmony]);

  // True when thinking is actively streaming (unclosed <think> tag)
  const isThinkingStreaming = useMemo(() => {
    if (harmony || !thinkingContent) return false;
    return !/<think>[\s\S]*?<\/think>/.test(message.content);
  }, [message.content, harmony, thinkingContent]);

  const segments = useMemo(() => {
    // Harmony provides its own ordered segments (thinking + commands + final text interleaved)
    if (harmony) return harmony.segments;
    return buildSegments(effectiveContent);
  }, [effectiveContent, harmony]);

  const contentWithoutThinking = useMemo(() => {
    return cleanContent
      .replace(THINKING_REGEX, '')
      .replace(THINKING_UNCLOSED_REGEX, '')
      .replace(EXEC_REGEX, '')
      .replace(SYS_OUTPUT_REGEX, '')
      .replace(/<tool_response>[\s\S]*?<\/tool_response>/g, '')
      .replace(MISTRAL_CALL_REGEX, '')
      .replace(MISTRAL_RESULT_REGEX, '')
      .trim();
  }, [cleanContent]);

  const isError = message.role === 'system' && (
    message.content.includes('\u274C') ||
    message.content.includes('Generation Crashed') ||
    message.content.includes('Error')
  );

  return { toolCalls, cleanContent, thinkingContent, isThinkingStreaming, contentWithoutThinking, segments, isError };
}
