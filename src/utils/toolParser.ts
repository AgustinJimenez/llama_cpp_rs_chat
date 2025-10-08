import type { ToolCall, ToolFormat } from '../types';

/**
 * Tool parser interface for different model formats
 */
interface ToolParser {
  detect(text: string): boolean;
  parse(text: string): ToolCall[];
}

/**
 * Mistral/Devstral tool parser
 * Format: [TOOL_CALLS]function_name[ARGS]{"arg": "value"}
 */
const mistralParser: ToolParser = {
  detect(text: string): boolean {
    return text.includes('[TOOL_CALLS]');
  },

  parse(text: string): ToolCall[] {
    const toolCalls: ToolCall[] = [];

    // Regex to match [TOOL_CALLS]function_name[ARGS]{...}
    const regex = /\[TOOL_CALLS\]([^\[]+)\[ARGS\](\{[^\}]*\})/g;
    let match;

    while ((match = regex.exec(text)) !== null) {
      const functionName = match[1].trim();
      const argsJson = match[2];

      try {
        const args = JSON.parse(argsJson);
        toolCalls.push({
          id: crypto.randomUUID(),
          name: functionName,
          arguments: args,
        });
      } catch (e) {
        console.error('Failed to parse tool call arguments:', e, argsJson);
      }
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
    const regex = /<function=([^>]+)>(\{[^\}]*\})<\/function>/g;
    let match;

    while ((match = regex.exec(text)) !== null) {
      const functionName = match[1].trim();
      const argsJson = match[2];

      try {
        const args = JSON.parse(argsJson);
        toolCalls.push({
          id: crypto.randomUUID(),
          name: functionName,
          arguments: args,
        });
      } catch (e) {
        console.error('Failed to parse tool call arguments:', e, argsJson);
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

    // Regex to match <tool_call>{...}</tool_call>
    const regex = /<tool_call>(\{[^\}]*\})<\/tool_call>/g;
    let match;

    while ((match = regex.exec(text)) !== null) {
      const callJson = match[1];

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
    .replace(/\[TOOL_CALLS\][^\[]+\[ARGS\]\{[^\}]*\}/g, '') // Mistral
    .replace(/<function=[^>]+>\{[^\}]*\}<\/function>/g, '') // Llama3
    .replace(/<tool_call>\{[^\}]*\}<\/tool_call>/g, '') // Qwen
    .trim();
}
