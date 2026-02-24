import type { ToolCall, ToolFormat } from '../types';

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

/**
 * Tool parser interface for different model formats
 */
interface ToolParser {
  detect(text: string): boolean;
  parse(text: string): ToolCall[];
}

/**
 * Mistral/Devstral tool parser
 * Comma format:   [TOOL_CALLS]function_name,{"arg": "value"}[/TOOL_CALLS]
 * JSON format:    [TOOL_CALLS][{"name":"func","arguments":{...}}][/TOOL_CALLS]
 * Bracket format: [TOOL_CALLS]function_name[ARGS]{"arg": "value"}
 */
const mistralParser: ToolParser = {
  detect(text: string): boolean {
    return text.includes('[TOOL_CALLS]');
  },

  parse(text: string): ToolCall[] {
    const toolCalls: ToolCall[] = [];

    // Try bracket format first: [TOOL_CALLS]name[ARGS]{...}
    // Uses balanced-brace scanner for JSON body (nested JSON breaks non-greedy regex).
    const bracketRegex = /\[TOOL_CALLS\](\w+)\[ARGS\]/g;
    let match;
    while ((match = bracketRegex.exec(text)) !== null) {
      const name = match[1].trim();
      const jsonStart = match.index + match[0].length;
      const balanced = extractBalancedJson(text, jsonStart);
      if (!balanced) continue;
      try {
        const args = JSON.parse(balanced.json);
        toolCalls.push({ id: crypto.randomUUID(), name, arguments: args });
      } catch { /* skip */ }
    }
    if (toolCalls.length > 0) return toolCalls;

    // Try closed-tag format: [TOOL_CALLS]...[/TOOL_CALLS]
    const closedRegex = /\[TOOL_CALLS\]([\s\S]*?)\[\/TOOL_CALLS\]/g;
    while ((match = closedRegex.exec(text)) !== null) {
      const body = match[1].trim();
      // Try comma format: name,{"key":"val"}
      const commaIdx = body.indexOf(',{');
      if (commaIdx > 0) {
        const name = body.slice(0, commaIdx).trim();
        try {
          const args = JSON.parse(body.slice(commaIdx + 1));
          if (name && !name.includes(' ')) {
            toolCalls.push({ id: crypto.randomUUID(), name, arguments: args });
            continue;
          }
        } catch { /* fall through */ }
      }
      // Try JSON object/array format
      try {
        const parsed = JSON.parse(body);
        const items = Array.isArray(parsed) ? parsed : [parsed];
        for (const item of items) {
          if (item.name) {
            toolCalls.push({ id: crypto.randomUUID(), name: item.name, arguments: item.arguments || {} });
          }
        }
      } catch { /* skip */ }
    }

    return toolCalls;
  },
};

/**
 * Llama 3 tool parser
 * Format: <function=name>{"arg": "value"}</function>
 */
const llama3Parser: ToolParser = {
  detect(text: string): boolean {
    return /<function=/.test(text);
  },

  parse(text: string): ToolCall[] {
    const toolCalls: ToolCall[] = [];

    // Regex to match <function=name>{...}</function>
    // Use [\s\S]*? to properly handle nested JSON
    const regex = /<function=([^>]+)>([\s\S]*?)<\/function>/g;
    let match;

    while ((match = regex.exec(text)) !== null) {
      const functionName = match[1].trim();
      const argsJson = match[2].trim();

      try {
        const args = JSON.parse(argsJson);
        toolCalls.push({
          id: crypto.randomUUID(),
          name: functionName,
          arguments: args,
        });
      } catch {
        // Fall back to XML parameter format: <parameter=key>value</parameter>
        const params: Record<string, unknown> = {};
        const paramRegex = /<parameter=([^>]+)>([\s\S]*?)<\/parameter>/g;
        let pm;
        while ((pm = paramRegex.exec(argsJson)) !== null) {
          params[pm[1].trim()] = pm[2].trim();
        }
        if (Object.keys(params).length > 0) {
          toolCalls.push({
            id: crypto.randomUUID(),
            name: functionName,
            arguments: params,
          });
        }
      }
    }

    return toolCalls;
  },
};

/**
 * Qwen tool parser
 * Format: <tool_call>{"name": "func", "arguments": {...}}</tool_call>
 */
const qwenParser: ToolParser = {
  detect(text: string): boolean {
    return text.includes('<tool_call>');
  },

  parse(text: string): ToolCall[] {
    const toolCalls: ToolCall[] = [];

    // Regex to match <tool_call>{...}</tool_call> with proper JSON handling
    // Use [\s\S]*? for non-greedy match of any character including newlines
    const regex = /<tool_call>([\s\S]*?)<\/tool_call>/g;
    let match;

    while ((match = regex.exec(text)) !== null) {
      const callJson = match[1].trim();

      try {
        const call = JSON.parse(callJson);
        toolCalls.push({
          id: crypto.randomUUID(),
          name: call.name,
          arguments: call.arguments || {},
        });
      } catch (e) {
        console.error('Failed to parse tool call:', e, callJson);
      }
    }

    return toolCalls;
  },
};

/**
 * Parser registry mapping tool formats to parsers
 */
const parserRegistry: Record<ToolFormat, ToolParser | null> = {
  mistral: mistralParser,
  llama3: llama3Parser,
  qwen: qwenParser,
  openai: null, // OpenAI uses structured API responses, not text parsing
  unknown: null,
};

/**
 * Parse tool calls from model output
 */
export function parseToolCalls(text: string, format: ToolFormat): ToolCall[] {
  const parser = parserRegistry[format];

  if (!parser) {
    return [];
  }

  if (!parser.detect(text)) {
    return [];
  }

  return parser.parse(text);
}

/**
 * Auto-detect and parse tool calls from text
 * Tries all parsers until one matches
 */
export function autoParseToolCalls(text: string): ToolCall[] {
  const parsers = [mistralParser, llama3Parser, qwenParser];

  for (const parser of parsers) {
    if (parser.detect(text)) {
      return parser.parse(text);
    }
  }

  return [];
}

/**
 * Remove tool call markers from text to get clean content
 */
export function stripToolCalls(text: string): string {
  return text
    .replace(/\[TOOL_CALLS\]\w+\[ARGS\]\{[\s\S]*?\}/g, '') // Mistral v2 bracket calls
    .replace(/\[TOOL_CALLS\][\s\S]*?\[\/TOOL_CALLS\]/g, '') // Mistral calls
    .replace(/\[TOOL_RESULTS\][\s\S]*?\[\/TOOL_RESULTS\]/g, '') // Mistral results
    .replace(/<function=[^>]+>[\s\S]*?<\/function>/g, '') // Llama3
    .replace(/<tool_call>[\s\S]*?<\/tool_call>/g, '') // Qwen tool calls
    .replace(/<tool_response>[\s\S]*?<\/tool_response>/g, '') // Qwen tool responses
    .trim();
}
