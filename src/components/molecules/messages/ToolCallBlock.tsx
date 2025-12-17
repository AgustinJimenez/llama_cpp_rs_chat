import React from 'react';
import type { ToolCall } from '../../../types';

interface ToolCallBlockProps {
  toolCalls: ToolCall[];
}

/**
 * Format tool call arguments for display.
 */
function formatToolArguments(args: Record<string, unknown> | string): string {
  if (typeof args === 'string') {
    return args;
  }

  const lines: string[] = ['{'];
  const entries = Object.entries(args);

  entries.forEach(([key, value], index) => {
    const isLast = index === entries.length - 1;

    if (typeof value === 'string') {
      // Unescape the string value for display
      const unescaped = value
        .replace(/\\n/g, '\n')
        .replace(/\\"/g, '"')
        .replace(/\\t/g, '\t')
        .replace(/\\\\/g, '\\');

      // For multiline content, display it nicely
      if (unescaped.includes('\n')) {
        lines.push(`  "${key}":`);
        lines.push(unescaped.split('\n').map(line => `    ${line}`).join('\n'));
      } else {
        lines.push(`  "${key}": "${unescaped}"${isLast ? '' : ','}`);
      }
    } else {
      lines.push(`  "${key}": ${JSON.stringify(value)}${isLast ? '' : ','}`);
    }
  });

  lines.push('}');
  return lines.join('\n');
}

/**
 * Display tool calls with their arguments.
 */
export const ToolCallBlock: React.FC<ToolCallBlockProps> = ({ toolCalls }) => {
  if (toolCalls.length === 0) return null;

  return (
    <div className="space-y-3">
      {toolCalls.map((toolCall) => (
        <div
          key={toolCall.id}
          className="p-3 bg-flat-purple rounded-lg"
        >
          <div className="flex items-center gap-2 mb-2">
            <span className="text-xs font-medium text-white">🔧 Tool Call</span>
            <span className="text-xs font-medium text-white">{toolCall.name}</span>
          </div>
          <pre className="text-xs text-white bg-black/20 p-3 rounded overflow-x-auto whitespace-pre-wrap">
            {formatToolArguments(toolCall.arguments)}
          </pre>
        </div>
      ))}
    </div>
  );
};
