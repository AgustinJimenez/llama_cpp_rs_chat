/**
 * LFM2 tool call span collector.
 * Format: <|tool_call_start|>[func_name(key="value")]<|tool_call_end|>
 * Extracted from toolSpanCollectors.ts to reduce file size.
 */

import type { ToolTags } from '../types';

import { findStreamingResponse, parsePythonFunctionCall } from './toolFormatUtils';
import type { Span } from './toolSpanCollectors';

const LFM2_CALL_REGEX = /<\|tool_call_start\|>([\s\S]*?)<\|tool_call_end\|>/g;
const LFM2_RESPONSE_REGEX = /<\|tool_response_start\|>([\s\S]*?)<\|tool_response_end\|>/g;

type Lfm2TcMatch = { start: number; end: number; name: string; args: Record<string, unknown> };
type Lfm2TrMatch = { start: number; end: number; content: string };

function stableToolId(name: string, charPos: number): string {
  return `tc-${name}-${charPos}`;
}

export function collectLfm2Spans(content: string, toolTags?: ToolTags): Span[] {
  const spans: Span[] = [];
  let match;

  const tcMatches: Lfm2TcMatch[] = [];
  while ((match = LFM2_CALL_REGEX.exec(content)) !== null) {
    const body = match[1].trim();
    const parsed = parsePythonFunctionCall(body);
    if (!parsed) continue;
    tcMatches.push({
      start: match.index,
      end: match.index + match[0].length,
      name: parsed.name,
      args: parsed.args,
    });
  }

  const trMatches: Lfm2TrMatch[] = [];
  const trRe = new RegExp(LFM2_RESPONSE_REGEX.source, 'g');
  while ((match = trRe.exec(content)) !== null) {
    trMatches.push({
      start: match.index,
      end: match.index + match[0].length,
      content: (match[1] || '').trim(),
    });
  }

  const paired: { tc: Lfm2TcMatch; tr: Lfm2TrMatch | null }[] = [];
  for (const tc of tcMatches) {
    const tr = trMatches.find((r) => r.start > tc.end);
    if (tr) trMatches.splice(trMatches.indexOf(tr), 1);
    paired.push({ tc, tr: tr || null });
  }

  const lastUnmatchedIdx = paired.reduce((acc, p, i) => (p.tr === null ? i : acc), -1);

  for (let i = 0; i < paired.length; i++) {
    const { tc, tr } = paired[i];
    const isLastUnmatched = !tr && i === lastUnmatchedIdx;

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

    spans.push({
      start: tc.start,
      end: spanEnd,
      segment: {
        type: 'tool_call',
        toolCall: {
          id: stableToolId(tc.name, tc.start),
          name: tc.name,
          arguments: tc.args,
          output,
          isStreaming,
          isPending: isLastUnmatched,
        },
      },
    });
  }

  return spans;
}
