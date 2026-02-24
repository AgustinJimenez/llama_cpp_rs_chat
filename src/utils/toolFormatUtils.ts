/**
 * Shared utility functions for tool format parsing.
 * Deduplicated from toolParser.ts, toolSpanCollectors.ts, and useMessageParsing.ts.
 */

/** Extract balanced JSON starting at text[start]. Returns { end, json } or null. */
export function extractBalancedJson(text: string, start: number): { end: number; json: string } | null {
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
export function findStreamingResponse(content: string, afterPos: number): { output: string; end: number } | null {
  const afterTc = content.slice(afterPos);
  const partialTrMatch = afterTc.match(/^[\s\S]*?<tool_response>([\s\S]*)$/);
  if (!partialTrMatch) return null;
  const lastCompleteTrEnd = content.lastIndexOf('</tool_response>');
  const partialTrStart = content.lastIndexOf('<tool_response>');
  if (partialTrStart <= lastCompleteTrEnd) return null;
  return { output: partialTrMatch[1] || '', end: content.length };
}
