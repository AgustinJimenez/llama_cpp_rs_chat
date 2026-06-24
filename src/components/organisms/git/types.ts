import type { AssignedCommit } from '../../../utils/gitGraph';

// ── Diff types ────────────────────────────────────────────────────────────────
export type DiffViewMode = 'split' | 'inline' | 'hunk';
export type DiffLineType = 'add' | 'remove' | 'context' | 'hunk' | 'meta';
export interface DiffLine {
  type: DiffLineType;
  content: string;
  oldLine?: number;
  newLine?: number;
}
export interface SplitRow {
  left: DiffLine | null;
  right: DiffLine | null;
  type: 'context' | 'changed' | 'hunk' | 'meta';
}

// ── File change ───────────────────────────────────────────────────────────────
export interface FileChange {
  status: string;
  path: string;
}

// ── Branch extraction ─────────────────────────────────────────────────────────
export interface BranchEntry {
  name: string;
  hash: string;
  isCurrent: boolean;
  kind: 'local' | 'remote' | 'tag';
}

// ── File tree ─────────────────────────────────────────────────────────────────
export type FileListMode = 'path' | 'tree';
export type DirTree = Map<string, DirTree | FileChange>;
export interface FlatTreeItem {
  name: string;
  fullPath: string;
  depth: number;
  isFolder: boolean;
  file?: FileChange;
  statusCounts?: Record<string, number>;
}

// ── Context menu ──────────────────────────────────────────────────────────────
export type CtxItemKind = 'action' | 'danger' | 'copy';
export interface CtxMenuDef {
  key: string;
  labelKey: string;
  sep?: boolean;
  kind: CtxItemKind;
  copyFn?: (c: AssignedCommit) => string;
}

export const CTX_DEFS: CtxMenuDef[] = [
  { key: 'checkout', labelKey: 'gitGraph.ctxCheckout', kind: 'action' },
  { key: 'createBranch', labelKey: 'gitGraph.ctxCreateBranch', kind: 'action', sep: true },
  { key: 'cherryPick', labelKey: 'gitGraph.ctxCherryPick', kind: 'action', sep: true },
  { key: 'revert', labelKey: 'gitGraph.ctxRevert', kind: 'action', sep: true },
  { key: 'resetSoft', labelKey: 'gitGraph.ctxResetSoft', kind: 'action' },
  { key: 'resetMixed', labelKey: 'gitGraph.ctxResetMixed', kind: 'action' },
  { key: 'resetHard', labelKey: 'gitGraph.ctxResetHard', kind: 'danger' },
  {
    key: 'copySHA',
    labelKey: 'gitGraph.ctxCopySHA',
    kind: 'copy',
    sep: true,
    copyFn: (c) => c.hash,
  },
  {
    key: 'copySubject',
    labelKey: 'gitGraph.ctxCopySubject',
    kind: 'copy',
    copyFn: (c) => c.subject,
  },
  { key: 'copyAuthor', labelKey: 'gitGraph.ctxCopyAuthor', kind: 'copy', copyFn: (c) => c.author },
];

export interface CtxMenuState {
  x: number;
  y: number;
  commit: AssignedCommit;
}
