import type { ToolCall, ToolFormat } from '../types';
import { extractBalancedJson } from './toolFormatUtils';

/**
 * Tool parser interface for different model formats
 */
interface ToolParser {
  detect(text: string): boolean;
  parse(text: string): ToolCall[];
}

/**
 * Mistral/Devstral tool parser
 * Bracket format: [TOOL_CALLS]function_name[ARGS]{"arg": "value"}
 * Comma format:   [TOOL_CALLS]function_name,{"arg": "value"}[/TOOL_CALLS]
 * JSON format:    [TOOL_CALLS][{"name":"func","arguments":{...}}][/TOOL_CALLS]
 * Bare JSON:      [TOOL_CALLS]{"name":"func","arguments":{...}}  (Magistral)
 */

/** Parse bracket format: [TOOL_CALLS]name[ARGS]{json} */
function parseMistralBracket(text: string): ToolCall[] {
  const calls: ToolCall[] = [];
  const re = /\[TOOL_CALLS\](\w+)\[ARGS\]/g;
  let match;
  while ((match = re.exec(text)) !== null) {
    const balanced = extractBalancedJson(text, match.index + match[0].length);
    if (!balanced) continue;
    try {
      calls.push({ id: crypto.randomUUID(), name: match[1].trim(), arguments: JSON.parse(balanced.json) });
    } catch { /* skip */ }
  }
  return calls;
}

/** Parse closed-tag format: [TOOL_CALLS]...[/TOOL_CALLS] (comma or JSON body) */
function parseMistralClosedTag(text: string): ToolCall[] {
  const calls: ToolCall[] = [];
  const re = /\[TOOL_CALLS\]([\s\S]*?)\[\/TOOL_CALLS\]/g;
  let match;
  while ((match = re.exec(text)) !== null) {
    const body = match[1].trim();
    const commaIdx = body.indexOf(',{');
    if (commaIdx > 0) {
      const name = body.slice(0, commaIdx).trim();
      try {
        const args = JSON.parse(body.slice(commaIdx + 1));
        if (name && !name.includes(' ')) { calls.push({ id: crypto.randomUUID(), name, arguments: args }); continue; }
      } catch { /* fall through */ }
    }
    try {
      const parsed = JSON.parse(body);
      const items = Array.isArray(parsed) ? parsed : [parsed];
      for (const item of items) {
        if (item.name) calls.push({ id: crypto.randomUUID(), name: item.name, arguments: item.arguments || {} });
      }
    } catch { /* skip */ }
  }
  return calls;
}

/** Parse bare JSON format: [TOOL_CALLS]{"name":"...","arguments":{...}} (Magistral) */
function parseMistralBareJson(text: string): ToolCall[] {
  const calls: ToolCall[] = [];
  const re = /\[TOOL_CALLS\]\s*\{/g;
  let match;
  while ((match = re.exec(text)) !== null) {
    const balanced = extractBalancedJson(text, match.index + match[0].length - 1);
    if (!balanced) continue;
    try {
      const parsed = JSON.parse(balanced.json);
      if (parsed.name) calls.push({ id: crypto.randomUUID(), name: parsed.name, arguments: parsed.arguments || {} });
    } catch { /* skip */ }
  }
  return calls;
}

const mistralParser: ToolParser = {
  detect(text: string): boolean {
    return text.includes('[TOOL_CALLS]');
  },

  parse(text: string): ToolCall[] {
    const bracket = parseMistralBracket(text);
    if (bracket.length > 0) return bracket;
    const closed = parseMistralClosedTag(text);
    if (closed.length > 0) return closed;
    return parseMistralBareJson(text);
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
 * GLM sometimes confuses <|begin_of_box|> (output wrapper) with <tool_call> — accept both.
 */
const qwenParser: ToolParser = {
  detect(text: string): boolean {
    return text.includes('<tool_call>') || text.includes('<|begin_of_box|>');
  },

  parse(text: string): ToolCall[] {
    const toolCalls: ToolCall[] = [];

    // Regex to match <tool_call>{...}</tool_call> or <tool_call>{...}<|end_of_box|> (GLM)
    // Also matches <|begin_of_box|>{...}<|end_of_box|> for GLM's confused tag usage.
    // False positives (output text) are filtered by JSON.parse + name check below.
    const regex = /(?:<tool_call>|<\|begin_of_box\|>)([\s\S]*?)(?:<\/tool_call>|<\|end_of_box\|>)/g;
    let match;

    while ((match = regex.exec(text)) !== null) {
      const callJson = match[1].trim();

      try {
        const parsed = JSON.parse(callJson);
        // Handle JSON arrays: multiple tool calls in one block
        const items = Array.isArray(parsed) ? parsed : [parsed];
        for (const call of items) {
          if (call?.name) {
            toolCalls.push({
              id: crypto.randomUUID(),
              name: call.name,
              arguments: call.arguments || {},
            });
          }
        }
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
    .replace(/<tool_call>[\s\S]*?(?:<\/tool_call>|<\|end_of_box\|>)/g, '') // Qwen/GLM tool calls
    .replace(/(?:<tool_response>|<\|begin_of_box\|>)[\s\S]*?(?:<\/tool_response>|<\|end_of_box\|>)/g, '') // Qwen/GLM tool responses
    .trim();
}
