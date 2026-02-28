/**
 * Harmony format parser (gpt-oss-20b).
 * Handles the <|start|>assistant<|channel|>... segment structure.
 * Unlike other format collectors (which return Span[]), Harmony replaces
 * the entire parsing pipeline — it produces complete segments + finalContent.
 */
import type { MessageSegment } from './toolSpanCollectors';
import { parseExecCommand } from './toolSpanCollectors';

export interface HarmonyParsed {
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
function pushText(segments: MessageSegment[], text: string) {
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
function extractTrailing(segments: MessageSegment[], segBody: string) {
  const idx = segBody.indexOf('<|end|>');
  if (idx < 0) return;
  const trailing = segBody.slice(idx + 7).replace(/<\|end\|>/g, '').trim();
  if (trailing) pushText(segments, trailing);
}

/** Extract a human-readable command string from tool call JSON. */
function parseCommand(toolName: string, argsJson: string): string {
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
function processToolOutput(acc: HarmonyAccumulator, body: string) {
  const outputText = body
    .replace(/^tool<\|message\|>/, '')
    .replace(/<\|end\|>[\s\S]*$/, '')
    .trim();

  if (acc.pendingCommand) {
    const [{ name, args }] = parseExecCommand(acc.pendingCommand);
    acc.segments.push({ type: 'tool_call', toolCall: {
      id: crypto.randomUUID(), name, arguments: args, output: outputText,
    } });
    acc.pendingCommand = null;
  }

  extractTrailing(acc.segments, body);
}

/** Process an assistant channel segment (analysis/commentary/final). */
function processAssistant(acc: HarmonyAccumulator, body: string) {
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
      acc.pendingCommand = parseCommand(toolCallMatch[1], toolCallMatch[2].trim());

      const postCall = channelBody.split('<|call|>').slice(1).join('<|call|>');
      const commentaryMatch = postCall.match(/<\|message\|>([\s\S]*?)(?:<\|end\|>|$)/);
      if (commentaryMatch) pushText(acc.segments, commentaryMatch[1]);
    } else {
      const messageParts = channelBody.split('<|message|>');
      for (let i = 1; i < messageParts.length; i++) {
        const part = messageParts[i]
          .replace(/<\|call\|>[\s\S]*$/, '')
          .replace(/<\|end\|>[\s\S]*$/, '')
          .trim();
        if (part) pushText(acc.segments, part);
      }
    }
  }

  extractTrailing(acc.segments, body);
}

/**
 * Parse Harmony-format model output (gpt-oss-20b).
 * Returns ordered segments preserving the chronological flow:
 *   text -> command -> text -> command -> ... -> text (final)
 * Returns null if the content is not Harmony format.
 */
export function parseHarmonyContent(raw: string): HarmonyParsed | null {
  if (!HARMONY_DETECT.test(raw)) return null;

  const acc: HarmonyAccumulator = { segments: [], finalParts: [], pendingCommand: null };

  const firstStart = raw.indexOf('<|start|>');
  if (firstStart > 0) {
    pushText(acc.segments, raw.slice(0, firstStart).replace(/<\|end\|>/g, ''));
  }

  const segmentRegex = new RegExp(HARMONY_SEGMENT_PATTERN.source, 'g');
  let match;
  while ((match = segmentRegex.exec(raw)) !== null) {
    const body = match[1];
    if (body.startsWith('tool<|message|>')) {
      processToolOutput(acc, body);
    } else if (body.startsWith('assistant<|channel|>')) {
      processAssistant(acc, body);
    }
  }

  if (acc.pendingCommand) {
    const [{ name: pName, args: pArgs }] = parseExecCommand(acc.pendingCommand);
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
