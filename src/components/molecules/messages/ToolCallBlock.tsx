import React, { useState } from 'react';
import type { ToolCall } from '../../../types';

interface ToolCallBlockProps {
  toolCalls: ToolCall[];
}

/**
 * Get a brief one-line summary for a tool call based on its name and arguments.
 */
const TOOL_SUMMARIZERS: Record<string, (args: Record<string, unknown>) => string> = {
  read_file: (args) => String(args.path || ''),
  write_file: (args) => {
    const content = String(args.content || '');
    return `${args.path} (${content.length} chars)`;
  },
  execute_python: (args) => {
    const code = String(args.code || '');
    const firstLine = code.split('\n')[0].trim();
    const lineCount = code.split('\n').length;
    return lineCount > 1 ? `${firstLine} ... (${lineCount} lines)` : firstLine;
  },
  execute_command: (args) => String(args.command || ''),
  list_directory: (args) => String(args.path || '.'),
};

function defaultToolSummary(args: Record<string, unknown>): string {
  const entries = Object.entries(args);
  if (entries.length === 0) return '';
  const [key, val] = entries[0];
  const valStr = typeof val === 'string' ? val : JSON.stringify(val);
  return `${key}: ${valStr.slice(0, 60)}${valStr.length > 60 ? '...' : ''}`;
}

function formatToolName(name: string): string {
  return name
    .split('_')
    .map(word => word.charAt(0).toUpperCase() + word.slice(1))
    .join(' ');
}

function getToolSummary(name: string, args: Record<string, unknown> | string): string {
  if (typeof args === 'string') return args.slice(0, 80);
  const summarizer = TOOL_SUMMARIZERS[name];
  return summarizer ? summarizer(args) : defaultToolSummary(args);
}

/**
 * Format tool call arguments for the expanded detail view.
 */
function formatToolArguments(args: Record<string, unknown> | string): string {
  if (typeof args === 'string') return args;

  const lines: string[] = ['{'];
  const entries = Object.entries(args);

  entries.forEach(([key, value], index) => {
    const isLast = index === entries.length - 1;

    if (typeof value === 'string') {
      const unescaped = value
        .replace(/\\n/g, '\n')
        .replace(/\\"/g, '"')
        .replace(/\\t/g, '\t')
        .replace(/\\\\/g, '\\');

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
 * Compact tool call display with expandable details.
 * Uses the same green theme as CommandExecBlock.
 */
export const ToolCallBlock: React.FC<ToolCallBlockProps> = ({ toolCalls }) => {
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());
  const [expandedOutputIds, setExpandedOutputIds] = useState<Set<string>>(new Set());

  if (toolCalls.length === 0) return null;

  const toggleExpand = (id: string) => {
    setExpandedIds(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleOutput = (id: string) => {
    setExpandedOutputIds(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  return (
    <div className="space-y-2">
      {toolCalls.map((toolCall) => {
        const isExpanded = expandedIds.has(toolCall.id);
        const isOutputExpanded = expandedOutputIds.has(toolCall.id);
        const summary = getToolSummary(toolCall.name, toolCall.arguments);

        return (
          <div
            key={toolCall.id}
            className="rounded-xl overflow-hidden"
            style={{ border: '1px solid hsl(220 8% 28%)' }}
          >
            {/* Tool call header */}
            <button
              onClick={() => toggleExpand(toolCall.id)}
              className="w-full bg-muted px-3 py-2 flex items-center gap-2 text-left hover:bg-accent transition-colors"
            >
              <span className="text-xs font-medium text-foreground">{formatToolName(toolCall.name)}</span>
              <span className="text-xs text-muted-foreground truncate flex-1">{summary}</span>
              <span className="text-muted-foreground">{isExpanded ? '\u25BC' : '\u25C0'}</span>
            </button>
            {/* Expanded arguments */}
            {isExpanded && (
              <pre className="text-xs text-foreground font-mono bg-card px-3 py-2 overflow-x-auto whitespace-pre-wrap max-h-64 overflow-y-auto">
                {formatToolArguments(toolCall.arguments)}
              </pre>
            )}
            {/* Tool output section */}
            {toolCall.output && (
              <>
                <button
                  onClick={() => toggleOutput(toolCall.id)}
                  className="w-full bg-muted px-3 py-1.5 flex items-center gap-2 text-left hover:bg-accent transition-colors"
                  style={{ borderTop: '1px solid hsl(220 8% 28%)' }}
                >
                  <span className="text-xs font-medium text-foreground">Output</span>
                  <span className="text-xs text-muted-foreground truncate flex-1">
                    {toolCall.output.length > 80 ? `${toolCall.output.slice(0, 80)}...` : toolCall.output}
                  </span>
                  <span className="text-muted-foreground">{isOutputExpanded ? '\u25BC' : '\u25C0'}</span>
                </button>
                {isOutputExpanded && (
                  <pre className="text-xs text-foreground font-mono bg-card px-3 py-2 overflow-x-auto whitespace-pre-wrap max-h-64 overflow-y-auto"
                    style={{ borderTop: '1px solid hsl(220 8% 28%)' }}>
                    {toolCall.output}
                  </pre>
                )}
              </>
            )}
          </div>
        );
      })}
    </div>
  );
};
