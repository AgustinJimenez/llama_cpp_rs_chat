import React, { useState } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
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

/** Track elapsed time while a condition is active. */
function useElapsedTime(isActive: boolean): number {
  const startRef = React.useRef<number | null>(null);
  const [elapsed, setElapsed] = React.useState(0);

  React.useEffect(() => {
    if (isActive && startRef.current === null) {
      startRef.current = Date.now();
    }
    if (!isActive) {
      startRef.current = null;
      setElapsed(0);
      return;
    }
    const id = setInterval(() => {
      if (startRef.current !== null) {
        setElapsed(Math.floor((Date.now() - startRef.current) / 1000));
      }
    }, 1000);
    return () => clearInterval(id);
  }, [isActive]);

  return elapsed;
}

function formatElapsed(seconds: number): string {
  if (seconds <= 0) return '';
  if (seconds < 60) return `${seconds}s`;
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return `${m}m ${s}s`;
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

const ExecutingHeader: React.FC<{ name: string; summary: string; hasOutput: boolean; elapsed: number }> = ({ name, summary, elapsed }) => {
  const elapsedStr = formatElapsed(elapsed);
  return (
    <div className="w-full bg-muted px-3 py-2 flex items-center gap-2">
      <span className="inline-block w-3 h-3 border-2 border-foreground/50 border-t-transparent rounded-full animate-spin flex-shrink-0" />
      <span className="text-xs font-medium text-foreground">
        {formatToolName(name)}{elapsedStr ? ` (${elapsedStr})` : null}
      </span>
      <span className="text-xs text-foreground truncate flex-1">{summary}</span>
    </div>
  );
};


const CompletedHeader: React.FC<{
  name: string; summary: string; isExpanded: boolean; onToggle: () => void;
}> = ({ name, summary, isExpanded, onToggle }) => (
  <button
    onClick={onToggle}
    className="w-full bg-muted px-3 py-2 flex items-center gap-2 text-left hover:bg-accent transition-colors"
  >
    <span className="text-xs font-medium text-foreground">{formatToolName(name)}</span>
    <span className="text-xs text-foreground truncate flex-1">{summary}</span>
    {isExpanded ? <ChevronDown className="h-3.5 w-3.5 text-foreground flex-shrink-0" /> : <ChevronRight className="h-3.5 w-3.5 text-foreground flex-shrink-0" />}
  </button>
);

const CompletedOutput: React.FC<{
  output: string; isExpanded: boolean; onToggle: () => void;
}> = ({ output, isExpanded, onToggle }) => (
  <>
    <button
      onClick={onToggle}
      className="w-full bg-muted px-3 py-1.5 flex items-center gap-2 text-left hover:bg-accent transition-colors"
      style={{ borderTop: '1px solid hsl(220 8% 28%)' }}
    >
      <span className="text-xs font-medium text-foreground">Output</span>
      <span className="text-xs text-foreground truncate flex-1">
        {output.length > 80 ? `${output.slice(0, 80)}...` : output}
      </span>
      {isExpanded ? <ChevronDown className="h-3.5 w-3.5 text-foreground flex-shrink-0" /> : <ChevronRight className="h-3.5 w-3.5 text-foreground flex-shrink-0" />}
    </button>
    {isExpanded ? <pre className="text-xs text-foreground font-mono bg-card px-3 py-2 overflow-x-auto whitespace-pre-wrap max-h-64 overflow-y-auto"
        style={{ borderTop: '1px solid hsl(220 8% 28%)' }}>
        {output}
      </pre> : null}
  </>
);

const SingleToolCall: React.FC<{ toolCall: ToolCall }> = ({ toolCall }) => {
  const [isExpanded, setIsExpanded] = useState(false);
  const [isOutputExpanded, setIsOutputExpanded] = useState(false);
  const summary = getToolSummary(toolCall.name, toolCall.arguments);
  const isExecuting = toolCall.isStreaming === true || (toolCall.isPending === true && !toolCall.output);
  const elapsed = useElapsedTime(isExecuting);

  return (
    <div
      className="rounded-xl overflow-hidden"
      style={{ border: '1px solid hsl(220 8% 28%)' }}
    >
      {isExecuting
        ? <ExecutingHeader name={toolCall.name} summary={summary} hasOutput={false} elapsed={elapsed} />
        : <CompletedHeader name={toolCall.name} summary={summary} isExpanded={isExpanded} onToggle={() => setIsExpanded(!isExpanded)} />
      }
      {isExpanded && !isExecuting ? <pre className="text-xs text-foreground font-mono bg-card px-3 py-2 overflow-x-auto whitespace-pre-wrap max-h-64 overflow-y-auto">
          {formatToolArguments(toolCall.arguments)}
        </pre> : null}
      {!isExecuting && toolCall.output ? <CompletedOutput output={toolCall.output} isExpanded={isOutputExpanded} onToggle={() => setIsOutputExpanded(!isOutputExpanded)} /> : null}
    </div>
  );
};

/**
 * Compact tool call display with expandable details.
 * Shows executing/streaming state for in-progress tool calls.
 */
export const ToolCallBlock: React.FC<ToolCallBlockProps> = ({ toolCalls }) => {
  if (toolCalls.length === 0) return null;

  return (
    <div className="space-y-2">
      {toolCalls.map((toolCall) => (
        <SingleToolCall key={toolCall.id} toolCall={toolCall} />
      ))}
    </div>
  );
};
