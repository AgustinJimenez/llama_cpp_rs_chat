import { useMemo } from 'react';
import { autoParseToolCalls, stripToolCalls } from '../utils/toolParser';
import { stripUnclosedToolCallTail } from '../utils/toolFormatUtils';
import {
  buildSegments,
  moveToolsOutOfThinking,
  THINKING_REGEX, THINKING_UNCLOSED_REGEX, THINKING_ORPHAN_CLOSE_REGEX,
} from '../utils/toolSpanCollectors';
import { parseHarmonyContent } from '../utils/harmonyParser';
import type { Message, ToolCall, ToolTags } from '../types';

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
const LFM2_RESULT_CLEANUP = /<\|tool_response_start\|>[\s\S]*?<\|tool_response_end\|>/g;
// GLM vision/media tags — strip hallucinated image/video/box markers from display
const GLM_VISION_CLEANUP = /<\|(?:begin_of_image|image|end_of_image|begin_of_video|video|end_of_video|begin_of_box|end_of_box)\|>/g;
// EOS / stop tokens — strip from display (visible only in RAW view)
const EOS_TOKEN_CLEANUP = /<\|(?:im_end|endoftext|end_of_text|eot_id|end)\|>/g;
// Internal system signals — strip from display
const INTERNAL_SIGNALS_CLEANUP = /\[INFINITE_LOOP_DETECTED\]/g;

/**
 * Build dynamic cleanup regex from active toolTags.
 * Strips exec_open...exec_close and output_open...output_close blocks,
 * so any model's tags are handled without hardcoded patterns.
 * Returns null if tags are the default SYSTEM.EXEC (already handled by EXEC_CLEANUP).
 */
function buildDynamicTagCleanup(tags?: ToolTags): RegExp | null {
  if (!tags) return null;
  // Skip if default SYSTEM.EXEC tags (already covered by hardcoded regexes)
  if (tags.exec_open.includes('SYSTEM.EXEC')) return null;
  // Skip if Qwen/GLM <tool_call> (already covered by toolParser strip)
  if (tags.exec_open === '<tool_call>') return null;

  const esc = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const parts: string[] = [];
  // Strip tool call blocks: exec_open...exec_close
  parts.push(`${esc(tags.exec_open)}[\\s\\S]*?${esc(tags.exec_close)}`);
  // Strip tool response blocks: output_open...output_close
  parts.push(`${esc(tags.output_open)}[\\s\\S]*?${esc(tags.output_close)}`);
  return new RegExp(parts.join('|'), 'g');
}

/**
 * Strip model-specific channel/role/control tags that leak into the display.
 * These are tags like <|channel>thought<channel|>, <turn|>, etc.
 * Handles any model that uses <|tag>...<tag|> or <tag|> patterns.
 */
const CHANNEL_TAG_CLEANUP = /<\|channel>[\s\S]*?<channel\|>/g;
const TURN_TAG_CLEANUP = /<(?:\|turn>(?:model|user|system|tool)|turn\|>)/g;

/**
 * Parse a message and extract various components:
 * - Tool calls
 * - Thinking content (for reasoning models)
 * - SYSTEM.EXEC blocks (command executions)
 * - Ordered segments (text + commands + tool calls interleaved chronologically)
 * - Clean content without special tags
 */
export function useMessageParsing(message: Message, toolTags?: ToolTags): ParsedMessage {
  const harmony = useMemo(() => parseHarmonyContent(message.content), [message.content]);
  const dynamicCleanup = useMemo(() => buildDynamicTagCleanup(toolTags), [toolTags]);
  const effectiveContent = (harmony ? harmony.finalContent : message.content).replace(EOS_TOKEN_CLEANUP, '').replace(INTERNAL_SIGNALS_CLEANUP, '');

  const toolCalls = useMemo(() => {
    if (message.role === 'assistant') return autoParseToolCalls(effectiveContent);
    return [];
  }, [effectiveContent, message.role]);

  const cleanContent = useMemo(() => {
    let content = stripUnclosedToolCallTail(effectiveContent, toolTags);
    if (toolCalls.length > 0) {
      content = stripToolCalls(content);
    } else {
      content = content
        .replace(/<tool_response>[\s\S]*?<\/tool_response>/g, '')
        .replace(LFM2_RESULT_CLEANUP, '');
    }
    content = content.replace(EOS_TOKEN_CLEANUP, '');
    // Dynamic: strip tool call/response blocks + channel/turn tags from active model
    content = content.replace(CHANNEL_TAG_CLEANUP, '').replace(TURN_TAG_CLEANUP, '');
    if (dynamicCleanup) {
      content = content.replace(dynamicCleanup, '');
    }
    return content;
  }, [effectiveContent, toolCalls.length, toolTags, dynamicCleanup]);

  const thinkingContent = useMemo(() => {
    if (harmony) return null;
    // Preprocess: move tool calls out of thinking blocks so they don't show as raw text
    const preprocessed = moveToolsOutOfThinking(message.content);
    const thinkMatch = preprocessed.match(/<think>([\s\S]*?)<\/think>/);
    if (thinkMatch) return thinkMatch[1].trim();
    const unclosedMatch = preprocessed.match(THINKING_UNCLOSED_REGEX);
    return unclosedMatch ? unclosedMatch[1].trim() || null : null;
  }, [message.content, harmony]);

  const isThinkingStreaming = useMemo(() => {
    if (harmony || !thinkingContent) return false;
    return !/<think>[\s\S]*?<\/think>/.test(message.content);
  }, [message.content, harmony, thinkingContent]);

  const segments = useMemo(() => {
    if (harmony) return harmony.segments;
    return buildSegments(effectiveContent, toolTags);
  }, [effectiveContent, harmony, toolTags]);

  const contentWithoutThinking = useMemo(() => {
    let result = cleanContent
      .replace(THINKING_REGEX, '')
      .replace(THINKING_UNCLOSED_REGEX, '')
      .replace(THINKING_ORPHAN_CLOSE_REGEX, '')
      .replace(EXEC_CLEANUP, '')
      .replace(SYS_OUTPUT_CLEANUP, '')
      .replace(/<tool_response>[\s\S]*?<\/tool_response>/g, '')
      .replace(LFM2_RESULT_CLEANUP, '')
      .replace(MISTRAL_CALL_CLEANUP, '')
      .replace(MISTRAL_RESULT_CLEANUP, '')
      .replace(GLM_VISION_CLEANUP, '')
      .replace(EOS_TOKEN_CLEANUP, '')
      // Dynamic: strip tool call/response blocks using active model tags
      .replace(CHANNEL_TAG_CLEANUP, '')
      .replace(TURN_TAG_CLEANUP, '');
    if (dynamicCleanup) {
      result = result.replace(dynamicCleanup, '');
    }
    return result.trim();
  }, [cleanContent, dynamicCleanup]);

  const isError = message.role === 'system' && (
    message.content.includes('\u274C') ||
    message.content.includes('Generation Crashed') ||
    message.content.includes('Error')
  );

  return { toolCalls, cleanContent, thinkingContent, isThinkingStreaming, contentWithoutThinking, segments, isError };
}
