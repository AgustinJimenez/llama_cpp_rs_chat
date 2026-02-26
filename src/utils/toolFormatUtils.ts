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

/**
 * Remove trailing, unclosed tool-call markup so raw tags don't flash in the UI
 * during streaming. This only trims the incomplete tail; completed tool calls
 * remain intact for parsing and rendering.
 */
export function stripUnclosedToolCallTail(content: string): string {
  let cutoff = content.length;

  // Qwen: <tool_call> ... (no closing tag yet)
  const lastToolOpen = content.lastIndexOf('<tool_call>');
  if (lastToolOpen !== -1) {
    const lastToolClose = content.lastIndexOf('</tool_call>');
    if (lastToolClose < lastToolOpen) cutoff = Math.min(cutoff, lastToolOpen);
  }

  // Llama 3: <function=...> ... (no closing tag yet)
  const lastFuncOpen = content.lastIndexOf('<function=');
  if (lastFuncOpen !== -1) {
    const lastFuncClose = content.lastIndexOf('</function>');
    if (lastFuncClose < lastFuncOpen) cutoff = Math.min(cutoff, lastFuncOpen);
  }

  // Mistral: [TOOL_CALLS] ... (no closing tag and incomplete JSON)
  const lastMistralOpen = content.lastIndexOf('[TOOL_CALLS]');
  if (lastMistralOpen !== -1) {
    const closeIdx = content.indexOf('[/TOOL_CALLS]', lastMistralOpen);
    if (closeIdx === -1) {
      let complete = false;
      const afterStart = lastMistralOpen + '[TOOL_CALLS]'.length;
      const argsIdx = content.indexOf('[ARGS]', afterStart);
      if (argsIdx !== -1) {
        const braceIdx = content.indexOf('{', argsIdx + '[ARGS]'.length);
        if (braceIdx !== -1) {
          const balanced = extractBalancedJson(content, braceIdx);
          if (balanced) complete = true;
        }
      } else {
        const braceIdx = content.indexOf('{', afterStart);
        if (braceIdx !== -1) {
          const balanced = extractBalancedJson(content, braceIdx);
          if (balanced) complete = true;
        }
      }
      if (!complete) cutoff = Math.min(cutoff, lastMistralOpen);
    }
  }

  // SYSTEM.EXEC: <||SYSTEM.EXEC> ... (no closing tag yet)
  const lastExecOpen = Math.max(
    content.lastIndexOf('<||SYSTEM.EXEC>'),
    content.lastIndexOf('SYSTEM.EXEC>')
  );
  if (lastExecOpen !== -1) {
    const lastExecClose = Math.max(
      content.lastIndexOf('<SYSTEM.EXEC||>'),
      content.lastIndexOf('SYSTEM.EXEC||>')
    );
    if (lastExecClose < lastExecOpen) cutoff = Math.min(cutoff, lastExecOpen);
  }

  return cutoff < content.length ? content.slice(0, cutoff).trimEnd() : content;
}
