/**
 * Gemma 4 tool call span collector.
 * Extracted from toolSpanCollectors.ts to reduce file size.
 */

import type { Span } from './toolSpanCollectors';

// Gemma 4 tool call: close tag may be <tool_call|> or may be stripped by backend
const GEMMA4_CALL_REGEX = /<\|tool_call>call:(\w+)\{([\s\S]*?)\}(?:<tool_call\|>)?/g;
const GEMMA4_RESPONSE_REGEX = /<\|tool_response>([\s\S]*?)(?:<tool_response\|>|$)/g;

/** Channel tags (thinking): <|channel>thought<channel|> */
export const CHANNEL_TAG_REGEX = /<\|channel>[\s\S]*?<channel\|>/g;
/** Turn tags: <|turn>model, <turn|>, etc. */
export const TURN_TAG_REGEX = /<(?:\|turn>(?:model|user|system|tool)|turn\|>)/g;

/** Generate a stable tool call ID from name and character position. */
function stableToolId(name: string, charPos: number): string {
  return `tc-${name}-${charPos}`;
}

/** Parse Gemma 4 key:value args into a simple object */
function parseGemma4Args(raw: string): Record<string, unknown> {
  const args: Record<string, unknown> = {};
  const quoteDelim = '<|"|>';
  let pos = 0;
  while (pos < raw.length) {
    while (pos < raw.length && /\s/.test(raw[pos])) pos++;
    if (pos >= raw.length) break;
    const colonIdx = raw.indexOf(':', pos);
    if (colonIdx < 0) break;
    const key = raw.slice(pos, colonIdx).trim();
    pos = colonIdx + 1;
    while (pos < raw.length && /\s/.test(raw[pos])) pos++;
    if (pos >= raw.length) break;
    if (raw.startsWith(quoteDelim, pos)) {
      const contentStart = pos + quoteDelim.length;
      const endIdx = raw.indexOf(quoteDelim, contentStart);
      if (endIdx >= 0) {
        args[key] = raw.slice(contentStart, endIdx);
        pos = endIdx + quoteDelim.length;
      } else {
        args[key] = raw.slice(contentStart);
        pos = raw.length;
      }
    } else if (raw.startsWith('true', pos)) {
      args[key] = true;
      pos += 4;
    } else if (raw.startsWith('false', pos)) {
      args[key] = false;
      pos += 5;
    } else {
      const valStart = pos;
      while (pos < raw.length && raw[pos] !== ',') pos++;
      args[key] = raw.slice(valStart, pos).trim();
    }
    if (pos < raw.length && raw[pos] === ',') pos++;
  }
  return args;
}

export function collectGemma4Spans(content: string): Span[] {
  const spans: Span[] = [];
  const calls: { start: number; end: number; name: string; args: Record<string, unknown> }[] = [];
  const responses: { start: number; end: number; content: string }[] = [];

  let m;
  const callRe = new RegExp(GEMMA4_CALL_REGEX.source, 'g');
  while ((m = callRe.exec(content)) !== null) {
    calls.push({
      start: m.index,
      end: m.index + m[0].length,
      name: m[1],
      args: parseGemma4Args(m[2]),
    });
  }
  const respRe = new RegExp(GEMMA4_RESPONSE_REGEX.source, 'g');
  while ((m = respRe.exec(content)) !== null) {
    responses.push({ start: m.index, end: m.index + m[0].length, content: m[1].trim() });
  }

  for (let i = 0; i < calls.length; i++) {
    const call = calls[i];
    const resp = i < responses.length ? responses[i] : null;
    const spanEnd = resp ? resp.end : call.end;
    spans.push({
      start: call.start,
      end: spanEnd,
      segment: {
        type: 'tool_call',
        toolCall: {
          id: stableToolId(call.name, call.start),
          name: call.name,
          arguments: call.args,
          output: resp?.content,
          isPending: !resp,
        },
      },
    });
  }
  return spans;
}
