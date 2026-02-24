/**
 * Format-specific tool call span collectors for message parsing.
 * Each collector finds tool call/response pairs in model output and returns
 * positioned spans for widget rendering in the MD view.
 */
import type { ToolCall } from '../types';
import { extractBalancedJson, findStreamingResponse } from './toolFormatUtils';

export type MessageSegment =
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

// --- Shared: convert parsed tool calls + results into spans ---

type ParsedCall = { start: number; end: number; name: string; args: Record<string, unknown> };
type ParsedResult = { start: number; end: number; content: string };

function buildToolSpans(
  content: string, calls: ParsedCall[], results: ParsedResult[],
): Span[] {
  const spans: Span[] = [];
  for (const call of calls) {
    const res = results.find(r => r.start >= call.end);
    if (res) results.splice(results.indexOf(res), 1);
    const isLast = call === calls[calls.length - 1];
    let output: string | undefined = res ? res.content : undefined;
    let isStreaming = false;
    let spanEnd = res ? res.end : call.end;
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

/** Bracket format spans: [TOOL_CALLS]name[ARGS]{json} */
function mistralBracketCalls(content: string): ParsedCall[] {
  const calls: ParsedCall[] = [];
  const re = new RegExp(MISTRAL_BRACKET_PREFIX.source, 'g');
  let match;
  while ((match = re.exec(content)) !== null) {
    const balanced = extractBalancedJson(content, match.index + match[0].length);
    if (!balanced) continue;
    try {
      calls.push({ start: match.index, end: balanced.end, name: match[1].trim(), args: JSON.parse(balanced.json) });
    } catch { /* skip */ }
  }
  return calls;
}

/** Closed-tag format spans: [TOOL_CALLS]...[/TOOL_CALLS] */
function mistralClosedTagCalls(content: string): ParsedCall[] {
  const calls: ParsedCall[] = [];
  let match;
  while ((match = MISTRAL_CALL_REGEX.exec(content)) !== null) {
    const parsed = parseMistralBody(match[1].trim());
    if (parsed) calls.push({ start: match.index, end: match.index + match[0].length, ...parsed });
  }
  return calls;
}

/** Bare JSON format spans: [TOOL_CALLS]{"name":"...","arguments":{...}} (Magistral) */
function mistralBareJsonCalls(content: string): ParsedCall[] {
  const calls: ParsedCall[] = [];
  const re = /\[TOOL_CALLS\]\s*\{/g;
  let match;
  while ((match = re.exec(content)) !== null) {
    const balanced = extractBalancedJson(content, match.index + match[0].length - 1);
    if (!balanced) continue;
    try {
      const parsed = JSON.parse(balanced.json);
      if (parsed.name) calls.push({ start: match.index, end: balanced.end, name: parsed.name, args: parsed.arguments || {} });
    } catch { /* skip */ }
  }
  return calls;
}

export function collectMistralSpans(content: string): Span[] {
  const bracket = mistralBracketCalls(content);
  const closed = bracket.length > 0 ? [] : mistralClosedTagCalls(content);
  const calls = bracket.length > 0 ? bracket : closed.length > 0 ? closed : mistralBareJsonCalls(content);

  if (calls.length === 0) return [];

  // Collect results from both [TOOL_RESULTS] and <tool_response> tags
  const results: ParsedResult[] = [];
  let match;
  while ((match = MISTRAL_RESULT_REGEX.exec(content)) !== null)
    results.push({ start: match.index, end: match.index + match[0].length, content: (match[1] || '').trim() });
  results.push(...freshResponseMatches(content));
  results.sort((a, b) => a.start - b.start);

  return buildToolSpans(content, calls, results);
}

// --- SYSTEM.EXEC: <||SYSTEM.EXEC>...<SYSTEM.EXEC||> ---

const EXEC_REGEX = /(?:<\|\|)?SYSTEM\.EXEC>([\s\S]*?)<(?:\|\|)?SYSTEM\.EXEC\|\|>/g;
const SYS_OUTPUT_REGEX = /(?:<\|\|)?SYSTEM\.OUTPUT>([\s\S]*?)<(?:\|\|)?SYSTEM\.OUTPUT\|\|>/g;

/** Convert a raw command string to a ToolCall-compatible name + arguments. */
export function parseExecCommand(command: string): { name: string; args: Record<string, unknown> } {
  try {
    const parsed = JSON.parse(command);
    if (parsed.name) return { name: parsed.name, args: parsed.arguments || {} };
  } catch { /* not JSON */ }
  const funcMatch = command.match(/^(\w+)\((\{[\s\S]*\})\)$/);
  if (funcMatch) {
    try {
      return { name: funcMatch[1], args: JSON.parse(funcMatch[2]) };
    } catch { /* fall through */ }
  }
  return { name: 'execute_command', args: { command } };
}

export function collectExecSpans(content: string): Span[] {
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

// --- Qwen: <tool_call>{"name":"...","arguments":{...}}</tool_call> ---

const TOOL_CALL_REGEX = /<tool_call>([\s\S]*?)<\/tool_call>/g;

export function collectQwenSpans(content: string): Span[] {
  const spans: Span[] = [];
  let match;

  const tcMatches: { start: number; end: number; json: string }[] = [];
  while ((match = TOOL_CALL_REGEX.exec(content)) !== null) {
    tcMatches.push({ start: match.index, end: match.index + match[0].length, json: match[1].trim() });
  }

  const trMatches: { start: number; end: number; content: string }[] = [];
  const trRe = new RegExp(TOOL_RESPONSE_REGEX.source, 'g');
  while ((match = trRe.exec(content)) !== null) {
    trMatches.push({ start: match.index, end: match.index + match[0].length, content: (match[1] || '').trim() });
  }

  type TcMatch = typeof tcMatches[number];
  type TrMatch = typeof trMatches[number];
  const paired: { tc: TcMatch; tr: TrMatch | null }[] = [];

  for (const tc of tcMatches) {
    const tr = trMatches.find(r => r.start > tc.end);
    if (tr) trMatches.splice(trMatches.indexOf(tr), 1);
    paired.push({ tc, tr: tr || null });
  }

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

// --- Segment builder: combines all collectors into ordered segments ---

export const THINKING_REGEX = /<think>[\s\S]*?<\/think>/g;
export const THINKING_UNCLOSED_REGEX = /<think>([\s\S]*)$/;

export function buildSegments(content: string): MessageSegment[] {
  const cleaned = content.replace(THINKING_REGEX, '').replace(THINKING_UNCLOSED_REGEX, '');
  const qwenSpans = collectQwenSpans(cleaned);
  const mistralSpans = qwenSpans.length > 0 ? [] : collectMistralSpans(cleaned);
  const toolSpans = qwenSpans.length > 0 ? qwenSpans
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
