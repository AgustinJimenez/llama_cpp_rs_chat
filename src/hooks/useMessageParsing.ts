import { useMemo } from 'react';

import type { Message, ToolCall, ToolTags } from '../types';
import { parseHarmonyContent } from '../utils/harmonyParser';
import { stripUnclosedToolCallTail } from '../utils/toolFormatUtils';
import { autoParseToolCalls, stripToolCalls } from '../utils/toolParser';
import {
  type MessageSegment,
  buildSegments,
  moveToolsOutOfThinking,
  THINKING_REGEX,
  THINKING_UNCLOSED_REGEX,
  THINKING_ORPHAN_CLOSE_REGEX,
  THINKING_ORPHAN_OPEN_REGEX,
} from '../utils/toolSpanCollectors';

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

// Cleanup regexes — used only by contentWithoutThinking to strip all format tags
const EXEC_CLEANUP = /(?:<\|\|)?SYSTEM\.EXEC>[\s\S]*?<(?:\|\|)?SYSTEM\.EXEC\|\|>/g;
const SYS_OUTPUT_CLEANUP = /(?:<\|\|)?SYSTEM\.OUTPUT>[\s\S]*?<(?:\|\|)?SYSTEM\.OUTPUT\|\|>/g;
const MISTRAL_CALL_CLEANUP =
  /(?:\[TOOL_CALLS\][\s\S]*?\[\/TOOL_CALLS\]|\[TOOL_CALLS\]\w+\[ARGS\]\{[\s\S]*?\}|\[TOOL_CALLS\]\s*\{[^}]*"name"[^}]*"arguments"[\s\S]*?\}\s*\})/g;
const MISTRAL_RESULT_CLEANUP = /\[TOOL_RESULTS\][\s\S]*?\[\/TOOL_RESULTS\]/g;
const LFM2_RESULT_CLEANUP = /<\|tool_response_start\|>[\s\S]*?<\|tool_response_end\|>/g;
// GLM vision/media tags — strip hallucinated image/video/box markers from display
const GLM_VISION_CLEANUP =
  /<\|(?:begin_of_image|image|end_of_image|begin_of_video|video|end_of_video|begin_of_box|end_of_box)\|>/g;
// EOS / stop tokens — strip from display (visible only in RAW view)
// Also matches <[im_end]> — the form some markdown renderers produce when | is escaped.
const EOS_TOKEN_CLEANUP = /<\|(?:im_end|endoftext|end_of_text|eot_id|end)\|>|<\[im_end\]>/g;
// Internal system signals — strip from display
const INTERNAL_SIGNALS_CLEANUP = /\[INFINITE_LOOP_DETECTED\]/g;
// Tool-limit warning injected by the backend — strip from assistant bubble (shown as ⚠️ SYSTEM instead)
const TOOL_LIMIT_WARNING_CLEANUP =
  /⚠️ \[IMPORTANT: You have reached the maximum of \d+ tool calls[^\]]*\]/g;

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

  const esc = (s: string) => s.replaceAll(/[.*+?^${}()|[\]\\]/g, '\\$&');
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
  const effectiveContent = (harmony ? harmony.finalContent : message.content)
    .replaceAll(EOS_TOKEN_CLEANUP, '')
    .replaceAll(INTERNAL_SIGNALS_CLEANUP, '')
    .replaceAll(TOOL_LIMIT_WARNING_CLEANUP, '');

  const toolCalls = useMemo(() => {
    if (message.role === 'assistant') {
      const calls = autoParseToolCalls(effectiveContent);
      if (message.toolCallTimings && message.toolCallTimings.length > 0) {
        return calls.map((call, i) => ({
          ...call,
          duration_ms: message.toolCallTimings?.[i],
        }));
      }
      return calls;
    }
    return [];
  }, [effectiveContent, message.role, message.toolCallTimings]);

  const cleanContent = useMemo(() => {
    let content = stripUnclosedToolCallTail(effectiveContent, toolTags);
    if (toolCalls.length > 0) {
      content = stripToolCalls(content);
    } else {
      content = content
        .replaceAll(/<tool_response>[\s\S]*?<\/tool_response>/g, '')
        .replaceAll(LFM2_RESULT_CLEANUP, '');
    }
    content = content.replaceAll(EOS_TOKEN_CLEANUP, '');
    // Dynamic: strip tool call/response blocks + channel/turn tags from active model
    content = content.replaceAll(CHANNEL_TAG_CLEANUP, '').replaceAll(TURN_TAG_CLEANUP, '');
    if (dynamicCleanup) {
      content = content.replace(dynamicCleanup, '');
    }
    return content;
  }, [effectiveContent, toolCalls.length, toolTags, dynamicCleanup]);

  const thinkingContent = useMemo(() => {
    if (harmony) return null;
    // Handle prefill-mode thinking: models like Qwen3 inject <think> as the assistant
    // prefix via chat template, so the stream has thinking content followed by </think>
    // with no opening tag. Wrap it so the regex below can match uniformly.
    let baseContent = message.content;
    const firstClose = baseContent.indexOf('</think>');
    const firstOpen = baseContent.indexOf('<think>');
    if (firstClose !== -1 && (firstOpen === -1 || firstOpen > firstClose)) {
      const CLOSE_TAG = '</think>';
      baseContent = `<think>${baseContent.slice(0, firstClose)}</think>${baseContent.slice(firstClose + CLOSE_TAG.length)}`;
    }
    // Preprocess: move tool calls out of thinking blocks so they don't show as raw text
    const preprocessed = moveToolsOutOfThinking(baseContent);
    const thinkMatch = preprocessed.match(/<think>([\s\S]*?)<\/think>/);
    if (thinkMatch) return thinkMatch[1].trim() || null; // null for empty <think></think>
    const unclosedMatch = preprocessed.match(THINKING_UNCLOSED_REGEX);
    // Return empty string (not null) for unclosed thinking — this allows the
    // ThinkingBlock to render immediately when <think> opens, even before content arrives.
    return unclosedMatch ? unclosedMatch[1].trim() || '' : null;
  }, [message.content, harmony]);

  const isThinkingStreaming = useMemo(() => {
    if (harmony || thinkingContent == null) return false;
    // For prefill-mode models: thinking is done once </think> appears in the content.
    const firstClose = message.content.indexOf('</think>');
    const firstOpen = message.content.indexOf('<think>');
    const isPrefillMode = firstClose !== -1 && (firstOpen === -1 || firstOpen > firstClose);
    if (isPrefillMode) return false; // </think> is present, so thinking is complete
    return !/<think>[\s\S]*?<\/think>/.test(message.content);
  }, [message.content, harmony, thinkingContent]);

  const segments = useMemo(() => {
    const raw = harmony ? harmony.segments : buildSegments(effectiveContent, toolTags);
    if (!message.toolCallTimings?.length) return raw;
    // Inject duration_ms into tool_call segments so ToolCallBlock can render timing labels
    let toolCallIdx = 0;
    return raw.map((segment) => {
      if (segment.type === 'tool_call') {
        const timing = message.toolCallTimings?.[toolCallIdx++];
        if (timing != null) {
          return { ...segment, toolCall: { ...segment.toolCall, duration_ms: timing } };
        }
      }
      return segment;
    });
  }, [effectiveContent, harmony, toolTags, message.toolCallTimings]);

  const contentWithoutThinking = useMemo(() => {
    let result = cleanContent
      .replace(THINKING_REGEX, '')
      .replace(THINKING_UNCLOSED_REGEX, '')
      .replace(THINKING_ORPHAN_CLOSE_REGEX, '')
      .replace(THINKING_ORPHAN_OPEN_REGEX, '')
      .replaceAll(EXEC_CLEANUP, '')
      .replaceAll(SYS_OUTPUT_CLEANUP, '')
      .replaceAll(/<tool_response>[\s\S]*?<\/tool_response>/g, '')
      .replaceAll(LFM2_RESULT_CLEANUP, '')
      .replaceAll(MISTRAL_CALL_CLEANUP, '')
      .replaceAll(MISTRAL_RESULT_CLEANUP, '')
      .replaceAll(GLM_VISION_CLEANUP, '')
      .replaceAll(EOS_TOKEN_CLEANUP, '')
      // Dynamic: strip tool call/response blocks using active model tags
      .replaceAll(CHANNEL_TAG_CLEANUP, '')
      .replaceAll(TURN_TAG_CLEANUP, '');
    if (dynamicCleanup) {
      result = result.replace(dynamicCleanup, '');
    }
    return result.trim();
  }, [cleanContent, dynamicCleanup]);

  const isError =
    message.role === 'error' ||
    (message.role === 'system' &&
      (message.content.includes('\u274C') ||
        message.content.includes('Generation Crashed') ||
        message.content.includes('Error')));

  return {
    toolCalls,
    cleanContent,
    thinkingContent,
    isThinkingStreaming,
    contentWithoutThinking,
    segments,
    isError,
  };
}
