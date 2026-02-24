/**
 * Format-specific tool call span collectors for message parsing.
 * Each collector finds tool call/response pairs in model output and returns
 * positioned spans for widget rendering in the MD view.
 */
import type { ToolCall } from '../types';

type MessageSegment =
  | { type: 'text'; content: string }
  | { type: 'tool_call'; toolCall: ToolCall }
  | { type: 'thinking'; content: string };

export type Span = { start: number; end: number; segment: MessageSegment };

// --- Regexes ---
const TOOL_RESPONSE_REGEX = /<tool_response>([\s\S]*?)<\/tool_response>/g;
const LLAMA3_FUNC_REGEX = /<function=([^>]+)>([\s\S]*?)<\/function>/g;
const MISTRAL_CALL_REGEX = /\[TOOL_CALLS\]([\s\S]*?)\[\/TOOL_CALLS\]/g;
const MISTRAL_BRACKET_PREFIX = /\[TOOL_CALLS\](\w+)\[ARGS\]/g;
const MISTRAL_RESULT_REGEX = /\[TOOL_RESULTS\]([\s\S]*?)\[\/TOOL_RESULTS\]/g;

// --- Shared helpers ---

/** Extract balanced JSON starting at text[start]. Returns { end, json } or null. */
function extractBalancedJson(text: string, start: number): { end: number; json: string } | null {
  if (start >= text.length || text[start] !== '{') return null;
  let depth = 0;
  let inString = false;
  let prevBackslash = false;
  for (let i = start; i < text.length; i++) {
    const ch = text[i];
    if (inString) {
      if (ch === '"' && !prevBackslash) inString = false;
      prevBackslash = ch === '\\' && !prevBackslash;
    } else {
      if (ch === '"') { inString = true; prevBackslash = false; }
      else if (ch === '{') depth++;
      else if (ch === '}') { depth--; if (depth === 0) return { end: i + 1, json: text.slice(start, i + 1) }; }
    }
  }
  return null;
}

/** Check if there's an unclosed <tool_response> after a given position. */
function findStreamingResponse(content: string, afterPos: number): { output: string; end: number } | null {
  const afterTc = content.slice(afterPos);
  const partialTrMatch = afterTc.match(/^[\s\S]*?<tool_response>([\s\S]*)$/);
  if (!partialTrMatch) return null;
  const lastCompleteTrEnd = content.lastIndexOf('</tool_response>');
  const partialTrStart = content.lastIndexOf('<tool_response>');
  if (partialTrStart <= lastCompleteTrEnd) return null;
  return { output: partialTrMatch[1] || '', end: content.length };
}

function freshResponseMatches(content: string): { start: number; end: number; content: string }[] {
  const matches: { start: number; end: number; content: string }[] = [];
  const re = new RegExp(TOOL_RESPONSE_REGEX.source, 'g');
  let m;
  while ((m = re.exec(content)) !== null)
    matches.push({ start: m.index, end: m.index + m[0].length, content: (m[1] || '').trim() });
  return matches;
}

// --- Llama3 XML: <function=name><parameter=k>v</parameter></function> ---

function parseXmlParams(body: string): Record<string, unknown> {
  const args: Record<string, unknown> = {};
  const re = /<parameter=([^>]+)>([\s\S]*?)<\/parameter>/g;
  let m;
  while ((m = re.exec(body)) !== null) args[m[1].trim()] = m[2].trim();
  return args;
}

export function collectLlama3Spans(content: string): Span[] {
  const spans: Span[] = [];
  let match;
  type F = { start: number; end: number; name: string; args: Record<string, unknown> };
  const funcs: F[] = [];
  while ((match = LLAMA3_FUNC_REGEX.exec(content)) !== null) {
    const args = parseXmlParams(match[2]);
    if (Object.keys(args).length > 0)
      funcs.push({ start: match.index, end: match.index + match[0].length, name: match[1].trim(), args });
  }
  if (funcs.length === 0) return spans;
  const trMatches = freshResponseMatches(content);
  for (const func of funcs) {
    const tr = trMatches.find(r => r.start >= func.end);
    if (tr) trMatches.splice(trMatches.indexOf(tr), 1);
    const prefix = content.slice(Math.max(0, func.start - 30), func.start);
    const tcTag = prefix.match(/<tool_call>\s*$/);
    const spanStart = tcTag ? func.start - tcTag[0].length : func.start;
    const isLast = func === funcs[funcs.length - 1];
    let output: string | undefined = tr ? tr.content : undefined;
    let isStreaming = false;
    let spanEnd = tr ? tr.end : func.end;
    if (!tr && isLast) {
      const streaming = findStreamingResponse(content, func.end);
      if (streaming) { output = streaming.output; isStreaming = true; spanEnd = streaming.end; }
    }
    spans.push({ start: spanStart, end: spanEnd, segment: { type: 'tool_call', toolCall: {
      id: crypto.randomUUID(), name: func.name, arguments: func.args,
      output, isStreaming, isPending: !tr && isLast,
    } } });
  }
  return spans;
}

// --- Mistral: [TOOL_CALLS]...[/TOOL_CALLS] + [TOOL_RESULTS]...[/TOOL_RESULTS] ---

function parseMistralBody(body: string): { name: string; args: Record<string, unknown> } | null {
  const commaIdx = body.indexOf(',{');
  if (commaIdx > 0) {
    const name = body.slice(0, commaIdx).trim();
    try {
      const args = JSON.parse(body.slice(commaIdx + 1));
      if (name && !name.includes(' ')) return { name, args };
    } catch { /* fall through */ }
  }
  try {
    const parsed = JSON.parse(body);
    const item = Array.isArray(parsed) ? parsed[0] : parsed;
    if (item?.name) return { name: item.name, args: item.arguments || {} };
  } catch { /* skip */ }
  return null;
}

export function collectMistralSpans(content: string): Span[] {
  const spans: Span[] = [];
  let match;
  type C = { start: number; end: number; name: string; args: Record<string, unknown> };
  const calls: C[] = [];

  // Try bracket format first: [TOOL_CALLS]name[ARGS]{...}
  // Uses balanced-brace scanner instead of regex for JSON body (nested JSON breaks \{.*?\}).
  const bracketRe = new RegExp(MISTRAL_BRACKET_PREFIX.source, 'g');
  while ((match = bracketRe.exec(content)) !== null) {
    const name = match[1].trim();
    const jsonStart = match.index + match[0].length;
    const balanced = extractBalancedJson(content, jsonStart);
    if (!balanced) continue;
    try {
      const args = JSON.parse(balanced.json);
      calls.push({ start: match.index, end: balanced.end, name, args });
    } catch { /* skip */ }
  }

  // Fall back to closed-tag format: [TOOL_CALLS]...[/TOOL_CALLS]
  if (calls.length === 0) {
    while ((match = MISTRAL_CALL_REGEX.exec(content)) !== null) {
      const parsed = parseMistralBody(match[1].trim());
      if (parsed) calls.push({ start: match.index, end: match.index + match[0].length, ...parsed });
    }
  }

  if (calls.length === 0) return spans;

  // Collect results from both [TOOL_RESULTS] and <tool_response> tags
  const results: { start: number; end: number; content: string }[] = [];
  while ((match = MISTRAL_RESULT_REGEX.exec(content)) !== null)
    results.push({ start: match.index, end: match.index + match[0].length, content: (match[1] || '').trim() });
  const trMatches = freshResponseMatches(content);
  results.push(...trMatches);
  results.sort((a, b) => a.start - b.start);

  for (const call of calls) {
    const res = results.find(r => r.start >= call.end);
    if (res) results.splice(results.indexOf(res), 1);
    const isLast = call === calls[calls.length - 1];
    let output: string | undefined = res ? res.content : undefined;
    let isStreaming = false;
    let spanEnd = res ? res.end : call.end;
    // Check for streaming response on the last call
    if (!res && isLast) {
      const streaming = findStreamingResponse(content, call.end);
      if (streaming) { output = streaming.output; isStreaming = true; spanEnd = streaming.end; }
    }
    spans.push({ start: call.start, end: spanEnd, segment: { type: 'tool_call', toolCall: {
      id: crypto.randomUUID(), name: call.name, arguments: call.args,
      output, isStreaming, isPending: !res && isLast,
    } } });
  }
  return spans;
}
