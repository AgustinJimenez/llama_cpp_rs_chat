import { detectLanguageFromPath } from '../../../utils/languageDetect';

const SUMMARY_MAX_LEN = 80;

export interface ToolDef {
  /** One-line summary of what the call acted on. */
  summary?: (args: Record<string, unknown>) => string;
  /** Group this tool into the "Gathered context" accordion. */
  isContext?: boolean;
  /** Label used in the context group count (e.g. "read", "search"). */
  contextLabel?: string;
  /** Show an inline diff card below the header for file-mutation tools. */
  diffOp?: { label: string; color: string; bg: string };
  /** Syntax language for the output block; null means plain text. */
  outputLanguage?: (args: Record<string, unknown>) => string | null;
}

const _registry = new Map<string, ToolDef>();

export function registerTool(name: string, def: ToolDef): void {
  _registry.set(name, def);
}

export function getToolDef(name: string): ToolDef {
  return _registry.get(name) ?? {};
}

/** Returns a one-line summary for a tool call. */
export function getToolSummary(name: string, args: Record<string, unknown> | string): string {
  if (typeof args === 'string') return args.slice(0, SUMMARY_MAX_LEN);
  const summarizer = _registry.get(name)?.summary;
  if (summarizer) return summarizer(args);
  // Fallback: first non-empty, non-numeric string value
  for (const val of Object.values(args)) {
    if (typeof val === 'string' && val.length > 0 && !/^\d+(\.\d+)?$/.test(val)) {
      return val.length > SUMMARY_MAX_LEN ? `${val.slice(0, SUMMARY_MAX_LEN)}...` : val;
    }
  }
  return '';
}

// ─── Tool definitions ─────────────────────────────────────────────────────────

registerTool('read_file', {
  summary: (a) => String(a.path || ''),
  isContext: true,
  contextLabel: 'read',
  outputLanguage: (a) => detectLanguageFromPath(String(a.path || '')),
});

registerTool('write_file', {
  summary: (a) => `${a.path} (${String(a.content || '').length} chars)`,
  diffOp: { label: 'created', color: '#3fb950', bg: 'rgba(63,185,80,0.10)' },
});

registerTool('edit_file', {
  summary: (a) => String(a.path || ''),
  diffOp: { label: 'edited', color: '#d29922', bg: 'rgba(210,153,34,0.10)' },
});

registerTool('undo_edit', {
  summary: (a) => String(a.path || ''),
  diffOp: { label: 'reverted', color: '#e8912d', bg: 'rgba(232,145,45,0.10)' },
});

registerTool('insert_text', {
  summary: (a) => `${a.path}:${a.line}`,
  diffOp: { label: 'inserted', color: '#58a6ff', bg: 'rgba(88,166,255,0.10)' },
});

registerTool('search_files', {
  summary: (a) => {
    const pat = String(a.pattern || '');
    const path = a.path ? ` in ${a.path}` : '';
    return `"${pat}"${path}`;
  },
  isContext: true,
  contextLabel: 'search',
});

registerTool('find_files', {
  summary: (a) => {
    const pat = String(a.pattern || '');
    const path = a.path ? ` in ${a.path}` : '';
    return `${pat}${path}`;
  },
  isContext: true,
  contextLabel: 'find',
});

registerTool('execute_python', {
  summary: (a) => {
    const code = String(a.code || '');
    const firstLine = code.split('\n')[0].trim();
    const lineCount = code.split('\n').length;
    return lineCount > 1 ? `${firstLine} ... (${lineCount} lines)` : firstLine;
  },
});

registerTool('execute_command', {
  summary: (a) => String(a.command || ''),
});

registerTool('list_directory', {
  summary: (a) => String(a.path || '.'),
  isContext: true,
  contextLabel: 'list',
});

registerTool('git_status', {
  summary: (a) => String(a.path || '.'),
  isContext: true,
  contextLabel: 'git',
});

registerTool('git_diff', {
  summary: (a) => {
    const path = a.path ? String(a.path) : '';
    const staged = a.staged ? ' (staged)' : '';
    return `${path}${staged}` || '.';
  },
  isContext: true,
  contextLabel: 'git',
  outputLanguage: () => 'diff',
});

registerTool('git_commit', {
  summary: (a) => {
    const msg = String(a.message || '');
    return msg.length > 60 ? `${msg.slice(0, 60)}...` : msg;
  },
});

registerTool('display_images', {
  summary: (a) => {
    const urls = Array.isArray(a.urls) ? (a.urls as string[]) : [];
    const title = a.title ? ` — ${String(a.title)}` : '';
    return `${urls.length} image${urls.length !== 1 ? 's' : ''}${title}`;
  },
});

registerTool('web_fetch', {
  isContext: true,
  contextLabel: 'fetch',
});

registerTool('web_search', {
  isContext: true,
  contextLabel: 'search',
});

registerTool('check_background_process', {
  isContext: true,
  contextLabel: 'check',
});

registerTool('check_environment', {
  isContext: true,
  contextLabel: 'check',
});

registerTool('find_executable', {
  isContext: true,
  contextLabel: 'find',
});
