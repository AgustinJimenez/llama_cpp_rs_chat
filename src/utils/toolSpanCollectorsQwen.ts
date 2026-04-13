/**
 * Qwen/GLM tool call span collector.
 * Extracted from toolSpanCollectors.ts to reduce file size and complexity.
 */

import type { ToolTags } from '../types';

import { extractBalancedJson, findStreamingResponse } from './toolFormatUtils';
import type { Span } from './toolSpanCollectors';

const TOOL_CALL_REGEX =
  /<tool_call>([\s\S]*?)(?:<\/tool_call>|<\|end_of_box\|>|(?=\s*<tool_response>))/g;
const TOOL_RESPONSE_REGEX = /<tool_response>([\s\S]*?)<\/tool_response>/g;

type QwenTcMatch = { start: number; end: number; json: string };
type QwenTrMatch = { start: number; end: number; content: string };

/** Generate a stable tool call ID from name and character position in the message. */
function stableToolId(name: string, charPos: number): string {
  return `tc-${name}-${charPos}`;
}

/** Resolve output/streaming state for a paired tool call + response. */
function resolveQwenOutput(
  content: string,
  tc: QwenTcMatch,
  tr: QwenTrMatch | null,
  isLastUnmatched: boolean,
  toolTags?: ToolTags,
): { output?: string; isStreaming: boolean; spanEnd: number } {
  let output: string | undefined = tr ? tr.content : undefined;
  let isStreaming = false;
  let spanEnd = tr ? tr.end : tc.end;
  if (isLastUnmatched) {
    const streaming = findStreamingResponse(content, tc.end, toolTags);
    if (streaming) {
      output = streaming.output;
      isStreaming = true;
      spanEnd = streaming.end;
    }
  }
  return { output, isStreaming, spanEnd };
}

/** Parse Llama3 XML parameter format inside <tool_call> tags */
function parseXmlParams(body: string): Record<string, unknown> {
  const args: Record<string, unknown> = {};
  const openRe = /<parameter=([^>]+)>/g;
  const opens: { key: string; contentStart: number }[] = [];
  let m;
  while ((m = openRe.exec(body)) !== null) {
    opens.push({ key: m[1].trim(), contentStart: m.index + m[0].length });
  }
  for (let i = 0; i < opens.length; i++) {
    const start = opens[i].contentStart;
    const boundary =
      i + 1 < opens.length
        ? body.lastIndexOf('</parameter>', opens[i + 1].contentStart)
        : body.lastIndexOf('</parameter>');
    if (boundary > start) {
      args[opens[i].key] = body.slice(start, boundary).trim();
    } else {
      const end =
        i + 1 < opens.length
          ? opens[i + 1].contentStart - `<parameter=${opens[i + 1].key}>`.length
          : body.length;
      args[opens[i].key] = body
        .slice(start, end)
        .replace(/<\/parameter>\s*$/, '')
        .trim();
    }
  }
  return args;
}

/** Parse GLM native XML arg format: `name\n<arg_key>k</arg_key>\n<arg_value>v</arg_value>` */
function parseGlmXmlArgs(body: string): { name: string; args: Record<string, unknown> } | null {
  if (!body.includes('<arg_key>')) return null;
  const firstArgPos = body.indexOf('<arg_key>');
  const name = body.slice(0, firstArgPos).trim();
  if (!name || name.includes(' ') || name.includes('{')) return null;
  const args: Record<string, unknown> = {};
  const re = /<arg_key>([\s\S]*?)<\/arg_key>\s*<arg_value>([\s\S]*?)<\/arg_value>/g;
  let m;
  while ((m = re.exec(body)) !== null) {
    const key = m[1].trim();
    const val = m[2].trim();
    try {
      args[key] = JSON.parse(val);
    } catch {
      args[key] = val;
    }
  }
  return Object.keys(args).length > 0 ? { name, args } : null;
}

/** Try to parse a tool_call body as standard JSON (Qwen format) */
function tryParseJsonBody(body: string): boolean {
  try {
    const parsed = JSON.parse(body);
    const items = Array.isArray(parsed) ? parsed : [parsed];
    return items.some((item: Record<string, unknown>) => item?.name);
  } catch {
    return false;
  }
}

/** Try GLM-4.7 "name{json}" format */
function tryParseNameJsonBody(body: string): string | null {
  const nameJsonMatch = body.match(/^(\w+)(\{[\s\S]*\})$/);
  if (!nameJsonMatch) return null;
  try {
    const args = JSON.parse(nameJsonMatch[2]);
    return JSON.stringify({ name: nameJsonMatch[1], arguments: args });
  } catch {
    return null;
  }
}

/** Try Llama3 format inside <tool_call> with closing </function> */
function tryParseLlama3Closed(body: string): string | null {
  const funcMatchClosed = body.match(/<function=([^>]+)>([\s\S]*)<\/function>/);
  if (!funcMatchClosed) return null;
  const args = parseXmlParams(funcMatchClosed[2]);
  return JSON.stringify({ name: funcMatchClosed[1].trim(), arguments: args });
}

/** Try <function=name> without </function> -- extract name + JSON args */
function tryParseLlama3Open(body: string): string | null {
  const funcMatchOpen = body.match(/^<function=([^>]+)>([\s\S]*)$/);
  if (!funcMatchOpen) return null;
  const name = funcMatchOpen[1].trim();
  const rest = funcMatchOpen[2].trim();
  const jsonStart = rest.indexOf('{');
  if (jsonStart < 0) return null;
  const balanced = extractBalancedJson(rest, jsonStart);
  if (!balanced) return null;
  try {
    const args = JSON.parse(balanced.json);
    const finalArgs =
      args.arguments && typeof args.arguments === 'object' && !args.name ? args.arguments : args;
    return JSON.stringify({ name, arguments: finalArgs });
  } catch {
    return null;
  }
}

/** Collect all tool call matches from content */
function collectToolCallMatches(content: string): QwenTcMatch[] {
  const tcMatches: QwenTcMatch[] = [];
  const re = new RegExp(TOOL_CALL_REGEX.source, TOOL_CALL_REGEX.flags);
  let match;
  while ((match = re.exec(content)) !== null) {
    const body = match[1].trim();
    const matchStart = match.index;
    const matchEnd = match.index + match[0].length;

    // Try JSON first (standard Qwen format)
    if (tryParseJsonBody(body)) {
      tcMatches.push({ start: matchStart, end: matchEnd, json: body });
      continue;
    }
    // Fallback 1: GLM-4.7 "name{json}" format
    const nameJson = tryParseNameJsonBody(body);
    if (nameJson) {
      tcMatches.push({ start: matchStart, end: matchEnd, json: nameJson });
      continue;
    }
    // Fallback 2: GLM native XML format
    const glmParsed = parseGlmXmlArgs(body);
    if (glmParsed) {
      const wrapped = JSON.stringify({ name: glmParsed.name, arguments: glmParsed.args });
      tcMatches.push({ start: matchStart, end: matchEnd, json: wrapped });
      continue;
    }
    // Fallback 3: Llama3 format with closing </function>
    const llama3Closed = tryParseLlama3Closed(body);
    if (llama3Closed) {
      tcMatches.push({ start: matchStart, end: matchEnd, json: llama3Closed });
      continue;
    }
    // Fallback 4: <function=name> without </function>
    const llama3Open = tryParseLlama3Open(body);
    if (llama3Open) {
      tcMatches.push({ start: matchStart, end: matchEnd, json: llama3Open });
    }
  }
  return tcMatches;
}

/** Collect tool response matches from content */
function collectResponseMatches(content: string): QwenTrMatch[] {
  const trMatches: QwenTrMatch[] = [];
  const trRe = new RegExp(TOOL_RESPONSE_REGEX.source, 'g');
  let match;
  while ((match = trRe.exec(content)) !== null) {
    trMatches.push({
      start: match.index,
      end: match.index + match[0].length,
      content: (match[1] || '').trim(),
    });
  }
  return trMatches;
}

export function collectQwenSpans(content: string, toolTags?: ToolTags): Span[] {
  const spans: Span[] = [];
  const tcMatches = collectToolCallMatches(content);
  const trMatches = collectResponseMatches(content);

  const paired: { tc: QwenTcMatch; tr: QwenTrMatch | null }[] = [];
  for (const tc of tcMatches) {
    const tr = trMatches.find((r) => r.start > tc.end);
    if (tr) trMatches.splice(trMatches.indexOf(tr), 1);
    paired.push({ tc, tr: tr || null });
  }

  const lastUnmatchedIdx = paired.reduce((acc, p, i) => (p.tr === null ? i : acc), -1);

  for (let i = 0; i < paired.length; i++) {
    const { tc, tr } = paired[i];
    try {
      const parsed = JSON.parse(tc.json);
      const items = Array.isArray(parsed) ? parsed : [parsed];
      const isLastUnmatched = !tr && i === lastUnmatchedIdx;
      const resolved = resolveQwenOutput(content, tc, tr, isLastUnmatched, toolTags);

      for (const item of items) {
        if (!item?.name) continue;
        spans.push({
          start: tc.start,
          end: resolved.spanEnd,
          segment: {
            type: 'tool_call',
            toolCall: {
              id: stableToolId(item.name, tc.start),
              name: item.name,
              arguments: item.arguments || {},
              output: resolved.output,
              isStreaming: resolved.isStreaming,
              isPending: isLastUnmatched,
            },
          },
        });
      }
    } catch {
      // Skip unparseable tool calls
    }
  }

  return spans;
}

// Re-export resolveQwenOutput for use by LFM2 collector
export { resolveQwenOutput };
export type { QwenTcMatch, QwenTrMatch };
