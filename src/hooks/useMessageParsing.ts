import { useMemo } from 'react';
import { autoParseToolCalls, stripToolCalls } from '../utils/toolParser';
import {
  buildSegments,
  THINKING_REGEX, THINKING_UNCLOSED_REGEX,
} from '../utils/toolSpanCollectors';
import { parseHarmonyContent } from '../utils/harmonyParser';
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

import type { MessageSegment } from '../utils/toolSpanCollectors';

// Cleanup regexes — used only by contentWithoutThinking to strip all format tags
const EXEC_CLEANUP = /(?:<\|\|)?SYSTEM\.EXEC>[\s\S]*?<(?:\|\|)?SYSTEM\.EXEC\|\|>/g;
const SYS_OUTPUT_CLEANUP = /(?:<\|\|)?SYSTEM\.OUTPUT>[\s\S]*?<(?:\|\|)?SYSTEM\.OUTPUT\|\|>/g;
const MISTRAL_CALL_CLEANUP = /(?:\[TOOL_CALLS\][\s\S]*?\[\/TOOL_CALLS\]|\[TOOL_CALLS\]\w+\[ARGS\]\{[\s\S]*?\}|\[TOOL_CALLS\]\s*\{[^}]*"name"[^}]*"arguments"[\s\S]*?\}\s*\})/g;
const MISTRAL_RESULT_CLEANUP = /\[TOOL_RESULTS\][\s\S]*?\[\/TOOL_RESULTS\]/g;

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
