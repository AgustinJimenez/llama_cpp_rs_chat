/**
 * Shared utility functions for tool format parsing.
 * Deduplicated from toolParser.ts, toolSpanCollectors.ts, and useMessageParsing.ts.
 */
import type { ToolTags } from '../types';

/** Extract balanced JSON starting at text[start]. Returns { end, json } or null. */
export function extractBalancedJson(
  text: string,
  start: number,
): { end: number; json: string } | null {
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
      if (ch === '"') {
        inString = true;
        prevBackslash = false;
      } else if (ch === '{') depth++;
      else if (ch === '}') {
        depth--;
        if (depth === 0) return { end: i + 1, json: text.slice(start, i + 1) };
      }
    }
  }
  return null;
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function parsePythonValue(raw: string): unknown {
  const value = raw.trim();
  if (!value) return '';
  if (
    (value.startsWith('"') && value.endsWith('"')) ||
    (value.startsWith("'") && value.endsWith("'"))
  ) {
    const unquoted = value.slice(1, -1);
    return unquoted.replace(/\\(['"\\nrt])/g, (_, ch) => {
      if (ch === 'n') return '\n';
      if (ch === 'r') return '\r';
      if (ch === 't') return '\t';
      return ch;
    });
  }
  if (value === 'true' || value === 'True') return true;
  if (value === 'false' || value === 'False') return false;
  if (value === 'null' || value === 'None') return null;
  if (/^-?\d+(\.\d+)?([eE][+-]?\d+)?$/.test(value)) return Number(value);
  if (
    (value.startsWith('{') && value.endsWith('}')) ||
    (value.startsWith('[') && value.endsWith(']'))
  ) {
    try {
      return JSON.parse(value);
    } catch {
      try {
        const replaced = value.includes('"') ? value : value.replace(/'/g, '"');
        return JSON.parse(replaced);
      } catch {
        return value;
      }
    }
  }
  return value;
}

function splitPythonArgs(argList: string): string[] {
  const parts: string[] = [];
  let current = '';
  let depth = 0;
  let quote: '"' | "'" | null = null;
  let prevBackslash = false;

  for (let i = 0; i < argList.length; i++) {
    const ch = argList[i];
    if (quote) {
      current += ch;
      if (ch === quote && !prevBackslash) quote = null;
      prevBackslash = ch === '\\' && !prevBackslash;
      continue;
    }

    if (ch === '"' || ch === "'") {
      quote = ch;
      current += ch;
      prevBackslash = false;
      continue;
    }

    if (ch === '(' || ch === '[' || ch === '{') depth++;
    if (ch === ')' || ch === ']' || ch === '}') depth = Math.max(0, depth - 1);

    if (ch === ',' && depth === 0) {
      if (current.trim()) parts.push(current.trim());
      current = '';
      continue;
    }
    current += ch;
  }

  if (current.trim()) parts.push(current.trim());
  return parts;
}

export function parsePythonFunctionCall(
  body: string,
): { name: string; args: Record<string, unknown> } | null {
  let trimmed = body.trim();
  if (trimmed.startsWith('[') && trimmed.endsWith(']')) trimmed = trimmed.slice(1, -1).trim();

  const callMatch = trimmed.match(/^([a-zA-Z_]\w*)\s*\(([\s\S]*)\)\s*$/);
  if (!callMatch) return null;

  const name = callMatch[1].trim();
  const argsRaw = callMatch[2].trim();
  if (!name) return null;

  const args: Record<string, unknown> = {};
  if (!argsRaw) return { name, args };

  for (const part of splitPythonArgs(argsRaw)) {
    const eqIdx = part.indexOf('=');
    if (eqIdx === -1) continue;
    const key = part.slice(0, eqIdx).trim();
    const value = part.slice(eqIdx + 1);
    if (!key) continue;
    args[key] = parsePythonValue(value);
  }

  return { name, args };
}

/** Check if there's an unclosed tool response tag after a given position. */
export function findStreamingResponse(
  content: string,
  afterPos: number,
  toolTags?: ToolTags,
): { output: string; end: number } | null {
  const afterTc = content.slice(afterPos);
  const openTag = toolTags?.output_open || '<tool_response>';
  const closeTag = toolTags?.output_close || '</tool_response>';
  const openRe = new RegExp(`^[\\s\\S]*?${escapeRegExp(openTag)}([\\s\\S]*)$`);

  const partialTrMatch = afterTc.match(openRe);
  if (!partialTrMatch) return null;

  const lastCompleteTrEnd = content.lastIndexOf(closeTag);
  const partialTrStart = content.lastIndexOf(openTag);
  if (partialTrStart <= lastCompleteTrEnd) return null;

  return { output: partialTrMatch[1] || '', end: content.length };
}

/** Check if a Mistral [TOOL_CALLS] block has complete JSON arguments. */
function isMistralToolCallComplete(content: string, openIdx: number): boolean {
  const afterStart = openIdx + '[TOOL_CALLS]'.length;
  const argsIdx = content.indexOf('[ARGS]', afterStart);
  const searchFrom = argsIdx !== -1 ? argsIdx + '[ARGS]'.length : afterStart;
  const braceIdx = content.indexOf('{', searchFrom);
  if (braceIdx === -1) return false;
  return extractBalancedJson(content, braceIdx) !== null;
}

/**
 * Remove trailing, unclosed tool-call markup so raw tags don't flash in the UI
 * during streaming. This only trims the incomplete tail; completed tool calls
 * remain intact for parsing and rendering.
 *
 * When `toolTags` is provided (model loaded), uses the model's actual exec tags.
 * When absent (viewing old conversations), falls back to multi-format detection.
 */
export function stripUnclosedToolCallTail(content: string, toolTags?: ToolTags): string {
  let cutoff = content.length;

  if (toolTags) {
    // Dynamic: use the model's actual exec_open / exec_close tags
    const lastOpen = content.lastIndexOf(toolTags.exec_open);
    if (lastOpen !== -1) {
      let lastClose = content.lastIndexOf(toolTags.exec_close);
      // GLM models close <tool_call> with <|end_of_box|> instead of </tool_call>
      if (toolTags.exec_open === '<tool_call>') {
        lastClose = Math.max(lastClose, content.lastIndexOf('<|end_of_box|>'));
      }
      if (lastClose < lastOpen) {
        // For Mistral bracket format ([TOOL_CALLS]name[ARGS]{json}), the close tag
        // [/TOOL_CALLS] is never used. Check if the tool call has complete JSON
        // arguments before treating it as unclosed.
        if (toolTags.exec_open === '[TOOL_CALLS]') {
          if (!isMistralToolCallComplete(content, lastOpen)) {
            cutoff = Math.min(cutoff, lastOpen);
          }
        } else {
          // Check if there's a <tool_response> after the open tag — if so, the tool
          // call was completed (e.g. Qwen3-Coder: <tool_call><function=...></function>
          // has no </tool_call> but tool executed and response was injected)
          const hasResponse = content.indexOf('<tool_response>', lastOpen) !== -1;
          if (!hasResponse) {
            cutoff = Math.min(cutoff, lastOpen);
          }
        }
      }
    }
  } else {
    // Fallback: check all known formats when no model tags are available

    // Qwen/GLM: <tool_call> ... (no closing tag yet)
    // GLM may close with <|end_of_box|> instead of </tool_call>
    const lastToolOpen = content.lastIndexOf('<tool_call>');
    if (lastToolOpen !== -1) {
      const lastToolClose = Math.max(
        content.lastIndexOf('</tool_call>'),
        content.lastIndexOf('<|end_of_box|>'),
      );
      if (lastToolClose < lastToolOpen) {
        // Don't strip if a <tool_response> exists after the open tag (tool completed)
        const hasResponse = content.indexOf('<tool_response>', lastToolOpen) !== -1;
        if (!hasResponse) cutoff = Math.min(cutoff, lastToolOpen);
      }
    }

    // Mistral: [TOOL_CALLS] ... (no closing tag and incomplete JSON)
    const lastMistralOpen = content.lastIndexOf('[TOOL_CALLS]');
    if (lastMistralOpen !== -1) {
      const closeIdx = content.indexOf('[/TOOL_CALLS]', lastMistralOpen);
      if (closeIdx === -1 && !isMistralToolCallComplete(content, lastMistralOpen)) {
        cutoff = Math.min(cutoff, lastMistralOpen);
      }
    }

    // SYSTEM.EXEC: <||SYSTEM.EXEC> ... (no closing tag yet)
    const lastExecOpen = Math.max(
      content.lastIndexOf('<||SYSTEM.EXEC>'),
      content.lastIndexOf('SYSTEM.EXEC>'),
    );
    if (lastExecOpen !== -1) {
      const lastExecClose = Math.max(
        content.lastIndexOf('<SYSTEM.EXEC||>'),
        content.lastIndexOf('SYSTEM.EXEC||>'),
      );
      if (lastExecClose < lastExecOpen) cutoff = Math.min(cutoff, lastExecOpen);
    }
  }

  // Llama 3: <function=...> ... (no closing tag yet) — always check, not covered by toolTags
  const lastFuncOpen = content.lastIndexOf('<function=');
  if (lastFuncOpen !== -1) {
    const lastFuncClose = content.lastIndexOf('</function>');
    if (lastFuncClose < lastFuncOpen) cutoff = Math.min(cutoff, lastFuncOpen);
  }

  return cutoff < content.length ? content.slice(0, cutoff).trimEnd() : content;
}
