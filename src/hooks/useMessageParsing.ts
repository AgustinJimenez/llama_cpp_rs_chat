import { useMemo } from 'react';
import { autoParseToolCalls, stripToolCalls } from '../utils/toolParser';
import {
  buildSegments, parseExecCommand,
  THINKING_REGEX, THINKING_UNCLOSED_REGEX,
} from '../utils/toolSpanCollectors';
import type { Message, ToolCall } from '../types';

export type { MessageSegment } from '../utils/toolSpanCollectors';

export interface ParsedMessage {
  toolCalls: ToolCall[];
  cleanContent: string;
  thinkingContent: string | null;
  isThinkingStreaming: boolean;
  contentWithoutThinking: string;
  segments: MessageSegment[];
  isError: boolean;
}

// Import the type for local use (re-exported above for consumers)
import type { MessageSegment } from '../utils/toolSpanCollectors';

// Cleanup regexes — used only by contentWithoutThinking to strip all format tags
const EXEC_CLEANUP = /(?:<\|\|)?SYSTEM\.EXEC>[\s\S]*?<(?:\|\|)?SYSTEM\.EXEC\|\|>/g;
const SYS_OUTPUT_CLEANUP = /(?:<\|\|)?SYSTEM\.OUTPUT>[\s\S]*?<(?:\|\|)?SYSTEM\.OUTPUT\|\|>/g;
const MISTRAL_CALL_CLEANUP = /(?:\[TOOL_CALLS\][\s\S]*?\[\/TOOL_CALLS\]|\[TOOL_CALLS\]\w+\[ARGS\]\{[\s\S]*?\}|\[TOOL_CALLS\]\s*\{[^}]*"name"[^}]*"arguments"[\s\S]*?\}\s*\})/g;
const MISTRAL_RESULT_CLEANUP = /\[TOOL_RESULTS\][\s\S]*?\[\/TOOL_RESULTS\]/g;

// --- Harmony format parser (gpt-oss-20b) ---

interface HarmonyParsed {
  segments: MessageSegment[];
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

      const postCall = channelBody.split('<|call|>').slice(1).join('<|call|>');
      const commentaryMatch = postCall.match(/<\|message\|>([\s\S]*?)(?:<\|end\|>|$)/);
      if (commentaryMatch) harmonyPushText(acc.segments, commentaryMatch[1]);
    } else {
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

  const firstStart = raw.indexOf('<|start|>');
  if (firstStart > 0) {
    harmonyPushText(acc.segments, raw.slice(0, firstStart).replace(/<\|end\|>/g, ''));
  }

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

  if (acc.pendingCommand) {
    const { name: pName, args: pArgs } = parseExecCommand(acc.pendingCommand);
    acc.segments.push({ type: 'tool_call', toolCall: {
      id: crypto.randomUUID(), name: pName, arguments: pArgs, isPending: true,
    } });
  }

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

/**
 * Parse a message and extract various components:
 * - Tool calls
 * - Thinking content (for reasoning models)
 * - SYSTEM.EXEC blocks (command executions)
 * - Ordered segments (text + commands + tool calls interleaved chronologically)
 * - Clean content without special tags
 */
export function useMessageParsing(message: Message): ParsedMessage {
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
    if (harmony) return null;
    const thinkMatch = message.content.match(/<think>([\s\S]*?)<\/think>/);
    if (thinkMatch) return thinkMatch[1].trim();
    const unclosedMatch = message.content.match(THINKING_UNCLOSED_REGEX);
    return unclosedMatch ? unclosedMatch[1].trim() || null : null;
  }, [message.content, harmony]);

  const isThinkingStreaming = useMemo(() => {
    if (harmony || !thinkingContent) return false;
    return !/<think>[\s\S]*?<\/think>/.test(message.content);
  }, [message.content, harmony, thinkingContent]);

  const segments = useMemo(() => {
    if (harmony) return harmony.segments;
    return buildSegments(effectiveContent);
  }, [effectiveContent, harmony]);

  const contentWithoutThinking = useMemo(() => {
    return cleanContent
      .replace(THINKING_REGEX, '')
      .replace(THINKING_UNCLOSED_REGEX, '')
      .replace(EXEC_CLEANUP, '')
      .replace(SYS_OUTPUT_CLEANUP, '')
      .replace(/<tool_response>[\s\S]*?<\/tool_response>/g, '')
      .replace(MISTRAL_CALL_CLEANUP, '')
      .replace(MISTRAL_RESULT_CLEANUP, '')
      .trim();
  }, [cleanContent]);

  const isError = message.role === 'system' && (
    message.content.includes('\u274C') ||
    message.content.includes('Generation Crashed') ||
    message.content.includes('Error')
  );

  return { toolCalls, cleanContent, thinkingContent, isThinkingStreaming, contentWithoutThinking, segments, isError };
}
