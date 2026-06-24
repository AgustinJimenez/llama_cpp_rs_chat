import type { AssignedCommit, RawCommit } from '../../../utils/gitGraph';

import {
  DAY_PER_MO,
  H_PER_DAY,
  MIN_PER_H,
  MO_PER_YR,
  MSG_LEN_ERROR,
  MSG_LEN_WARN,
  MS_PER_S,
  S_PER_MIN,
} from './constants';
import type { BranchEntry, DiffLine, DirTree, FileChange, FlatTreeItem, SplitRow } from './types';

// ── Time helpers ──────────────────────────────────────────────────────────────
export function relDate(iso: string): string {
  const diffMs = Date.now() - new Date(iso).getTime();
  const s = Math.floor(diffMs / MS_PER_S);
  if (s < S_PER_MIN) return 'just now';
  const m = Math.floor(s / S_PER_MIN);
  if (m < MIN_PER_H) return `${m}m`;
  const h = Math.floor(m / MIN_PER_H);
  if (h < H_PER_DAY) return `${h}h`;
  const d = Math.floor(h / H_PER_DAY);
  if (d < DAY_PER_MO) return `${d}d`;
  const mo = Math.floor(d / DAY_PER_MO);
  if (mo < MO_PER_YR) return `${mo}mo`;
  return `${Math.floor(mo / MO_PER_YR)}y`;
}

export function commitMsgLenCls(len: number): string {
  if (len > MSG_LEN_ERROR) return 'text-red-400';
  if (len > MSG_LEN_WARN) return 'text-amber-400';
  return 'text-muted-foreground/50';
}

// ── Diff parsing ──────────────────────────────────────────────────────────────
export const diffCache = new Map<string, DiffLine[]>();

export function parseHunkNums(line: string): { oldStart: number; newStart: number } | null {
  const m = line.match(/@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
  return m ? { oldStart: parseInt(m[1], 10), newStart: parseInt(m[2], 10) } : null;
}

export function parseDiff(raw: string): DiffLine[] {
  let oldLine = 1;
  let newLine = 1;
  let inDiff = false;
  return raw.split('\n').map((rawLine): DiffLine => {
    if (rawLine.startsWith('diff ')) {
      inDiff = true;
      return { type: 'meta', content: rawLine };
    }
    if (!inDiff) return { type: 'meta', content: rawLine };
    if (rawLine.startsWith('@@')) {
      const nums = parseHunkNums(rawLine);
      if (nums) {
        oldLine = nums.oldStart;
        newLine = nums.newStart;
      }
      return { type: 'hunk', content: rawLine };
    }
    if (
      rawLine.startsWith('index ') ||
      rawLine.startsWith('--- ') ||
      rawLine.startsWith('+++ ') ||
      rawLine.startsWith('new file') ||
      rawLine.startsWith('deleted file')
    ) {
      return { type: 'meta', content: rawLine };
    }
    if (rawLine.startsWith('+')) return { type: 'add', content: rawLine, newLine: newLine++ };
    if (rawLine.startsWith('-')) return { type: 'remove', content: rawLine, oldLine: oldLine++ };
    const result: DiffLine = { type: 'context', content: rawLine, oldLine, newLine };
    oldLine++;
    newLine++;
    return result;
  });
}

export function toSplitRows(lines: DiffLine[]): SplitRow[] {
  const result: SplitRow[] = [];
  let i = 0;
  while (i < lines.length) {
    const ln = lines[i];
    if (ln.type === 'hunk') {
      result.push({ left: ln, right: null, type: 'hunk' });
      i++;
    } else if (ln.type === 'meta') {
      result.push({ left: ln, right: null, type: 'meta' });
      i++;
    } else if (ln.type === 'context') {
      result.push({ left: ln, right: ln, type: 'context' });
      i++;
    } else {
      const removes: DiffLine[] = [];
      const adds: DiffLine[] = [];
      while (i < lines.length && lines[i].type === 'remove') removes.push(lines[i++]);
      while (i < lines.length && lines[i].type === 'add') adds.push(lines[i++]);
      const len = Math.max(removes.length, adds.length);
      for (let j = 0; j < len; j++) {
        result.push({ left: removes[j] ?? null, right: adds[j] ?? null, type: 'changed' });
      }
    }
  }
  return result;
}

export function splitSideCls(line: DiffLine | null, side: 'left' | 'right'): string {
  if (!line) return 'bg-muted/10 text-transparent select-none';
  if (side === 'left' && line.type === 'remove') return 'bg-red-950/50 text-red-300';
  if (side === 'right' && line.type === 'add') return 'bg-emerald-950/50 text-emerald-300';
  return 'text-foreground/80';
}

// ── File tree helpers ─────────────────────────────────────────────────────────
export function insertPath(tree: DirTree, parts: string[], file: FileChange): void {
  if (parts.length === 1) {
    tree.set(parts[0], file);
    return;
  }
  const [head, ...rest] = parts;
  if (!tree.has(head)) tree.set(head, new Map() as DirTree);
  const sub = tree.get(head);
  if (sub instanceof Map) insertPath(sub as DirTree, rest, file);
}

export function countDirStatuses(tree: DirTree): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const v of tree.values()) {
    if (v instanceof Map) {
      const sub = countDirStatuses(v as DirTree);
      for (const [k, n] of Object.entries(sub)) counts[k] = (counts[k] ?? 0) + n;
    } else {
      const s = (v as FileChange).status[0] ?? '?';
      counts[s] = (counts[s] ?? 0) + 1;
    }
  }
  return counts;
}

export function flattenDirTree(
  tree: DirTree,
  depth: number,
  collapsed: Set<string>,
  base: string,
): FlatTreeItem[] {
  const items: FlatTreeItem[] = [];
  const sorted = [...tree.entries()].sort(([a, av], [b, bv]) => {
    const ad = av instanceof Map;
    const bd = bv instanceof Map;
    if (ad !== bd) return ad ? -1 : 1;
    return a.localeCompare(b);
  });
  for (const [name, value] of sorted) {
    const fullPath = base ? `${base}/${name}` : name;
    if (value instanceof Map) {
      const statusCounts = countDirStatuses(value as DirTree);
      items.push({ name, fullPath, depth, isFolder: true, statusCounts });
      if (!collapsed.has(fullPath)) {
        items.push(...flattenDirTree(value as DirTree, depth + 1, collapsed, fullPath));
      }
    } else {
      items.push({ name, fullPath, depth, isFolder: false, file: value as FileChange });
    }
  }
  return items;
}

// ── Branch extraction ─────────────────────────────────────────────────────────
export function extractBranches(commits: AssignedCommit[]): {
  local: BranchEntry[];
  remote: BranchEntry[];
  tags: BranchEntry[];
} {
  const local: BranchEntry[] = [];
  const remote: BranchEntry[] = [];
  const tags: BranchEntry[] = [];
  const seen = new Set<string>();
  for (const c of commits) {
    for (const ref of c.refs) {
      if (ref === 'HEAD' || ref.includes(' -> ')) continue;
      let name = ref;
      let kind: BranchEntry['kind'];
      const isCurrent = false;
      if (ref.startsWith('tag: ')) {
        name = ref.slice('tag: '.length);
        kind = 'tag';
      } else if (ref.includes('/')) {
        kind = 'remote';
      } else {
        kind = 'local';
      }
      if (seen.has(name)) continue;
      seen.add(name);
      const entry: BranchEntry = { name, hash: c.hash, isCurrent, kind };
      if (kind === 'local') local.push(entry);
      else if (kind === 'remote') remote.push(entry);
      else tags.push(entry);
    }
    for (const ref of c.refs) {
      if (ref.startsWith('HEAD -> ')) {
        const name = ref.slice('HEAD -> '.length);
        const found = local.find((e) => e.name === name);
        if (found) found.isCurrent = true;
      }
    }
  }
  return { local, remote, tags };
}

// ── API helpers ───────────────────────────────────────────────────────────────
export interface GitLogResponse {
  commits: RawCommit[];
  error: string | null;
}

export async function fetchGitLog(path: string): Promise<GitLogResponse> {
  const res = await fetch(`/api/git/log?path=${encodeURIComponent(path)}`);
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  return res.json() as Promise<GitLogResponse>;
}

export async function pickDirectory(): Promise<string | null> {
  try {
    const res = await fetch('/api/browse/pick-directory', { method: 'POST' });
    if (!res.ok) return null;
    const data = (await res.json()) as { path?: string };
    return data.path ?? null;
  } catch {
    return null;
  }
}
