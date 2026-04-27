import { ChevronDown, ChevronRight, Square } from 'lucide-react';
import React, { useState, useRef, useEffect, useCallback } from 'react';

const TOOL_SUMMARY_MAX_LENGTH = 80;
const DEACTIVATE_DEBOUNCE_MS = 2000;
const SCROLL_BOTTOM_THRESHOLD_PX = 30;
const SYNTAX_HIGHLIGHT_MAX_LENGTH = 50000;

import type { ToolCall } from '../../../types';
import { detectLanguageFromPath } from '../../../utils/languageDetect';
import { SyntaxHighlighter, dracula } from '../../../utils/syntaxHighlighterSetup';

/** Desktop automation tool names that can be aborted via /api/desktop/abort. */
const DESKTOP_TOOLS = new Set([
  'click_screen',
  'type_text',
  'press_key',
  'move_mouse',
  'scroll_screen',
  'mouse_drag',
  'mouse_button',
  'paste',
  'clear_field',
  'hover_element',
  'screenshot_region',
  'screenshot_diff',
  'window_screenshot',
  'wait_for_screen_change',
  'ocr_screen',
  'ocr_find_text',
  'ocr_region',
  'get_ui_tree',
  'click_ui_element',
  'invoke_ui_action',
  'read_ui_element_value',
  'wait_for_ui_element',
  'find_ui_elements',
  'focus_window',
  'click_window_relative',
  'snap_window',
  'set_window_topmost',
  'open_application',
  'send_keys_to_window',
  'find_and_click_text',
  'type_into_element',
  'file_dialog_navigate',
  'drag_and_drop_element',
  'wait_for_text_on_screen',
  'scroll_element',
  'click_and_verify',
  'fill_form',
  'run_action_sequence',
  'mouse_drag',
  'handle_dialog',
  'wait_for_element_state',
]);

interface ToolCallBlockProps {
  toolCalls: ToolCall[];
  /** When false, override any streaming/pending flags (generation was stopped). */
  isGenerating?: boolean;
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
  edit_file: (args) => String(args.path || ''),
  undo_edit: (args) => String(args.path || ''),
  insert_text: (args) => `${args.path}:${args.line}`,
  search_files: (args) => {
    const pat = String(args.pattern || '');
    const path = args.path ? ` in ${args.path}` : '';
    return `"${pat}"${path}`;
  },
  find_files: (args) => {
    const pat = String(args.pattern || '');
    const path = args.path ? ` in ${args.path}` : '';
    return `${pat}${path}`;
  },
  execute_python: (args) => {
    const code = String(args.code || '');
    const firstLine = code.split('\n')[0].trim();
    const lineCount = code.split('\n').length;
    return lineCount > 1 ? `${firstLine} ... (${lineCount} lines)` : firstLine;
  },
  execute_command: (args) => String(args.command || ''),
  list_directory: (args) => String(args.path || '.'),
  git_status: (args) => String(args.path || '.'),
  git_diff: (args) => {
    const path = args.path ? String(args.path) : '';
    const staged = args.staged ? ' (staged)' : '';
    return `${path}${staged}` || '.';
  },
  git_commit: (args) => {
    const msg = String(args.message || '');
    return msg.length > 60 ? `${msg.slice(0, 60)}...` : msg;
  },
};

function formatParamValue(key: string, val: unknown): string {
  if (key === 'summary') return val === false || val === 'false' || val === 'no' ? 'No' : 'Yes';
  if (typeof val === 'boolean') return val ? 'Yes' : 'No';
  if (typeof val === 'string') return val;
  return JSON.stringify(val);
}

function defaultToolSummary(args: Record<string, unknown>): string {
  // Filter out the 'summary' param when other params exist (it's noise)
  const entries = Object.entries(args).filter(
    ([k]) => !(k === 'summary' && Object.keys(args).length > 1),
  );
  if (entries.length === 0) return '';
  const [key, val] = entries[0];
  const valStr = formatParamValue(key, val);
  return `${key}: ${valStr.slice(0, 60)}${valStr.length > 60 ? '...' : ''}`;
}

function formatToolName(name: string): string {
  return name
    .split('_')
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(' ');
}

function getToolSummary(name: string, args: Record<string, unknown> | string): string {
  if (typeof args === 'string') return args.slice(0, TOOL_SUMMARY_MAX_LENGTH);
  const summarizer = TOOL_SUMMARIZERS[name];
  return summarizer ? summarizer(args) : defaultToolSummary(args);
}

/** Track elapsed time while a condition is active, with debounced deactivation. */
function useElapsedTime(isActive: boolean): number {
  const startRef = React.useRef<number | null>(null);
  const [elapsed, setElapsed] = React.useState(0);
  const deactivateTimer = React.useRef<ReturnType<typeof setTimeout> | null>(null);

  React.useEffect(() => {
    if (isActive) {
      // Cancel any pending deactivation
      if (deactivateTimer.current !== null) {
        clearTimeout(deactivateTimer.current);
        deactivateTimer.current = null;
      }
      if (startRef.current === null) {
        startRef.current = Date.now();
      }
      const id = setInterval(() => {
        if (startRef.current !== null) {
          setElapsed(Math.floor((Date.now() - startRef.current) / 1000));
        }
      }, 1000);
      return () => clearInterval(id);
    } else if (startRef.current !== null) {
      // Debounce: wait before resetting to survive brief flickers
      deactivateTimer.current = setTimeout(() => {
        startRef.current = null;
        setElapsed(0);
        deactivateTimer.current = null;
      }, DEACTIVATE_DEBOUNCE_MS);
      return () => {
        if (deactivateTimer.current !== null) {
          clearTimeout(deactivateTimer.current);
          deactivateTimer.current = null;
        }
      };
    }
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
        lines.push(
          unescaped
            .split('\n')
            .map((line) => `    ${line}`)
            .join('\n'),
        );
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

const ExecutingHeader: React.FC<{
  name: string;
  summary: string;
  hasOutput: boolean;
  elapsed: number;
  isExpanded: boolean;
  onToggle: () => void;
}> = ({ name, summary, elapsed, isExpanded, onToggle }) => {
  const elapsedStr = formatElapsed(elapsed);
  const isDesktop = DESKTOP_TOOLS.has(name);
  return (
    <div className="w-full bg-muted px-3 py-2 flex items-center gap-2 hover:bg-accent transition-colors">
      <button onClick={onToggle} className="flex items-center gap-2 text-left flex-1 min-w-0">
        <span className="inline-block w-3 h-3 border-2 border-foreground/50 border-t-transparent rounded-full animate-spin flex-shrink-0" />
        <span className="text-xs font-medium text-foreground whitespace-nowrap">
          {formatToolName(name)}
          {elapsedStr ? ` (${elapsedStr})` : null}
        </span>
        <span className="text-xs text-foreground truncate flex-1">{summary}</span>
        {isExpanded ? (
          <ChevronDown className="h-3.5 w-3.5 text-foreground flex-shrink-0" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5 text-foreground flex-shrink-0" />
        )}
      </button>
      {isDesktop ? (
        <button
          onClick={(e) => {
            e.stopPropagation();
            fetch('/api/desktop/abort', { method: 'POST' }).catch(() => {});
          }}
          className="flex items-center gap-1 px-2 py-0.5 rounded text-[10px] font-medium bg-destructive/20 hover:bg-destructive/40 text-destructive transition-colors flex-shrink-0"
          title="Abort desktop automation"
        >
          <Square className="h-2.5 w-2.5 fill-current" />
          Abort
        </button>
      ) : null}
    </div>
  );
};

const CompletedHeader: React.FC<{
  name: string;
  summary: string;
  isExpanded: boolean;
  onToggle: () => void;
  resultStatus?: 'success' | 'error';
}> = ({ name, summary, isExpanded, onToggle, resultStatus }) => (
  <button
    onClick={onToggle}
    className={`w-full px-3 py-2 flex items-center gap-2 text-left hover:bg-accent transition-colors ${
      resultStatus === 'error' ? 'bg-red-500/10 border-l-2 border-red-500' : 'bg-muted'
    }`}
  >
    {resultStatus ? (
      <span
        className={`w-2 h-2 rounded-full flex-shrink-0 ${resultStatus === 'error' ? 'bg-red-500' : 'bg-green-500'}`}
        title={resultStatus === 'error' ? 'Tool call returned an error' : 'Tool call succeeded'}
      />
    ) : null}
    <span className="text-xs font-medium text-foreground">{formatToolName(name)}</span>
    <span className="text-xs text-foreground truncate flex-1">{summary}</span>
    {isExpanded ? (
      <ChevronDown className="h-3.5 w-3.5 text-foreground flex-shrink-0" />
    ) : (
      <ChevronRight className="h-3.5 w-3.5 text-foreground flex-shrink-0" />
    )}
  </button>
);

/** Scrollable output container with overscroll containment and streaming auto-scroll. */
const ScrollableOutput: React.FC<{
  output: string;
  isStreaming?: boolean;
  language?: string | null;
}> = ({ output, isStreaming, language }) => {
  const scrollRef = useRef<HTMLElement>(null);
  const userScrolledRef = useRef(false);

  // Auto-scroll to bottom during streaming, unless user scrolled up
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || !isStreaming || userScrolledRef.current) return;
    el.scrollTop = el.scrollHeight;
  }, [output, isStreaming]);

  // Reset user-scrolled flag when streaming starts
  useEffect(() => {
    if (isStreaming) userScrolledRef.current = false;
  }, [isStreaming]);

  const handleScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el || !isStreaming) return;
    const distFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    userScrolledRef.current = distFromBottom > SCROLL_BOTTOM_THRESHOLD_PX;
  }, [isStreaming]);

  const containClass = 'border-t border-border overscroll-contain';

  // Lock height during streaming to prevent layout shifts
  const heightClass = isStreaming ? 'h-64 overflow-y-auto' : 'max-h-64 overflow-y-auto';

  if (language && output.length < SYNTAX_HIGHLIGHT_MAX_LENGTH) {
    return (
      <div
        ref={scrollRef as React.RefObject<HTMLDivElement>}
        onScroll={handleScroll}
        className={`${containClass} ${heightClass}`}
      >
        <SyntaxHighlighter
          style={dracula}
          language={language}
          customStyle={{
            margin: 0,
            padding: '0.5rem 0.75rem',
            fontSize: '0.75rem',
            background: 'transparent',
          }}
        >
          {output}
        </SyntaxHighlighter>
      </div>
    );
  }
  return (
    <pre
      ref={scrollRef as React.RefObject<HTMLPreElement>}
      onScroll={handleScroll}
      className={`text-xs text-foreground font-mono bg-card px-3 py-2 overflow-x-auto whitespace-pre-wrap ${containClass} ${heightClass}`}
    >
      {output}
    </pre>
  );
};

const CompletedOutput: React.FC<{
  output: string;
  isExpanded: boolean;
  onToggle: () => void;
  language?: string | null;
  isStreaming?: boolean;
}> = ({ output, isExpanded, onToggle, language, isStreaming }) => (
  <>
    <button
      onClick={onToggle}
      className="w-full bg-muted px-3 py-1.5 flex items-center gap-2 text-left hover:bg-accent transition-colors border-t border-border"
    >
      <span className="text-xs font-medium text-foreground">Output</span>
      <span className="text-xs text-foreground truncate flex-1">
        {(() => {
          if (isStreaming) return 'Streaming...';
          if (output.length > TOOL_SUMMARY_MAX_LENGTH) {
            return `${output.slice(0, TOOL_SUMMARY_MAX_LENGTH)}...`;
          }
          return output;
        })()}
      </span>
      {isExpanded ? (
        <ChevronDown className="h-3.5 w-3.5 text-foreground flex-shrink-0" />
      ) : (
        <ChevronRight className="h-3.5 w-3.5 text-foreground flex-shrink-0" />
      )}
    </button>
    {isExpanded ? (
      <ScrollableOutput output={output} isStreaming={isStreaming} language={language} />
    ) : null}
  </>
);

/**
 * Renders a colored inline diff for edit_file tool calls.
 * Red lines = removed (old_string), green lines = added (new_string).
 */
const EditFileDiff: React.FC<{ args: Record<string, unknown> }> = ({ args }) => {
  const oldStr = String(args.old_string || '');
  const newStr = String(args.new_string || '');
  const oldLines = oldStr.split('\n');
  const newLines = newStr.split('\n');

  return (
    <pre className="text-xs font-mono bg-card px-3 py-2 overflow-x-auto whitespace-pre-wrap max-h-64 overflow-y-auto border-t border-border overscroll-contain">
      <div className="text-muted-foreground mb-1">{String(args.path || '')}</div>
      {oldLines.map((line) => {
        const lineKey = `old-${line.length}-${line}`;
        return (
          <div
            key={lineKey}
            style={{ backgroundColor: 'rgba(248, 81, 73, 0.15)', color: '#f85149' }}
          >
            {'- '}
            {line}
          </div>
        );
      })}
      {newLines.map((line) => {
        const lineKey = `new-${line.length}-${line}`;
        return (
          <div
            key={lineKey}
            style={{ backgroundColor: 'rgba(63, 185, 80, 0.15)', color: '#3fb950' }}
          >
            {'+ '}
            {line}
          </div>
        );
      })}
    </pre>
  );
};

/** Detect syntax highlighting language for a tool's output. */
function getOutputLanguage(name: string, args: Record<string, unknown> | string): string | null {
  if (typeof args !== 'object') return null;
  if (name === 'read_file') return detectLanguageFromPath(String(args.path || ''));
  if (name === 'git_diff') return 'diff';
  return null;
}

const SingleToolCall: React.FC<{ toolCall: ToolCall; isGenerating?: boolean }> = ({
  toolCall,
  isGenerating,
}) => {
  const [isExpanded, setIsExpanded] = useState(false);
  const [isOutputExpanded, setIsOutputExpanded] = useState(false);
  const summary = getToolSummary(toolCall.name, toolCall.arguments);
  // If generation is stopped, tool calls can't be executing anymore
  const isExecuting =
    isGenerating !== false &&
    (toolCall.isStreaming === true || (toolCall.isPending === true && !toolCall.output));
  const elapsed = useElapsedTime(isExecuting);
  const outputLanguage = getOutputLanguage(toolCall.name, toolCall.arguments);

  // Parse [TOOL_RESULT:success/error] tag from output
  const toolResultMatch = toolCall.output?.match(/^\[TOOL_RESULT:(success|error)\]/);
  const toolResultStatus = toolResultMatch?.[1] as 'success' | 'error' | undefined;
  const cleanOutput = toolResultMatch
    ? (toolCall.output ?? '').slice(toolResultMatch[0].length)
    : toolCall.output;
  const hasOutput = !!cleanOutput && cleanOutput.length > 0;
  const isEditFile = toolCall.name === 'edit_file' && typeof toolCall.arguments === 'object';
  const expandedContent = isEditFile ? (
    <EditFileDiff args={toolCall.arguments as Record<string, unknown>} />
  ) : (
    <pre className="text-xs text-foreground font-mono bg-card px-3 py-2 overflow-x-auto whitespace-pre-wrap max-h-64 overflow-y-auto overscroll-contain">
      {formatToolArguments(toolCall.arguments)}
    </pre>
  );

  return (
    <div className="flat-card overflow-hidden" style={{ contain: 'content' }}>
      {isExecuting ? (
        <ExecutingHeader
          name={toolCall.name}
          summary={summary}
          hasOutput={!!toolCall.output}
          elapsed={elapsed}
          isExpanded={isExpanded}
          onToggle={() => setIsExpanded(!isExpanded)}
        />
      ) : (
        <CompletedHeader
          name={toolCall.name}
          summary={summary}
          isExpanded={isExpanded}
          onToggle={() => setIsExpanded(!isExpanded)}
          resultStatus={toolResultStatus}
        />
      )}
      {isExpanded ? expandedContent : null}
      {hasOutput && (cleanOutput ?? '').trim().length > 0 ? (
        <CompletedOutput
          output={cleanOutput ?? ''}
          isExpanded={isOutputExpanded}
          onToggle={() => setIsOutputExpanded(!isOutputExpanded)}
          language={outputLanguage}
          isStreaming={isExecuting}
        />
      ) : null}
    </div>
  );
};

/**
 * Compact tool call display with expandable details.
 * Shows executing/streaming state for in-progress tool calls.
 */
export const ToolCallBlock = React.memo(({ toolCalls, isGenerating }: ToolCallBlockProps) => {
  if (toolCalls.length === 0) return null;

  return (
    <div className="space-y-2">
      {toolCalls.map((toolCall) => (
        <SingleToolCall key={toolCall.id} toolCall={toolCall} isGenerating={isGenerating} />
      ))}
    </div>
  );
});
ToolCallBlock.displayName = 'ToolCallBlock';
