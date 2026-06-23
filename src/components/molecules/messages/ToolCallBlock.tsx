import { ChevronDown, ChevronRight, Square, BookOpen } from 'lucide-react';
import React, { useState, useRef, useEffect, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';

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
  display_images: (args) => {
    const urls = Array.isArray(args.urls) ? (args.urls as string[]) : [];
    const title = args.title ? ` — ${String(args.title)}` : '';
    return `${urls.length} image${urls.length !== 1 ? 's' : ''}${title}`;
  },
};

function defaultToolSummary(args: Record<string, unknown>): string {
  // Show only the first string value — it identifies what the tool acted on.
  // Skip numeric/boolean config params (e.g. max_chars, timeout) entirely.
  for (const val of Object.values(args)) {
    if (typeof val === 'string' && val.length > 0 && !/^\d+(\.\d+)?$/.test(val)) {
      return val.slice(0, 80) + (val.length > 80 ? '...' : '');
    }
  }
  return '';
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

  // react-doctor-disable-next-line react-doctor/no-cascading-set-state, react-doctor/no-effect-event-handler -- single state, conditional branches
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

  for (const [index, [key, value]] of entries.entries()) {
    const isLast = index === entries.length - 1;

    if (typeof value === 'string') {
      const unescaped = value
        .replaceAll('\\n', '\n')
        .replaceAll('\\"', '"')
        .replaceAll('\\t', '\t')
        .replaceAll('\\\\', '\\');

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
  }

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
  const { t } = useTranslation();
  const elapsedStr = formatElapsed(elapsed);
  const isDesktop = DESKTOP_TOOLS.has(name);
  const execChevron = isExpanded ? (
    <ChevronDown className="size-3.5 flex-shrink-0 text-foreground" />
  ) : (
    <ChevronRight className="size-3.5 flex-shrink-0 text-foreground" />
  );
  return (
    <div className="flex w-full items-center gap-2 bg-muted px-3 py-2 transition-colors hover:bg-accent">
      <button onClick={onToggle} className="flex min-w-0 flex-1 items-center gap-2 text-left">
        <span className="inline-block size-3 flex-shrink-0 animate-spin rounded-full border-2 border-foreground/50 border-t-transparent" />
        <span className="whitespace-nowrap text-xs font-medium text-foreground">
          {formatToolName(name)}
          {!!elapsedStr && ` (${elapsedStr})`}
        </span>
        <span className="flex-1 truncate text-xs text-foreground">{summary}</span>
        {execChevron}
      </button>
      {!!isDesktop && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            fetch('/api/desktop/abort', { method: 'POST' }).catch(() => {});
          }}
          className="flex flex-shrink-0 items-center gap-1 rounded bg-destructive/20 px-2 py-0.5 text-[10px] font-medium text-destructive transition-colors hover:bg-destructive/40"
          title={t('toolCallBlock.abortDesktop')}
        >
          <Square className="size-2.5 fill-current" />
          {t('toolCallBlock.abort')}
        </button>
      )}
    </div>
  );
};

const CompletedHeader: React.FC<{
  name: string;
  summary: string;
  isExpanded: boolean;
  onToggle: () => void;
  resultStatus?: 'success' | 'error';
  durationMs?: number;
}> = ({ name, summary, isExpanded, onToggle, resultStatus, durationMs }) => {
  let durationStr: string | null = null;
  if (durationMs != null) {
    if (durationMs === 0) durationStr = '<1ms';
    else if (durationMs < 1000) durationStr = `${durationMs}ms`;
    else durationStr = `${(durationMs / 1000).toFixed(1)}s`;
  }
  const completedChevron = isExpanded ? (
    <ChevronDown className="size-3.5 flex-shrink-0 text-foreground" />
  ) : (
    <ChevronRight className="size-3.5 flex-shrink-0 text-foreground" />
  );
  const isError = resultStatus === 'error';
  const buttonBgClass = isError ? 'bg-red-500/10 border-l-2 border-red-500' : 'bg-muted';
  const statusDotClass = isError ? 'bg-red-500' : 'bg-green-500';
  const statusTitle = isError ? 'Tool call returned an error' : 'Tool call succeeded';
  return (
    <button
      onClick={onToggle}
      className={`flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-accent ${buttonBgClass}`}
    >
      {!!resultStatus && (
        <span
          className={`size-2 flex-shrink-0 rounded-full ${statusDotClass}`}
          title={statusTitle}
        />
      )}
      <span className="text-xs font-medium text-foreground">
        {formatToolName(name)}
        {!!durationStr && <span className="font-normal text-foreground/50"> ({durationStr})</span>}
      </span>
      <span className="flex-1 truncate text-xs text-foreground">{summary}</span>
      {completedChevron}
    </button>
  );
};

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
  // react-doctor-disable-next-line react-doctor/no-effect-event-handler
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
      className={`overflow-x-auto whitespace-pre-wrap bg-card px-3 py-2 font-mono text-xs text-foreground ${containClass} ${heightClass}`}
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
}> = ({ output, isExpanded, onToggle, language, isStreaming }) => {
  const { t } = useTranslation();
  const outputChevron = isExpanded ? (
    <ChevronDown className="size-3.5 flex-shrink-0 text-foreground" />
  ) : (
    <ChevronRight className="size-3.5 flex-shrink-0 text-foreground" />
  );
  return (
    <>
      <button
        onClick={onToggle}
        className="flex w-full items-center gap-2 border-t border-border bg-muted px-3 py-1.5 text-left transition-colors hover:bg-accent"
      >
        <span className="text-xs font-medium text-foreground">{t('toolCallBlock.output')}</span>
        <span className="flex-1 truncate text-xs text-foreground">
          {(() => {
            if (isStreaming) return 'Streaming...';
            if (output.length > TOOL_SUMMARY_MAX_LENGTH) {
              return `${output.slice(0, TOOL_SUMMARY_MAX_LENGTH)}...`;
            }
            return output;
          })()}
        </span>
        {outputChevron}
      </button>
      {!!isExpanded && (
        <ScrollableOutput output={output} isStreaming={isStreaming} language={language} />
      )}
    </>
  );
};

const FILE_DIFF_OPS: Record<string, { label: string; color: string; bg: string }> = {
  write_file: { label: 'created', color: '#3fb950', bg: 'rgba(63,185,80,0.10)' },
  edit_file:  { label: 'edited',  color: '#d29922', bg: 'rgba(210,153,34,0.10)' },
  insert_text:{ label: 'inserted',color: '#58a6ff', bg: 'rgba(88,166,255,0.10)' },
  undo_edit:  { label: 'reverted',color: '#e8912d', bg: 'rgba(232,145,45,0.10)' },
};

const MAX_DIFF_LINES = 300;

/**
 * Inline diff view for file manipulation tools (write_file, edit_file, insert_text).
 * Always visible below the tool header — no click required.
 */
const FileDiffView: React.FC<{ name: string; args: Record<string, unknown> }> = ({ name, args }) => {
  const op = FILE_DIFF_OPS[name];
  if (!op) return null;

  const path = String(args.path || '');

  type DiffLine = { type: 'add' | 'remove'; content: string };
  let lines: DiffLine[] = [];

  if (name === 'write_file') {
    lines = String(args.content || '').split('\n').map((l) => ({ type: 'add', content: l }));
  } else if (name === 'edit_file') {
    const removed = String(args.old_string || '').split('\n').map((l) => ({ type: 'remove' as const, content: l }));
    const added   = String(args.new_string || '').split('\n').map((l) => ({ type: 'add'    as const, content: l }));
    lines = [...removed, ...added];
  } else if (name === 'insert_text') {
    lines = String(args.content || '').split('\n').map((l) => ({ type: 'add', content: l }));
  }
  // undo_edit: no content diff available, just show path badge

  const truncated = lines.length > MAX_DIFF_LINES;
  const visible = truncated ? lines.slice(0, MAX_DIFF_LINES) : lines;

  return (
    <div className="border-t border-border">
      {/* File path + operation badge */}
      <div className="flex items-center gap-2 bg-card px-3 py-1.5">
        <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-muted-foreground">{path}</span>
        <span
          className="flex-shrink-0 rounded px-1.5 py-0.5 font-mono text-[10px] font-semibold uppercase tracking-wide"
          style={{ color: op.color, background: op.bg }}
        >
          {op.label}
        </span>
      </div>
      {/* Diff lines */}
      {visible.length > 0 && (
        <div className="max-h-52 overflow-y-auto overscroll-contain">
          <pre className="bg-card font-mono text-xs leading-5">
            {visible.map((line, i) => (
              <div
                key={i}
                style={{
                  backgroundColor: line.type === 'add'
                    ? 'rgba(63,185,80,0.07)'
                    : 'rgba(248,81,73,0.07)',
                }}
                className="flex px-3"
              >
                <span
                  className="mr-2 w-3 flex-shrink-0 select-none"
                  style={{ color: line.type === 'add' ? '#3fb950' : '#f85149' }}
                >
                  {line.type === 'add' ? '+' : '-'}
                </span>
                <span
                  className="flex-1 whitespace-pre-wrap break-all"
                  style={{ color: line.type === 'add' ? '#3fb950' : '#f85149' }}
                >
                  {line.content}
                </span>
              </div>
            ))}
            {truncated && (
              <div className="px-3 py-1 text-[11px] text-muted-foreground">
                … {lines.length - MAX_DIFF_LINES} more lines
              </div>
            )}
          </pre>
        </div>
      )}
    </div>
  );
};

/** Gallery of images from a display_images tool result. */
const ImageGallery: React.FC<{ output: string }> = ({ output }) => {
  const [lightbox, setLightbox] = useState<string | null>(null);
  let urls: string[] = [];
  let title = '';
  try {
    const parsed = JSON.parse(output) as { urls?: string[]; title?: string };
    urls = Array.isArray(parsed.urls) ? parsed.urls : [];
    title = parsed.title ?? '';
  } catch {
    return null;
  }
  if (urls.length === 0) return null;
  return (
    <div className="px-3 py-2">
      {!!title && <p className="mb-2 text-xs font-medium text-muted-foreground">{title}</p>}
      <div className="flex flex-wrap gap-2">
        {urls.map((src, i) => (
          <button
            key={i}
            type="button"
            className="overflow-hidden rounded-lg border border-border/50 transition-colors hover:border-primary/50"
            onClick={() => setLightbox(src)}
          >
            <img
              src={src}
              alt={`Image ${i + 1}`}
              className="h-32 w-auto max-w-[200px] object-cover"
              loading="lazy"
            />
          </button>
        ))}
      </div>
      {lightbox !== null &&
        createPortal(
          <div
            className="fixed inset-0 z-[9999] flex cursor-pointer items-center justify-center bg-black/95 p-4"
            role="button"
            tabIndex={0}
            onClick={() => setLightbox(null)}
            onKeyDown={(e) => {
              if (e.key === 'Escape' || e.key === 'Enter') setLightbox(null);
            }}
          >
            <img
              src={lightbox}
              alt="Full size"
              className="max-h-full max-w-full object-contain"
              onClick={(e) => e.stopPropagation()}
            />
            <button
              onClick={() => setLightbox(null)}
              className="absolute right-4 top-4 rounded-full bg-white/20 p-2 text-white backdrop-blur transition-colors hover:bg-white/30"
              title="Close"
            >
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M18 6 6 18M6 6l12 12" />
              </svg>
            </button>
          </div>,
          document.body,
        )}
    </div>
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
  // If generation is stopped, tool calls can't be executing anymore.
  // If duration_ms is known the tool has already finished executing — show CompletedHeader
  // even when isPending=true (batch tools share one <tool_response> so output can't be matched).
  const isExecuting =
    isGenerating !== false &&
    toolCall.duration_ms == null &&
    (toolCall.isStreaming === true || (toolCall.isPending === true && !toolCall.output));
  const elapsed = useElapsedTime(isExecuting);
  const outputLanguage = getOutputLanguage(toolCall.name, toolCall.arguments);

  // Parse [TOOL_RESULT:success/error] tag from output
  const toolResultMatch = toolCall.output?.match(/^\[TOOL_RESULT:(success|error)\]/);
  const toolResultStatus = toolResultMatch?.[1] as 'success' | 'error' | undefined;
  const rawAfterStatus = toolResultMatch
    ? (toolCall.output ?? '').slice(toolResultMatch[0].length)
    : toolCall.output;
  // Parse [DISPLAY_IMAGES]{...json} emitted by the display_images tool
  const displayImagesMatch = rawAfterStatus?.match(/^\[DISPLAY_IMAGES\](\{.*\})$/s);
  const displayImagesJson = displayImagesMatch?.[1] ?? null;
  const cleanOutput = displayImagesMatch ? null : rawAfterStatus;
  const hasOutput = !!cleanOutput && cleanOutput.length > 0;
  const hasDiffView = typeof toolCall.arguments === 'object' && toolCall.name in FILE_DIFF_OPS;
  const expandedContent = (
    <pre className="max-h-64 overflow-x-auto overflow-y-auto overscroll-contain whitespace-pre-wrap bg-card px-3 py-2 font-mono text-xs text-foreground">
      {formatToolArguments(toolCall.arguments)}
    </pre>
  );

  const headerEl = isExecuting ? (
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
      durationMs={toolCall.duration_ms}
    />
  );
  const hasCleanOutput = hasOutput && (cleanOutput ?? '').trim().length > 0;

  return (
    <div className="flat-card overflow-hidden" style={{ contain: 'content' }}>
      {headerEl}
      {hasDiffView && (
        <FileDiffView
          name={toolCall.name}
          args={toolCall.arguments as Record<string, unknown>}
        />
      )}
      {!!isExpanded && expandedContent}
      {!!hasCleanOutput && (
        <CompletedOutput
          output={cleanOutput ?? ''}
          isExpanded={isOutputExpanded}
          onToggle={() => setIsOutputExpanded(!isOutputExpanded)}
          language={outputLanguage}
          isStreaming={isExecuting}
        />
      )}
      {displayImagesJson !== null && <ImageGallery output={displayImagesJson} />}
    </div>
  );
};

// ─── Context-tool grouping ────────────────────────────────────────────────────

/** Read-only tools with no visible side effects — grouped into a collapsed batch. */
const CONTEXT_TOOLS = new Set([
  'read_file',
  'list_directory',
  'find_files',
  'search_files',
  'web_fetch',
  'web_search',
  'check_background_process',
  'check_environment',
  'find_executable',
  'git_status',
  'git_diff',
]);

const CONTEXT_TOOL_LABELS: Record<string, string> = {
  read_file: 'read',
  list_directory: 'list',
  find_files: 'find',
  search_files: 'search',
  web_fetch: 'fetch',
  web_search: 'search',
  check_background_process: 'check',
  check_environment: 'check',
  find_executable: 'find',
  git_status: 'git',
  git_diff: 'git',
};

type ToolGroup =
  | { type: 'single'; toolCall: ToolCall }
  | { type: 'context-group'; toolCalls: ToolCall[] };

function groupToolCalls(toolCalls: ToolCall[]): ToolGroup[] {
  const groups: ToolGroup[] = [];
  let pending: ToolCall[] = [];

  const flushPending = () => {
    if (pending.length === 1) {
      groups.push({ type: 'single', toolCall: pending[0] });
    } else if (pending.length > 1) {
      groups.push({ type: 'context-group', toolCalls: [...pending] });
    }
    pending = [];
  };

  for (const tc of toolCalls) {
    if (CONTEXT_TOOLS.has(tc.name)) {
      pending.push(tc);
    } else {
      flushPending();
      groups.push({ type: 'single', toolCall: tc });
    }
  }
  flushPending();
  return groups;
}

function buildContextSummary(toolCalls: ToolCall[]): string {
  const counts: Record<string, number> = {};
  for (const tc of toolCalls) {
    const label = CONTEXT_TOOL_LABELS[tc.name] ?? tc.name;
    counts[label] = (counts[label] ?? 0) + 1;
  }
  return Object.entries(counts)
    .map(([label, count]) => `${count} ${label}${count > 1 ? 's' : ''}`)
    .join(', ');
}

const ContextToolGroup: React.FC<{ toolCalls: ToolCall[]; isGenerating?: boolean }> = ({
  toolCalls,
  isGenerating,
}) => {
  const [expanded, setExpanded] = useState(false);
  const anyExecuting = toolCalls.some(
    (tc) =>
      isGenerating !== false &&
      tc.duration_ms == null &&
      (tc.isStreaming === true || (tc.isPending === true && !tc.output)),
  );
  const summary = buildContextSummary(toolCalls);
  const label = anyExecuting ? 'Gathering context' : 'Gathered context';

  return (
    <div className="flat-card overflow-hidden">
      <button
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-2 bg-muted px-3 py-2 text-left transition-colors hover:bg-accent"
      >
        {anyExecuting ? (
          <span className="inline-block size-3 flex-shrink-0 animate-spin rounded-full border-2 border-foreground/50 border-t-transparent" />
        ) : (
          <BookOpen className="size-3 flex-shrink-0 text-muted-foreground" />
        )}
        <span className="text-xs font-medium text-foreground">{label}:</span>
        <span className="flex-1 truncate text-xs text-muted-foreground">{summary}</span>
        <span className="text-xs text-muted-foreground/60">{toolCalls.length}</span>
        {expanded ? (
          <ChevronDown className="size-3.5 flex-shrink-0 text-foreground" />
        ) : (
          <ChevronRight className="size-3.5 flex-shrink-0 text-foreground" />
        )}
      </button>
      {!!expanded && (
        <div className="divide-y divide-border border-t border-border">
          {toolCalls.map((tc) => (
            <SingleToolCall key={tc.id} toolCall={tc} isGenerating={isGenerating} />
          ))}
        </div>
      )}
    </div>
  );
};

// ─────────────────────────────────────────────────────────────────────────────

/**
 * Compact tool call display with expandable details.
 * Shows executing/streaming state for in-progress tool calls.
 * Consecutive context-gathering tools are grouped into a single collapsed row.
 */
export const ToolCallBlock = React.memo(({ toolCalls, isGenerating }: ToolCallBlockProps) => {
  if (toolCalls.length === 0) return null;

  const groups = groupToolCalls(toolCalls);

  return (
    <div className="space-y-2">
      {groups.map((group) =>
        group.type === 'context-group' ? (
          <ContextToolGroup
            key={group.toolCalls[0].id}
            toolCalls={group.toolCalls}
            isGenerating={isGenerating}
          />
        ) : (
          <SingleToolCall key={group.toolCall.id} toolCall={group.toolCall} isGenerating={isGenerating} />
        ),
      )}
    </div>
  );
});
ToolCallBlock.displayName = 'ToolCallBlock';
