/* eslint-disable max-lines */
import { AlignJustify, ArrowDown, ArrowLeftRight, ArrowUp, ArrowUpDown, ChevronDown, ChevronRight, Circle, Columns2, Filter, FolderOpen, FolderTree, GitBranch, List, Minus, Pencil, Plus, RefreshCw, Search, X } from 'lucide-react';
import { useVirtualizer } from '@tanstack/react-virtual';
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import toast from 'react-hot-toast';
import { useTranslation } from 'react-i18next';

import { fetchGitStatus, gitCommit, gitFetch, gitPull, gitPush, gitStage, gitUnstage, processCommits } from '../../utils/gitGraph';
import type { AssignedCommit, GitStatusResult, GraphEdge, RawCommit } from '../../utils/gitGraph';

// ── Layout constants ──────────────────────────────────────────────────────────
const ROW_H = 30;
const LANE_W = 18;
const NODE_R = 5;
const SVG_PAD_R = 10;
const EDGE_STROKE_W = 1.5;
const EDGE_OPACITY = 0.75;
const SELECTED_R_DELTA = 2;
const SELECTED_STROKE_W = 2;

// Horizontal layout constants
const COL_W = 30;
const H_LANE_H = 18;

// Vertical table column widths
const REFS_COL_W = 140;
const AUTHOR_COL_W = 100;
const DATE_COL_W = 40;
const BRANCH_PANEL_W = 190;
const DETAIL_PANEL_W = 240;

// ── Time helpers ──────────────────────────────────────────────────────────────
const MS_PER_S = 1000;
const S_PER_MIN = 60;
const MIN_PER_H = 60;
const H_PER_DAY = 24;
const DAY_PER_MO = 30;
const MO_PER_YR = 12;

function relDate(iso: string): string {
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

// ── Context menu items (module-level, no recreation on render) ────────────────
interface CtxMenuItem {
  key: string;
  labelKey: string;
  getValue: (c: AssignedCommit) => string;
  sep?: boolean; // render a divider above this item
}

const CTX_ITEMS: CtxMenuItem[] = [
  // ── Copy info ──
  { key: 'hash',       labelKey: 'gitGraph.copyHash',         getValue: (c) => c.hash },
  { key: 'short',      labelKey: 'gitGraph.copyShortHash',    getValue: (c) => c.short_hash },
  { key: 'subject',    labelKey: 'gitGraph.copySubject',      getValue: (c) => c.subject },
  { key: 'author',     labelKey: 'gitGraph.copyAuthor',       getValue: (c) => c.author },
  // ── Branch & worktree ──
  { key: 'checkout',   labelKey: 'gitGraph.copyCheckout',     getValue: (c) => `git checkout ${c.hash}`,                              sep: true },
  { key: 'branch',     labelKey: 'gitGraph.copyBranch',       getValue: (c) => `git checkout -b NEW_BRANCH_NAME ${c.hash}` },
  { key: 'worktree',   labelKey: 'gitGraph.copyWorktree',     getValue: (c) => `git worktree add WORKTREE_PATH ${c.hash}` },
  // ── Integrate ──
  { key: 'cherrypick', labelKey: 'gitGraph.copyCherryPick',   getValue: (c) => `git cherry-pick ${c.hash}`,                          sep: true },
  { key: 'merge',      labelKey: 'gitGraph.copyMerge',        getValue: (c) => `git merge ${c.hash}` },
  { key: 'rebase',     labelKey: 'gitGraph.copyRebase',       getValue: (c) => `git rebase --onto ${c.hash}` },
  // ── Undo ──
  { key: 'revert',     labelKey: 'gitGraph.copyRevert',       getValue: (c) => `git revert ${c.hash}`,                               sep: true },
  { key: 'resetHard',  labelKey: 'gitGraph.copyResetHard',    getValue: (c) => `git reset --hard ${c.hash}` },
  { key: 'resetSoft',  labelKey: 'gitGraph.copyResetSoft',    getValue: (c) => `git reset --soft ${c.hash}` },
  { key: 'resetMixed', labelKey: 'gitGraph.copyResetMixed',   getValue: (c) => `git reset --mixed ${c.hash}` },
  // ── Tag & patch ──
  { key: 'tag',        labelKey: 'gitGraph.copyTag',          getValue: (c) => `git tag NEW_TAG_NAME ${c.hash}`,                     sep: true },
  { key: 'tagAnnot',   labelKey: 'gitGraph.copyTagAnnotated', getValue: (c) => `git tag -a NEW_TAG_NAME -m "TAG_MESSAGE" ${c.hash}` },
  { key: 'patch',      labelKey: 'gitGraph.copyPatch',        getValue: (c) => `git format-patch ${c.hash}^..${c.hash}` },
  // ── Inspect ──
  { key: 'show',       labelKey: 'gitGraph.copyShow',         getValue: (c) => `git show ${c.hash}`,                                 sep: true },
  { key: 'diff',       labelKey: 'gitGraph.copyDiff',         getValue: (c) => `git diff ${c.hash}^ ${c.hash}` },
  { key: 'log',        labelKey: 'gitGraph.copyLog',          getValue: (c) => `git log --oneline ${c.hash}~10..${c.hash}` },
];

// ── Ref badge ─────────────────────────────────────────────────────────────────
type RefKind = 'head' | 'local' | 'remote' | 'tag';

function parseRef(r: string): { label: string; kind: RefKind } {
  if (r.startsWith('HEAD -> ')) return { label: r.slice('HEAD -> '.length), kind: 'head' };
  if (r === 'HEAD') return { label: 'HEAD', kind: 'head' };
  if (r.startsWith('tag: ')) return { label: r.slice('tag: '.length), kind: 'tag' };
  if (r.includes('/')) return { label: r, kind: 'remote' };
  return { label: r, kind: 'local' };
}

const REF_CLS: Record<RefKind, string> = {
  head: 'bg-emerald-600 text-white',
  local: 'bg-violet-600 text-white',
  remote: 'bg-blue-600 text-white',
  tag: 'bg-amber-400 text-black',
};

const RefBadge: React.FC<{ refStr: string }> = ({ refStr }) => {
  const { label, kind } = parseRef(refStr);
  return (
    <span
      className={`inline-flex max-w-[110px] shrink-0 items-center truncate rounded px-1 font-mono text-[10px] leading-[1.6] ${REF_CLS[kind]}`}
    >
      {label}
    </span>
  );
};

// ── SVG graph (vertical) ──────────────────────────────────────────────────────
function edgeSvgPath(e: GraphEdge): string {
  const x1 = e.fromLane * LANE_W + LANE_W / 2;
  const x2 = e.toLane * LANE_W + LANE_W / 2;
  const y1 = e.fromRow * ROW_H + ROW_H / 2;
  const y2 = e.toRow * ROW_H + ROW_H / 2;
  if (e.fromLane === e.toLane) return `M${x1},${y1}L${x1},${y2}`;
  const ym = (y1 + y2) / 2;
  return `M${x1},${y1}C${x1},${ym} ${x2},${ym} ${x2},${y2}`;
}

const GraphSvg: React.FC<{
  commits: AssignedCommit[];
  edges: GraphEdge[];
  maxLane: number;
  selectedHash: string | null;
  onSelect: (hash: string) => void;
  onContextMenu: (clientX: number, clientY: number, hash: string) => void;
}> = ({ commits, edges, maxLane, selectedHash, onSelect, onContextMenu }) => {
  const svgW = (maxLane + 1) * LANE_W + SVG_PAD_R;
  const svgH = commits.length * ROW_H;
  return (
    <svg width={svgW} height={svgH} className="shrink-0" aria-hidden="true">
      <g>
        {edges.map((e) => (
          <path
            key={e.id}
            d={edgeSvgPath(e)}
            stroke={e.color}
            strokeWidth={EDGE_STROKE_W}
            fill="none"
            opacity={EDGE_OPACITY}
          />
        ))}
      </g>
      <g>
        {commits.map((c) => {
          const cx = c.lane * LANE_W + LANE_W / 2;
          const cy = c.row * ROW_H + ROW_H / 2;
          const isSelected = c.hash === selectedHash;
          const r = isSelected ? NODE_R + SELECTED_R_DELTA : NODE_R;
          const stroke = isSelected ? 'white' : c.color;
          const strokeW = isSelected ? SELECTED_STROKE_W : 0;
          return (
            <circle
              key={c.hash}
              cx={cx}
              cy={cy}
              r={r}
              fill={c.color}
              stroke={stroke}
              strokeWidth={strokeW}
              style={{ cursor: 'pointer' }}
              onClick={() => onSelect(c.hash)}
              onContextMenu={(e) => {
                e.preventDefault();
                onContextMenu(e.clientX, e.clientY, c.hash);
              }}
            />
          );
        })}
      </g>
    </svg>
  );
};

// ── SVG graph (horizontal) ────────────────────────────────────────────────────
function hEdgeSvgPath(e: GraphEdge): string {
  const x1 = e.fromRow * COL_W + COL_W / 2;
  const x2 = e.toRow * COL_W + COL_W / 2;
  const y1 = e.fromLane * H_LANE_H + H_LANE_H / 2;
  const y2 = e.toLane * H_LANE_H + H_LANE_H / 2;
  if (e.fromLane === e.toLane) return `M${x1},${y1}L${x2},${y1}`;
  const xm = (x1 + x2) / 2;
  return `M${x1},${y1}C${xm},${y1} ${xm},${y2} ${x2},${y2}`;
}

const HGraphSvg: React.FC<{
  commits: AssignedCommit[];
  edges: GraphEdge[];
  maxLane: number;
  selectedHash: string | null;
  onSelect: (hash: string) => void;
  onContextMenu: (clientX: number, clientY: number, hash: string) => void;
}> = ({ commits, edges, maxLane, selectedHash, onSelect, onContextMenu }) => {
  const svgW = commits.length * COL_W + SVG_PAD_R;
  const svgH = (maxLane + 1) * H_LANE_H;
  return (
    <svg width={svgW} height={svgH} aria-hidden="true">
      <g>
        {edges.map((e) => (
          <path
            key={e.id}
            d={hEdgeSvgPath(e)}
            stroke={e.color}
            strokeWidth={EDGE_STROKE_W}
            fill="none"
            opacity={EDGE_OPACITY}
          />
        ))}
      </g>
      <g>
        {commits.map((c) => {
          const cx = c.row * COL_W + COL_W / 2;
          const cy = c.lane * H_LANE_H + H_LANE_H / 2;
          const isSelected = c.hash === selectedHash;
          const r = isSelected ? NODE_R + SELECTED_R_DELTA : NODE_R;
          const stroke = isSelected ? 'white' : c.color;
          const strokeW = isSelected ? SELECTED_STROKE_W : 0;
          return (
            <circle
              key={c.hash}
              cx={cx}
              cy={cy}
              r={r}
              fill={c.color}
              stroke={stroke}
              strokeWidth={strokeW}
              style={{ cursor: 'pointer' }}
              onClick={() => onSelect(c.hash)}
              onContextMenu={(e) => {
                e.preventDefault();
                onContextMenu(e.clientX, e.clientY, c.hash);
              }}
            />
          );
        })}
      </g>
    </svg>
  );
};

// ── File-change type (from /api/git/commit-files) ────────────────────────────
interface FileChange {
  status: string;
  path: string;
}

// ── Branch extraction ─────────────────────────────────────────────────────────
interface BranchEntry {
  name: string;
  hash: string;
  isCurrent: boolean;
  kind: 'local' | 'remote' | 'tag';
}

function extractBranches(commits: AssignedCommit[]): {
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
    // Detect current branch from HEAD -> <name> ref
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

// ── Left branch panel ─────────────────────────────────────────────────────────
const BranchSection: React.FC<{
  titleKey: string;
  entries: BranchEntry[];
  selectedHash: string | null;
  onSelect: (hash: string) => void;
  dotBase: string;
}> = ({ titleKey, entries, selectedHash, onSelect, dotBase }) => {
  const { t } = useTranslation();
  const [open, setOpen] = useState(true);
  const chevronCls = `size-3 shrink-0 transition-transform ${open ? '' : '-rotate-90'}`;
  return (
    <>
      <button
        type="button"
        onClick={() => setOpen((p) => !p)}
        className="flex w-full items-center gap-1.5 px-3 py-1.5 text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/50 hover:text-muted-foreground"
      >
        <ChevronDown className={chevronCls} />
        {t(titleKey)}
        <span className="ml-auto text-muted-foreground/40">{entries.length}</span>
      </button>
      {open && entries.map((e) => {
        const isActive = e.hash === selectedHash;
        const rowCls = `flex w-full min-w-0 items-center gap-1.5 px-3 py-1 text-xs ${isActive ? 'bg-muted/70 text-foreground' : 'text-foreground/70 hover:bg-muted/40 hover:text-foreground'}`;
        const dotCls = `size-1.5 shrink-0 rounded-full ${e.isCurrent ? 'bg-emerald-400' : dotBase}`;
        return (
          <button type="button" key={e.name} className={rowCls} onClick={() => onSelect(e.hash)}>
            <span className={dotCls} />
            <span className="min-w-0 truncate">{e.name}</span>
          </button>
        );
      })}
    </>
  );
};

const BranchPanel: React.FC<{
  commits: AssignedCommit[];
  selectedHash: string | null;
  onSelect: (hash: string) => void;
}> = ({ commits, selectedHash, onSelect }) => {
  const { local, remote, tags } = useMemo(() => extractBranches(commits), [commits]);
  return (
    <div
      style={{ width: BRANCH_PANEL_W }}
      className="flex shrink-0 flex-col overflow-y-auto border-r border-border/40 bg-muted/5 py-1"
    >
      <BranchSection titleKey="gitGraph.panelLocal"  entries={local}  selectedHash={selectedHash} onSelect={onSelect} dotBase="bg-violet-500" />
      <BranchSection titleKey="gitGraph.panelRemote" entries={remote} selectedHash={selectedHash} onSelect={onSelect} dotBase="bg-blue-500" />
      {tags.length > 0 && (
        <BranchSection titleKey="gitGraph.panelTags" entries={tags} selectedHash={selectedHash} onSelect={onSelect} dotBase="bg-amber-400" />
      )}
    </div>
  );
};

// ── Diff viewer ───────────────────────────────────────────────────────────────
type DiffViewMode = 'split' | 'inline' | 'hunk';
type DiffLineType = 'add' | 'remove' | 'context' | 'hunk' | 'meta';
interface DiffLine { type: DiffLineType; content: string; oldLine?: number; newLine?: number; }
interface SplitRow { left: DiffLine | null; right: DiffLine | null; type: 'context' | 'changed' | 'hunk' | 'meta'; }

const diffCache = new Map<string, DiffLine[]>();

const DIFF_MODES: ReadonlyArray<{
  key: DiffViewMode;
  labelKey: string;
  // eslint-disable-next-line @typescript-eslint/naming-convention
  Icon: React.ComponentType<{ className?: string }>;
}> = [
  { key: 'split',  labelKey: 'gitGraph.diffModeSplit',  Icon: Columns2      },
  { key: 'inline', labelKey: 'gitGraph.diffModeInline', Icon: AlignJustify  },
  { key: 'hunk',   labelKey: 'gitGraph.diffModeHunk',   Icon: Filter        },
];

function parseHunkNums(line: string): { oldStart: number; newStart: number } | null {
  const m = line.match(/@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
  return m ? { oldStart: parseInt(m[1], 10), newStart: parseInt(m[2], 10) } : null;
}

function parseDiff(raw: string): DiffLine[] {
  let oldLine = 1;
  let newLine = 1;
  let inDiff = false;
  return raw.split('\n').map((rawLine): DiffLine => {
    if (rawLine.startsWith('diff ')) { inDiff = true; return { type: 'meta', content: rawLine }; }
    if (!inDiff) return { type: 'meta', content: rawLine };
    if (rawLine.startsWith('@@')) {
      const nums = parseHunkNums(rawLine);
      if (nums) { oldLine = nums.oldStart; newLine = nums.newStart; }
      return { type: 'hunk', content: rawLine };
    }
    if (rawLine.startsWith('index ') || rawLine.startsWith('--- ') ||
        rawLine.startsWith('+++ ') || rawLine.startsWith('new file') ||
        rawLine.startsWith('deleted file')) {
      return { type: 'meta', content: rawLine };
    }
    if (rawLine.startsWith('+')) return { type: 'add',    content: rawLine, newLine: newLine++ };
    if (rawLine.startsWith('-')) return { type: 'remove', content: rawLine, oldLine: oldLine++ };
    const result: DiffLine = { type: 'context', content: rawLine, oldLine, newLine };
    oldLine++; newLine++;
    return result;
  });
}

function toSplitRows(lines: DiffLine[]): SplitRow[] {
  const result: SplitRow[] = [];
  let i = 0;
  while (i < lines.length) {
    const ln = lines[i];
    if (ln.type === 'hunk')    { result.push({ left: ln, right: null, type: 'hunk' }); i++; }
    else if (ln.type === 'meta')    { result.push({ left: ln, right: null, type: 'meta' }); i++; }
    else if (ln.type === 'context') { result.push({ left: ln, right: ln,   type: 'context' }); i++; }
    else {
      const removes: DiffLine[] = [];
      const adds: DiffLine[] = [];
      while (i < lines.length && lines[i].type === 'remove') removes.push(lines[i++]);
      while (i < lines.length && lines[i].type === 'add')    adds.push(lines[i++]);
      const len = Math.max(removes.length, adds.length);
      for (let j = 0; j < len; j++) {
        result.push({ left: removes[j] ?? null, right: adds[j] ?? null, type: 'changed' });
      }
    }
  }
  return result;
}

const DIFF_LINE_CLS: Record<DiffLineType, string> = {
  add:     'bg-emerald-950/50 text-emerald-300',
  remove:  'bg-red-950/50 text-red-300',
  hunk:    'bg-muted/40 text-blue-400 font-semibold',
  meta:    'text-muted-foreground/40',
  context: 'text-foreground/80',
};

function splitSideCls(line: DiffLine | null, side: 'left' | 'right'): string {
  if (!line) return 'bg-muted/10 text-transparent select-none';
  if (side === 'left'  && line.type === 'remove') return 'bg-red-950/50 text-red-300';
  if (side === 'right' && line.type === 'add')    return 'bg-emerald-950/50 text-emerald-300';
  return 'text-foreground/80';
}

const DiffInlineTable: React.FC<{ lines: DiffLine[] }> = ({ lines }) => (
  <table className="w-full border-collapse font-mono text-xs">
    <tbody>
      {lines.map((ln, i) => {
        const cls = DIFF_LINE_CLS[ln.type];
        if (ln.type === 'hunk' || ln.type === 'meta') {
          return (
            <tr key={i} className={cls}>
              <td colSpan={3} className="whitespace-pre px-3 py-0">{ln.content}</td>
            </tr>
          );
        }
        const oldNum = (ln.type === 'context' || ln.type === 'remove') ? ln.oldLine : undefined;
        const newNum = (ln.type === 'context' || ln.type === 'add')    ? ln.newLine : undefined;
        return (
          <tr key={i} className={cls}>
            <td className="w-10 select-none border-r border-border/20 px-2 text-right text-[10px] text-muted-foreground/40">{oldNum}</td>
            <td className="w-10 select-none border-r border-border/20 px-2 text-right text-[10px] text-muted-foreground/40">{newNum}</td>
            <td className="whitespace-pre px-3 py-0">{ln.content}</td>
          </tr>
        );
      })}
    </tbody>
  </table>
);

const DiffSplitTable: React.FC<{ rows: SplitRow[] }> = ({ rows }) => (
  <table className="w-full border-collapse font-mono text-xs table-fixed">
    <colgroup>
      <col style={{ width: 40 }} />
      <col style={{ width: '50%' }} />
      <col style={{ width: 40 }} />
      <col />
    </colgroup>
    <tbody>
      {rows.map((row, i) => {
        if (row.type === 'hunk' || row.type === 'meta') {
          const spanCls = row.type === 'hunk' ? DIFF_LINE_CLS.hunk : DIFF_LINE_CLS.meta;
          return (
            <tr key={i} className={spanCls}>
              <td colSpan={4} className="whitespace-pre px-3 py-0">{row.left?.content ?? ''}</td>
            </tr>
          );
        }
        const leftCls  = splitSideCls(row.left,  'left');
        const rightCls = splitSideCls(row.right, 'right');
        const leftNum  = row.left?.oldLine  ?? '';
        const rightNum = row.right?.newLine ?? '';
        return (
          <tr key={i}>
            <td className={`select-none border-r border-border/20 px-2 text-right text-[10px] text-muted-foreground/40 ${leftCls}`}>{leftNum}</td>
            <td className={`whitespace-pre border-r border-border/30 px-2 py-0 ${leftCls}`}>{row.left?.content ?? ' '}</td>
            <td className={`select-none border-r border-border/20 px-2 text-right text-[10px] text-muted-foreground/40 ${rightCls}`}>{rightNum}</td>
            <td className={`whitespace-pre px-2 py-0 ${rightCls}`}>{row.right?.content ?? ' '}</td>
          </tr>
        );
      })}
    </tbody>
  </table>
);

// eslint-disable-next-line max-lines-per-function -- diff viewer: header + mode toggle + 3 view modes
const DiffViewer: React.FC<{
  repoPath: string;
  hash: string;
  file: string;
  onClose: () => void;
}> = ({ repoPath, hash, file, onClose }) => {
  const { t } = useTranslation();
  const [lines, setLines] = useState<DiffLine[]>([]);
  const [diffLoading, setDiffLoading] = useState(true);
  const [diffError, setDiffError] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<DiffViewMode>(
    () => (localStorage.getItem('gitGraphDiffMode') as DiffViewMode | null) ?? 'inline',
  );

  useEffect(() => {
    const cacheKey = `${hash}:${file}`;
    const cached = diffCache.get(cacheKey);
    if (cached) { setLines(cached); setDiffLoading(false); return; }
    setDiffLoading(true);
    setDiffError(null);
    const url =
      `/api/git/file-diff?path=${encodeURIComponent(repoPath)}` +
      `&hash=${encodeURIComponent(hash)}&file=${encodeURIComponent(file)}`;
    fetch(url)
      .then((r) => r.json())
      .then((data: { diff?: string; error?: string }) => {
        if (data.error) { setDiffError(data.error); return; }
        const parsed = parseDiff(data.diff ?? '');
        diffCache.set(cacheKey, parsed);
        setLines(parsed);
      })
      .catch((error: unknown) => {
        setDiffError(error instanceof Error ? error.message : 'Unknown error');
      })
      .finally(() => { setDiffLoading(false); });
  }, [repoPath, hash, file]);

  const splitRows  = useMemo(() => toSplitRows(lines), [lines]);
  const hunkLines  = useMemo(() => lines.filter((l) => l.type !== 'context'), [lines]);
  const fileName   = file.split('/').pop() ?? file;

  const handleModeChange = useCallback((mode: DiffViewMode) => {
    setViewMode(mode);
    localStorage.setItem('gitGraphDiffMode', mode);
  }, []);

  let viewContent: React.ReactNode = null;
  if (viewMode === 'split')      viewContent = <DiffSplitTable rows={splitRows} />;
  else if (viewMode === 'hunk')  viewContent = <DiffInlineTable lines={hunkLines} />;
  else                           viewContent = <DiffInlineTable lines={lines} />;

  return (
    <div className="flex flex-col flex-1 min-h-0 overflow-hidden border-l border-border bg-background">
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-4 py-2">
        <span className="font-mono text-xs text-muted-foreground">{hash.slice(0, 7)}</span>
        <span className="text-muted-foreground/40">·</span>
        <span className="flex-1 truncate font-mono text-sm font-medium text-foreground">{fileName}</span>
        <div className="flex shrink-0 items-center overflow-hidden rounded border border-border">
          {DIFF_MODES.map((m) => {
            const activeCls = viewMode === m.key
              ? 'bg-muted text-foreground'
              : 'text-muted-foreground hover:bg-muted/50';
            return (
              <button
                key={m.key}
                type="button"
                onClick={() => handleModeChange(m.key)}
                className={`px-1.5 py-1 ${activeCls}`}
                title={t(m.labelKey)}
                aria-label={t(m.labelKey)}
              >
                <m.Icon className="size-3.5" />
              </button>
            );
          })}
        </div>
        <button
          type="button"
          onClick={onClose}
          className="shrink-0 rounded p-1 text-muted-foreground hover:bg-muted hover:text-foreground"
          aria-label={t('gitGraph.closeDiff')}
        >
          ✕
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        {diffLoading && <p className="p-4 text-xs text-muted-foreground">{t('gitGraph.loadingFiles')}</p>}
        {!!diffError && <p className="p-4 text-xs text-red-400">{diffError}</p>}
        {!diffLoading && !diffError && viewContent}
      </div>
    </div>
  );
};

// ── Right commit detail panel ─────────────────────────────────────────────────
const FILE_STATUS_CLS: Record<string, string> = {
  A: 'text-emerald-400',
  D: 'text-red-400',
  M: 'text-amber-400',
  R: 'text-blue-400',
};

type StatusIconCmp = React.ComponentType<{ className?: string }>;
const FILE_STATUS_ICON: Record<string, StatusIconCmp> = { M: Pencil, A: Plus, D: Minus, R: Pencil };
const STATUS_ORDER = ['M', 'A', 'D', 'R'];

// ── File tree helpers ─────────────────────────────────────────────────────────
type FileListMode = 'path' | 'tree';
type DirTree = Map<string, DirTree | FileChange>;
interface FlatTreeItem {
  name: string; fullPath: string; depth: number; isFolder: boolean;
  file?: FileChange; statusCounts?: Record<string, number>;
}

function insertPath(tree: DirTree, parts: string[], file: FileChange): void {
  if (parts.length === 1) { tree.set(parts[0], file); return; }
  const [head, ...rest] = parts;
  if (!tree.has(head)) tree.set(head, new Map() as DirTree);
  const sub = tree.get(head);
  if (sub instanceof Map) insertPath(sub as DirTree, rest, file);
}

function countDirStatuses(tree: DirTree): Record<string, number> {
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

function flattenDirTree(tree: DirTree, depth: number, collapsed: Set<string>, base: string): FlatTreeItem[] {
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
      if (!collapsed.has(fullPath)) items.push(...flattenDirTree(value as DirTree, depth + 1, collapsed, fullPath));
    } else {
      items.push({ name, fullPath, depth, isFolder: false, file: value as FileChange });
    }
  }
  return items;
}

// eslint-disable-next-line max-lines-per-function -- detail panel: author block + parents + file list
const CommitDetailPanel: React.FC<{
  commit: AssignedCommit;
  files: FileChange[];
  filesLoading: boolean;
  width: number;
  onSelectParent: (hash: string) => void;
  onSelectFile: (file: string) => void;
}> = ({ commit, files, filesLoading, width, onSelectParent, onSelectFile }) => {
  const { t } = useTranslation();
  const [fileListMode, setFileListMode] = useState<FileListMode>(
    () => (localStorage.getItem('gitGraphFileListMode') as FileListMode | null) ?? 'path',
  );
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());

  useEffect(() => { setCollapsed(new Set()); }, [files]);

  const toggleFolder = useCallback((folderPath: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(folderPath)) next.delete(folderPath);
      else next.add(folderPath);
      return next;
    });
  }, []);

  const handleFileListMode = useCallback((mode: FileListMode) => {
    setFileListMode(mode);
    localStorage.setItem('gitGraphFileListMode', mode);
  }, []);

  const treeItems = useMemo(() => {
    if (fileListMode !== 'tree') return [];
    const dirTree: DirTree = new Map();
    for (const f of files) insertPath(dirTree, f.path.split('/'), f);
    return flattenDirTree(dirTree, 0, collapsed, '');
  }, [files, fileListMode, collapsed]);

  const fileSummary = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const f of files) {
      const s = f.status[0] ?? '?';
      counts[s] = (counts[s] ?? 0) + 1;
    }
    return counts;
  }, [files]);

  const initials = commit.author
    .split(' ')
    .slice(0, 2)
    .map((w) => w[0]?.toUpperCase() ?? '')
    .join('');
  const dateStr = commit.date.slice(0, 16).replace('T', ' ');
  const filesLabel = filesLoading
    ? t('gitGraph.loadingFiles')
    : `${t('gitGraph.filesChanged')} (${files.length})`;
  return (
    <div
      style={{ width }}
      className="flex shrink-0 flex-col overflow-hidden bg-muted/5"
    >
      {/* Hash */}
      <div className="flex shrink-0 items-center gap-2 border-b border-border/40 px-3 py-2">
        <span className="text-[10px] text-muted-foreground/60">{t('gitGraph.commitLabel')}</span>
        <code className="min-w-0 truncate font-mono text-xs text-foreground/80">{commit.hash}</code>
      </div>
      {/* Message */}
      <p className="shrink-0 px-3 py-2.5 text-sm font-medium leading-snug text-foreground">
        {commit.subject}
      </p>
      {/* Author */}
      <div className="flex shrink-0 items-center gap-2.5 border-t border-border/40 px-3 py-2">
        <div className="flex size-7 shrink-0 items-center justify-center rounded-full bg-violet-600 text-xs font-bold text-white">
          {initials}
        </div>
        <div className="min-w-0">
          <p className="truncate text-sm font-medium text-foreground">{commit.author}</p>
          <p className="text-xs text-muted-foreground/70">{dateStr}</p>
        </div>
      </div>
      {/* Parents */}
      {commit.parents.length > 0 && (
        <div className="shrink-0 border-t border-border/40 px-3 py-2">
          <p className="mb-1 text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/50">
            {t('gitGraph.parent')}
          </p>
          {commit.parents.map((p) => (
            <button
              type="button"
              key={p}
              onClick={() => onSelectParent(p)}
              className="block font-mono text-xs text-blue-400 hover:underline"
            >
              {p.slice(0, 7)}
            </button>
          ))}
        </div>
      )}
      {/* Files */}
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden border-t border-border/40">
        <div className="flex shrink-0 items-center gap-1 px-3 py-1.5">
          <p className="flex-1 text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/50">
            {filesLabel}
          </p>
          <div className="flex items-center overflow-hidden rounded border border-border/40">
            <button
              type="button"
              onClick={() => handleFileListMode('path')}
              className={`px-1.5 py-0.5 ${fileListMode === 'path' ? 'bg-muted text-foreground' : 'text-muted-foreground hover:bg-muted/50'}`}
              title={t('gitGraph.fileViewPath')}
              aria-label={t('gitGraph.fileViewPath')}
            >
              <List className="size-3" />
            </button>
            <button
              type="button"
              onClick={() => handleFileListMode('tree')}
              className={`px-1.5 py-0.5 ${fileListMode === 'tree' ? 'bg-muted text-foreground' : 'text-muted-foreground hover:bg-muted/50'}`}
              title={t('gitGraph.fileViewTree')}
              aria-label={t('gitGraph.fileViewTree')}
            >
              <FolderTree className="size-3" />
            </button>
          </div>
        </div>
        {/* Summary bar */}
        {files.length > 0 && (
          <div className="flex shrink-0 flex-wrap gap-x-3 gap-y-0.5 border-b border-border/40 px-3 pb-2 pt-1">
            {STATUS_ORDER.filter((s) => fileSummary[s]).map((s) => {
              const Icon = FILE_STATUS_ICON[s];
              const cls = FILE_STATUS_CLS[s] ?? 'text-muted-foreground';
              return (
                <span key={s} className={`flex items-center gap-1 text-xs ${cls}`}>
                  <Icon className="size-3 shrink-0" />
                  <span>{fileSummary[s]}</span>
                </span>
              );
            })}
          </div>
        )}
        <div className="min-h-0 flex-1 overflow-y-auto">
          {fileListMode === 'path' && files.map((f, i) => {
            const s = f.status[0] ?? '?';
            const Icon = FILE_STATUS_ICON[s];
            const cls = FILE_STATUS_CLS[s] ?? 'text-muted-foreground';
            return (
              <button
                key={i}
                type="button"
                onClick={() => onSelectFile(f.path)}
                className="flex w-full items-center gap-2 px-3 py-0.5 text-left hover:bg-muted/50 rounded"
              >
                <Icon className={`size-3 shrink-0 ${cls}`} />
                <span className="min-w-0 truncate text-xs text-foreground/70">{f.path}</span>
              </button>
            );
          })}
          {fileListMode === 'tree' && treeItems.map((item, i) => {
            if (item.isFolder) {
              const FolderChevron = collapsed.has(item.fullPath) ? ChevronRight : ChevronDown;
              const folderCounts = STATUS_ORDER.filter((s) => (item.statusCounts ?? {})[s]).map((s) => {
                const Icon = FILE_STATUS_ICON[s];
                const cls = FILE_STATUS_CLS[s] ?? 'text-muted-foreground';
                return (
                  <span key={s} className={`flex items-center gap-0.5 ${cls}`}>
                    <Icon className="size-2.5" />
                    <span className="text-[10px] leading-none">{(item.statusCounts ?? {})[s]}</span>
                  </span>
                );
              });
              return (
                <button
                  key={i}
                  type="button"
                  onClick={() => toggleFolder(item.fullPath)}
                  style={{ paddingLeft: item.depth * 10 + 8 }}
                  className="flex w-full items-center gap-1 py-0.5 pr-2 text-muted-foreground hover:bg-muted/50"
                >
                  <FolderChevron className="size-3 shrink-0" />
                  <span className="min-w-0 flex-1 truncate text-xs">{item.name}</span>
                  {collapsed.has(item.fullPath) && <span className="ml-1 flex shrink-0 items-center gap-1.5">{folderCounts}</span>}
                </button>
              );
            }
            const s = item.file?.status[0] ?? '?';
            const Icon = FILE_STATUS_ICON[s];
            const cls = FILE_STATUS_CLS[s] ?? 'text-muted-foreground';
            return (
              <button
                key={i}
                type="button"
                onClick={() => onSelectFile(item.fullPath)}
                style={{ paddingLeft: item.depth * 10 + 8 }}
                className="flex w-full items-center gap-1.5 py-0.5 pr-2 hover:bg-muted/50"
              >
                <Icon className={`size-3 shrink-0 ${cls}`} />
                <span className="min-w-0 truncate text-xs text-foreground/70">{item.name}</span>
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
};

// ── Staging panel ────────────────────────────────────────────────────────────
// eslint-disable-next-line max-lines-per-function
const StagingPanel: React.FC<{
  repoPath: string;
  width: number;
  onCommitDone: () => void;
  onClose: () => void;
}> = ({ repoPath, width, onCommitDone, onClose }) => {
  const { t } = useTranslation();
  const [status, setStatus] = useState<GitStatusResult | null>(null);
  const [statusLoading, setStatusLoading] = useState(false);
  const [commitMsg, setCommitMsg] = useState('');
  const [commitDesc, setCommitDesc] = useState('');
  const [isCommitting, setIsCommitting] = useState(false);
  const [opError, setOpError] = useState<string | null>(null);

  const refreshStatus = useCallback(async () => {
    if (!repoPath) return;
    setStatusLoading(true);
    setOpError(null);
    try {
      const data = await fetchGitStatus(repoPath);
      setStatus(data);
      if (data.error) setOpError(data.error);
    } catch (error) {
      setOpError(error instanceof Error ? error.message : 'Status error');
    } finally { setStatusLoading(false); }
  }, [repoPath]);

  useEffect(() => { void refreshStatus(); }, [refreshStatus]);

  const handleStageAll   = useCallback(async () => {
    setOpError(null);
    const res = await gitStage(repoPath, []);
    if (!res.ok) setOpError(res.error); else void refreshStatus();
  }, [repoPath, refreshStatus]);

  const handleUnstageAll = useCallback(async () => {
    setOpError(null);
    const res = await gitUnstage(repoPath, []);
    if (!res.ok) setOpError(res.error); else void refreshStatus();
  }, [repoPath, refreshStatus]);

  const handleStageFile   = useCallback(async (file: string) => {
    const res = await gitStage(repoPath, [file]);
    if (!res.ok) setOpError(res.error); else void refreshStatus();
  }, [repoPath, refreshStatus]);

  const handleUnstageFile = useCallback(async (file: string) => {
    const res = await gitUnstage(repoPath, [file]);
    if (!res.ok) setOpError(res.error); else void refreshStatus();
  }, [repoPath, refreshStatus]);

  const handleCommit = useCallback(async () => {
    if (!commitMsg.trim()) return;
    setIsCommitting(true);
    setOpError(null);
    try {
      const res = await gitCommit(repoPath, commitMsg.trim(), commitDesc.trim());
      if (res.ok) { setCommitMsg(''); setCommitDesc(''); onCommitDone(); }
      else setOpError(res.error ?? 'Commit failed');
    } catch (error) {
      setOpError(error instanceof Error ? error.message : 'Commit error');
    } finally { setIsCommitting(false); }
  }, [repoPath, commitMsg, commitDesc, onCommitDone]);

  const stagedCount   = status?.staged.length ?? 0;
  const unstagedCount = status?.unstaged.length ?? 0;
  const canCommit = stagedCount > 0 && commitMsg.trim().length > 0 && !isCommitting;
  const refreshIconCls = `size-3 ${statusLoading ? 'animate-spin' : ''}`;
  const commitBtnLabel = isCommitting ? t('gitGraph.committing') : `${t('gitGraph.commitButton')} (${stagedCount})`;

  return (
    <div style={{ width }} className="flex shrink-0 flex-col overflow-hidden bg-muted/5">
      <div className="flex shrink-0 items-center gap-2 border-b border-border/40 px-3 py-2">
        <span className="flex-1 text-xs font-semibold text-foreground">{t('gitGraph.stagingTitle')}</span>
        <button type="button" onClick={() => void refreshStatus()} className="rounded p-1 text-muted-foreground hover:bg-muted hover:text-foreground" title={t('gitGraph.refreshStatus')}>
          <RefreshCw className={refreshIconCls} />
        </button>
        <button type="button" onClick={onClose} className="rounded p-1 text-muted-foreground hover:bg-muted hover:text-foreground">
          <X className="size-3" />
        </button>
      </div>
      {!!opError && <p className="shrink-0 px-3 py-1.5 text-xs text-red-400">{opError}</p>}
      {/* Unstaged section */}
      <div className="flex shrink-0 items-center gap-2 border-b border-border/40 px-3 py-1">
        <span className="flex-1 text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/50">
          {t('gitGraph.unstaged')} ({unstagedCount})
        </span>
        {unstagedCount > 0 && (
          <button type="button" onClick={() => void handleStageAll()} className="rounded px-1.5 py-0.5 text-[10px] text-emerald-400 hover:bg-muted">
            {t('gitGraph.stageAll')}
          </button>
        )}
      </div>
      <div className="shrink-0 overflow-y-auto" style={{ maxHeight: 120 }}>
        {statusLoading && !status && <p className="px-3 py-1 text-xs text-muted-foreground">{t('gitGraph.loadingFiles')}</p>}
        {(status?.unstaged ?? []).map((f, i) => {
          const s = f.status;
          const scls = s === 'A' || s === '?' ? 'text-emerald-400' : s === 'D' ? 'text-red-400' : 'text-amber-400';
          return (
            <div key={i} className="flex items-center gap-1.5 px-3 py-0.5 hover:bg-muted/40">
              <span className={`w-3 shrink-0 text-[10px] font-bold ${scls}`}>{s}</span>
              <span className="min-w-0 flex-1 truncate text-xs text-foreground/70">{f.path}</span>
              <button type="button" onClick={() => void handleStageFile(f.path)} className="shrink-0 rounded p-0.5 text-emerald-400 hover:bg-muted" title={t('gitGraph.stageFile')}>
                <Plus className="size-3" />
              </button>
            </div>
          );
        })}
      </div>
      {/* Staged section */}
      <div className="flex shrink-0 items-center gap-2 border-b border-border/40 border-t border-border/40 px-3 py-1">
        <span className="flex-1 text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/50">
          {t('gitGraph.staged')} ({stagedCount})
        </span>
        {stagedCount > 0 && (
          <button type="button" onClick={() => void handleUnstageAll()} className="rounded px-1.5 py-0.5 text-[10px] text-muted-foreground hover:bg-muted">
            {t('gitGraph.unstageAll')}
          </button>
        )}
      </div>
      <div className="shrink-0 overflow-y-auto" style={{ maxHeight: 120 }}>
        {(status?.staged ?? []).map((f, i) => {
          const s = f.status;
          const scls = s === 'A' ? 'text-emerald-400' : s === 'D' ? 'text-red-400' : 'text-amber-400';
          return (
            <div key={i} className="flex items-center gap-1.5 px-3 py-0.5 hover:bg-muted/40">
              <span className={`w-3 shrink-0 text-[10px] font-bold ${scls}`}>{s}</span>
              <span className="min-w-0 flex-1 truncate text-xs text-foreground/70">{f.path}</span>
              <button type="button" onClick={() => void handleUnstageFile(f.path)} className="shrink-0 rounded p-0.5 text-muted-foreground hover:bg-muted" title={t('gitGraph.unstageFile')}>
                <Minus className="size-3" />
              </button>
            </div>
          );
        })}
      </div>
      {/* Commit message */}
      <div className="flex min-h-0 flex-1 flex-col gap-2 border-t border-border/40 p-3">
        <input
          type="text"
          value={commitMsg}
          onChange={(e) => setCommitMsg(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter' && canCommit) void handleCommit(); }}
          placeholder={t('gitGraph.commitMsgPlaceholder')}
          className="w-full rounded border border-border/40 bg-muted/30 px-2 py-1.5 text-xs text-foreground placeholder:text-muted-foreground/50 focus:outline-none focus:ring-1 focus:ring-ring"
        />
        <textarea
          value={commitDesc}
          onChange={(e) => setCommitDesc(e.target.value)}
          placeholder={t('gitGraph.commitDescPlaceholder')}
          rows={2}
          className="w-full resize-none rounded border border-border/40 bg-muted/30 px-2 py-1.5 text-xs text-foreground placeholder:text-muted-foreground/50 focus:outline-none focus:ring-1 focus:ring-ring"
        />
        <button type="button" onClick={() => void handleCommit()} disabled={!canCommit}
          className="w-full rounded bg-emerald-600 py-1.5 text-xs font-semibold text-white hover:bg-emerald-700 disabled:opacity-40">
          {commitBtnLabel}
        </button>
      </div>
    </div>
  );
};

// ── Commit row ────────────────────────────────────────────────────────────────
const CommitRow: React.FC<{
  commit: AssignedCommit;
  isSelected: boolean;
  onSelect: (hash: string) => void;
  onContextMenu: (clientX: number, clientY: number, hash: string) => void;
}> = ({ commit, isSelected, onSelect, onContextMenu }) => {
  const rowCls = isSelected
    ? 'flex min-w-0 items-center gap-1.5 px-2 bg-muted/70 cursor-pointer'
    : 'flex min-w-0 items-center gap-1.5 px-2 hover:bg-muted/40 cursor-pointer';
  return (
    <div
      style={{ height: ROW_H, minHeight: ROW_H }}
      className={rowCls}
      role="button"
      tabIndex={0}
      onClick={() => onSelect(commit.hash)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') onSelect(commit.hash);
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        onContextMenu(e.clientX, e.clientY, commit.hash);
      }}
    >
      <span className="w-[50px] shrink-0 font-mono text-[10px] text-foreground/80">
        {commit.short_hash}
      </span>
      <span className="min-w-0 flex-1 truncate text-xs text-foreground">{commit.subject}</span>
      <span
        style={{ width: AUTHOR_COL_W }}
        className="shrink-0 truncate pl-2 text-[10px] text-foreground/70"
      >
        {commit.author}
      </span>
      <span
        style={{ width: DATE_COL_W }}
        className="shrink-0 pl-2 text-right text-[10px] text-foreground/80"
      >
        {relDate(commit.date)}
      </span>
    </div>
  );
};

// ── Context menu ──────────────────────────────────────────────────────────────
interface CtxMenuState {
  x: number;
  y: number;
  commit: AssignedCommit;
}

const ContextMenu: React.FC<{ state: CtxMenuState; onClose: () => void }> = ({
  state,
  onClose,
}) => {
  const { t } = useTranslation();
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [onClose]);

  const handleCopy = (item: CtxMenuItem) => {
    const text = item.getValue(state.commit);
    void navigator.clipboard.writeText(text).then(() => {
      toast.success(t('gitGraph.copiedNotice'));
    });
    onClose();
  };

  return (
    <div
      ref={ref}
      style={{ position: 'fixed', top: state.y, left: state.x }}
      className="z-50 min-w-[170px] rounded-md border border-border bg-popover py-1 shadow-lg"
      role="menu"
    >
      {CTX_ITEMS.map((item) => (
        <React.Fragment key={item.key}>
          {!!item.sep && <div className="my-1 border-t border-border/40" />}
          <button
            type="button"
            role="menuitem"
            className="flex w-full items-center px-3 py-1.5 text-xs text-foreground hover:bg-muted"
            onClick={() => handleCopy(item)}
          >
            {t(item.labelKey)}
          </button>
        </React.Fragment>
      ))}
    </div>
  );
};

// ── API helpers ───────────────────────────────────────────────────────────────
interface GitLogResponse {
  commits: RawCommit[];
  error: string | null;
}

async function fetchGitLog(path: string): Promise<GitLogResponse> {
  const res = await fetch(`/api/git/log?path=${encodeURIComponent(path)}`);
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  return res.json() as Promise<GitLogResponse>;
}

async function pickDirectory(): Promise<string | null> {
  try {
    const res = await fetch('/api/browse/pick-directory', { method: 'POST' });
    if (!res.ok) return null;
    const data = (await res.json()) as { path?: string };
    return data.path ?? null;
  } catch {
    return null;
  }
}

// ── GitGraphView ──────────────────────────────────────────────────────────────
// eslint-disable-next-line max-lines-per-function -- organically large: state + 2 layout modes + context menu
export const GitGraphView: React.FC = () => {
  const { t } = useTranslation();
  const [path, setPath] = useState(() => localStorage.getItem('gitGraphPath') ?? '');
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [commits, setCommits] = useState<AssignedCommit[]>([]);
  const [edges, setEdges] = useState<GraphEdge[]>([]);
  const [maxLane, setMaxLane] = useState(0);
  const [selectedHash, setSelectedHash] = useState<string | null>(null);
  const [isHorizontal, setIsHorizontal] = useState(
    () => localStorage.getItem('gitGraphLayout') === 'horizontal',
  );
  const [ctxMenu, setCtxMenu] = useState<CtxMenuState | null>(null);
  const [commitFiles, setCommitFiles] = useState<FileChange[]>([]);
  const [filesLoading, setFilesLoading] = useState(false);
  const [selectedDiffFile, setSelectedDiffFile] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [stagingActive, setStagingActive] = useState(false);
  const [toolbarBusy, setToolbarBusy] = useState<'fetch' | 'pull' | 'push' | null>(null);

  const filteredCommits = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return commits;
    return commits.filter((c) =>
      c.subject.toLowerCase().includes(q) ||
      c.hash.startsWith(q) ||
      c.author.toLowerCase().includes(q),
    );
  }, [commits, searchQuery]);

  const listScrollRef = useRef<HTMLDivElement>(null);
  const rowVirtualizer = useVirtualizer({
    count: commits.length,
    getScrollElement: () => listScrollRef.current,
    estimateSize: () => ROW_H,
    overscan: 20,
  });
  const [detailPanelWidth, setDetailPanelWidth] = useState(() => {
    const saved = localStorage.getItem('gitGraphDetailPanelWidth');
    return saved ? parseInt(saved, 10) : DETAIL_PANEL_W;
  });

  const handleDetailResizeStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startW = detailPanelWidth;
    const onMove = (ev: MouseEvent) => {
      const newW = Math.max(200, Math.min(600, startW + (startX - ev.clientX)));
      setDetailPanelWidth(newW);
    };
    const onUp = () => {
      setDetailPanelWidth((w) => { localStorage.setItem('gitGraphDetailPanelWidth', String(w)); return w; });
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
    };
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  }, [detailPanelWidth]);

  const loadGraph = useCallback(async (repoPath: string) => {
    const trimmed = repoPath.trim();
    if (!trimmed) return;
    setLoading(true);
    setLoadError(null);
    try {
      const data = await fetchGitLog(trimmed);
      if (data.error) {
        setLoadError(data.error);
        setCommits([]);
        setEdges([]);
        setMaxLane(0);
      } else {
        const result = processCommits(data.commits);
        setCommits(result.commits);
        setEdges(result.edges);
        setMaxLane(result.maxLane);
        setSelectedHash(null);
        localStorage.setItem('gitGraphPath', trimmed);
      }
    } catch (error) {
      setLoadError(error instanceof Error ? error.message : 'Unknown error');
    } finally {
      setLoading(false);
    }
  }, []);

  const handleBrowse = useCallback(async () => {
    const picked = await pickDirectory();
    if (picked) {
      setPath(picked);
      await loadGraph(picked);
    }
  }, [loadGraph]);

  const handleLoad = useCallback(() => {
    void loadGraph(path);
  }, [path, loadGraph]);

  const handleContextMenu = useCallback(
    (clientX: number, clientY: number, hash: string) => {
      const commit = commits.find((c) => c.hash === hash);
      if (!commit) return;
      setCtxMenu({ x: clientX, y: clientY, commit });
    },
    [commits],
  );

  const closeCtxMenu = useCallback(() => setCtxMenu(null), []);
  const toggleLayout = useCallback(() => {
    setIsHorizontal((p) => {
      const next = !p;
      localStorage.setItem('gitGraphLayout', next ? 'horizontal' : 'vertical');
      return next;
    });
  }, []);

  const openStaging = useCallback(() => { setStagingActive(true); }, []);
  const handleCommitDone = useCallback(() => {
    setStagingActive(false);
    void loadGraph(path);
  }, [loadGraph, path]);

  const handleFetch = useCallback(async () => {
    if (!path.trim()) return;
    setToolbarBusy('fetch');
    try {
      const res = await gitFetch(path.trim());
      if (res.ok) { toast.success(res.output || t('gitGraph.fetchDone')); void loadGraph(path); }
      else toast.error(res.error ?? t('gitGraph.fetchError'));
    } catch (error) { toast.error(error instanceof Error ? error.message : 'Fetch error'); }
    finally { setToolbarBusy(null); }
  }, [path, loadGraph, t]);

  const handlePull = useCallback(async () => {
    if (!path.trim()) return;
    setToolbarBusy('pull');
    try {
      const res = await gitPull(path.trim());
      if (res.ok) { toast.success(res.output || t('gitGraph.pullDone')); void loadGraph(path); }
      else toast.error(res.error ?? t('gitGraph.pullError'));
    } catch (error) { toast.error(error instanceof Error ? error.message : 'Pull error'); }
    finally { setToolbarBusy(null); }
  }, [path, loadGraph, t]);

  const handlePush = useCallback(async () => {
    if (!path.trim()) return;
    setToolbarBusy('push');
    try {
      const res = await gitPush(path.trim());
      if (res.ok) toast.success(res.output || t('gitGraph.pushDone'));
      else toast.error(res.error ?? t('gitGraph.pushError'));
    } catch (error) { toast.error(error instanceof Error ? error.message : 'Push error'); }
    finally { setToolbarBusy(null); }
  }, [path, t]);

  // Auto-load saved repo on first mount
  useEffect(() => {
    const saved = localStorage.getItem('gitGraphPath');
    if (saved) void loadGraph(saved);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Fetch changed files when a commit is selected
  useEffect(() => {
    setSelectedDiffFile(null);
    if (!selectedHash || !path.trim()) {
      setCommitFiles([]);
      return;
    }
    setFilesLoading(true);
    fetch(`/api/git/commit-files?path=${encodeURIComponent(path.trim())}&hash=${selectedHash}`)
      .then((r) => r.json())
      .then((data: { files?: FileChange[] }) => { setCommitFiles(data.files ?? []); })
      .catch(() => { setCommitFiles([]); })
      .finally(() => { setFilesLoading(false); });
  }, [selectedHash, path]);

  const selectedCommit = commits.find((c) => c.hash === selectedHash) ?? null;
  const refreshCls = loading ? 'size-3.5 animate-spin' : 'size-3.5';
  const layoutIcon = isHorizontal
    ? <ArrowUpDown className="size-3.5" />
    : <ArrowLeftRight className="size-3.5" />;
  const layoutTitle = isHorizontal ? t('gitGraph.switchVertical') : t('gitGraph.switchHorizontal');

  // ── Body content (no nested ternaries) ───────────────────────────────────
  let bodyContent: React.ReactNode;
  if (loading) {
    bodyContent = (
      <div className="flex flex-1 items-center justify-center text-sm text-muted-foreground">
        {t('gitGraph.loading')}
      </div>
    );
  } else if (loadError) {
    bodyContent = (
      <div className="flex flex-1 items-center justify-center px-6">
        <p className="max-w-md text-center text-sm text-destructive">{loadError}</p>
      </div>
    );
  } else if (commits.length === 0) {
    const emptyText = path ? t('gitGraph.noCommits') : t('gitGraph.welcomeText');
    bodyContent = (
      <div className="flex flex-1 items-center justify-center">
        <div className="text-center">
          <GitBranch className="mx-auto mb-3 size-10 text-muted-foreground/30" />
          <p className="text-sm text-muted-foreground">{emptyText}</p>
        </div>
      </div>
    );
  } else if (isHorizontal) {
    bodyContent = (
      <div className="flex flex-col flex-1 min-h-0 min-w-0 overflow-hidden">
        <div className="shrink-0 min-w-0 overflow-x-auto overflow-y-hidden border-b border-border/40 bg-background/50 px-4">
          <HGraphSvg
            commits={commits}
            edges={edges}
            maxLane={maxLane}
            selectedHash={selectedHash}
            onSelect={setSelectedHash}
            onContextMenu={handleContextMenu}
          />
        </div>
        <div className="min-h-0 flex-1 overflow-auto px-4">
          {commits.map((c) => (
            <CommitRow
              key={c.hash}
              commit={c}
              isSelected={c.hash === selectedHash}
              onSelect={setSelectedHash}
              onContextMenu={handleContextMenu}
            />
          ))}
        </div>
      </div>
    );
  } else {
    const graphColW = (maxLane + 1) * LANE_W + SVG_PAD_R;
    const detailPanelEl = selectedCommit && !stagingActive ? (
      <CommitDetailPanel
        commit={selectedCommit}
        files={commitFiles}
        filesLoading={filesLoading}
        width={detailPanelWidth}
        onSelectParent={setSelectedHash}
        onSelectFile={setSelectedDiffFile}
      />
    ) : null;
    const stagingPanelEl = stagingActive ? (
      <StagingPanel
        repoPath={path}
        width={detailPanelWidth}
        onCommitDone={handleCommitDone}
        onClose={() => setStagingActive(false)}
      />
    ) : null;
    const rightPanelEl = stagingActive ? stagingPanelEl : detailPanelEl;
    const showResizeHandle = stagingActive || !!detailPanelEl;

    const fetchBtnCls = `size-3.5 ${toolbarBusy === 'fetch' ? 'animate-spin' : ''}`;
    const pullBtnCls  = `size-3.5 ${toolbarBusy === 'pull'  ? 'animate-bounce' : ''}`;
    const pushBtnCls  = `size-3.5 ${toolbarBusy === 'push'  ? 'animate-bounce' : ''}`;
    const tbBtnBase   = 'flex items-center gap-1 rounded px-2 py-1 text-xs text-foreground/70 hover:bg-muted disabled:opacity-40';
    const wipRowCls = stagingActive
      ? 'flex w-full items-center gap-2 border-b border-border/40 px-4 py-1.5 text-left text-xs bg-muted/50 text-foreground cursor-pointer'
      : 'flex w-full items-center gap-2 border-b border-border/40 px-4 py-1.5 text-left text-xs text-muted-foreground/60 hover:bg-muted/30 hover:text-foreground/80 cursor-pointer';

    // Filtered (search) vs normal (virtualizer) scroll content
    let scrollInnerEl: React.ReactNode;
    if (searchQuery.trim()) {
      const noMatch = filteredCommits.length === 0;
      const noMatchEl = noMatch
        ? <p className="p-4 text-xs text-muted-foreground">{t('gitGraph.noResults')}</p>
        : null;
      scrollInnerEl = (
        <div ref={listScrollRef} className="flex flex-1 min-h-0 overflow-y-auto overflow-x-hidden px-4">
          <div className="min-w-0 w-full">
            {noMatchEl}
            {filteredCommits.map((c) => (
              <CommitRow key={c.hash} commit={c} isSelected={c.hash === selectedHash} onSelect={setSelectedHash} onContextMenu={handleContextMenu} />
            ))}
          </div>
        </div>
      );
    } else {
      scrollInnerEl = (
        <div ref={listScrollRef} className="flex flex-1 min-h-0 overflow-y-auto overflow-x-hidden px-4">
          <div style={{ width: REFS_COL_W }} className="shrink-0">
            <div style={{ height: rowVirtualizer.getTotalSize(), position: 'relative' }}>
              {rowVirtualizer.getVirtualItems().map((vi) => {
                const c = commits[vi.index];
                const refCellCls = `flex items-center gap-1 overflow-hidden ${c.hash === selectedHash ? 'bg-muted/70' : ''}`;
                return (
                  <div key={c.hash} style={{ position: 'absolute', top: vi.start, height: ROW_H, left: 0, right: 0 }} className={refCellCls}>
                    {c.refs.map((r) => <RefBadge key={r} refStr={r} />)}
                  </div>
                );
              })}
            </div>
          </div>
          <GraphSvg
            commits={commits}
            edges={edges}
            maxLane={maxLane}
            selectedHash={selectedHash}
            onSelect={setSelectedHash}
            onContextMenu={handleContextMenu}
          />
          <div className="min-w-0 flex-1 border-l border-border/40">
            <div style={{ height: rowVirtualizer.getTotalSize(), position: 'relative' }}>
              {rowVirtualizer.getVirtualItems().map((vi) => {
                const c = commits[vi.index];
                return (
                  <div key={c.hash} style={{ position: 'absolute', top: vi.start, height: ROW_H, left: 0, right: 0 }}>
                    <CommitRow commit={c} isSelected={c.hash === selectedHash} onSelect={setSelectedHash} onContextMenu={handleContextMenu} />
                  </div>
                );
              })}
            </div>
          </div>
        </div>
      );
    }

    const commitTableEl = (
      <div className="flex flex-col flex-1 min-h-0 overflow-hidden">
        {/* Toolbar */}
        <div className="flex shrink-0 items-center gap-0.5 border-b border-border/40 px-2 py-1">
          <button type="button" onClick={() => void handleFetch()} disabled={!path.trim() || !!toolbarBusy} className={tbBtnBase} title={t('gitGraph.fetchTitle')}>
            <RefreshCw className={fetchBtnCls} /><span>{t('gitGraph.fetchLabel')}</span>
          </button>
          <button type="button" onClick={() => void handlePull()} disabled={!path.trim() || !!toolbarBusy} className={tbBtnBase} title={t('gitGraph.pullTitle')}>
            <ArrowDown className={pullBtnCls} /><span>{t('gitGraph.pullLabel')}</span>
          </button>
          <button type="button" onClick={() => void handlePush()} disabled={!path.trim() || !!toolbarBusy} className={tbBtnBase} title={t('gitGraph.pushTitle')}>
            <ArrowUp className={pushBtnCls} /><span>{t('gitGraph.pushLabel')}</span>
          </button>
          <div className="mx-1 h-4 w-px bg-border/40" />
          <Search className="size-3 shrink-0 text-muted-foreground/50" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder={t('gitGraph.searchPlaceholder')}
            className="flex-1 min-w-0 bg-transparent text-xs text-foreground placeholder:text-muted-foreground/40 focus:outline-none"
          />
          {!!searchQuery && (
            <button type="button" onClick={() => setSearchQuery('')} className="shrink-0 rounded p-0.5 text-muted-foreground hover:bg-muted hover:text-foreground">
              <X className="size-3" />
            </button>
          )}
        </div>
        {/* Column headers */}
        <div className="flex shrink-0 items-center border-b border-border/40 bg-background px-4 py-1.5 text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/50">
          <div style={{ width: REFS_COL_W }} className="shrink-0">{t('gitGraph.colBranchTag')}</div>
          <div style={{ width: graphColW }} className="shrink-0 text-center">{t('gitGraph.colGraph')}</div>
          <div className="min-w-0 flex-1 pl-3">{t('gitGraph.colCommit')}</div>
          <div style={{ width: AUTHOR_COL_W }} className="shrink-0 pl-2">{t('gitGraph.colAuthor')}</div>
          <div style={{ width: DATE_COL_W }} className="shrink-0 pl-2 text-right">{t('gitGraph.colDate')}</div>
        </div>
        {/* WIP row */}
        <button type="button" onClick={openStaging} className={wipRowCls}>
          <Circle className="size-3.5 shrink-0 text-muted-foreground/40" />
          <span className="min-w-0 flex-1 truncate italic">{t('gitGraph.wipRowPlaceholder')}</span>
        </button>
        {scrollInnerEl}
      </div>
    );

    const centerContent = selectedDiffFile && selectedHash ? (
      <DiffViewer
        repoPath={path}
        hash={selectedHash}
        file={selectedDiffFile}
        onClose={() => setSelectedDiffFile(null)}
      />
    ) : commitTableEl;

    bodyContent = (
      <div className="flex flex-1 min-h-0 overflow-hidden">
        <BranchPanel commits={commits} selectedHash={selectedHash} onSelect={setSelectedHash} />
        {centerContent}
        {showResizeHandle && (
          <div
            onMouseDown={handleDetailResizeStart}
            className="w-1 shrink-0 cursor-col-resize border-l border-border/40 hover:border-primary/60 hover:bg-primary/10 transition-colors"
          />
        )}
        {rightPanelEl}
      </div>
    );
  }


  const ctxMenuEl = ctxMenu ? (
    <ContextMenu state={ctxMenu} onClose={closeCtxMenu} />
  ) : null;

  return (
    <div className="flex h-full w-full flex-col overflow-hidden bg-background">
      {/* Path bar */}
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-2">
        <GitBranch className="size-4 shrink-0 text-muted-foreground" />
        <span className="shrink-0 text-sm font-medium text-foreground">{t('gitGraph.title')}</span>
        <input
          type="text"
          value={path}
          onChange={(e) => setPath(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') handleLoad();
          }}
          placeholder={t('gitGraph.placeholder')}
          className="min-w-0 flex-1 rounded border border-border bg-muted/30 px-2 py-1 text-xs text-foreground placeholder:text-muted-foreground/50 focus:outline-none focus:ring-1 focus:ring-ring"
        />
        <button
          type="button"
          onClick={handleBrowse}
          className="btn-icon shrink-0"
          title={t('gitGraph.browseFolderAria')}
          aria-label={t('gitGraph.browseFolderAria')}
        >
          <FolderOpen className="size-3.5" />
        </button>
        <button
          type="button"
          onClick={handleLoad}
          disabled={loading || !path.trim()}
          className="btn-icon shrink-0 disabled:opacity-40"
          title={t('gitGraph.reloadAria')}
          aria-label={t('gitGraph.reloadAria')}
        >
          <RefreshCw className={refreshCls} />
        </button>
        <button
          type="button"
          onClick={toggleLayout}
          disabled={commits.length === 0}
          className="btn-icon shrink-0 disabled:opacity-40"
          title={layoutTitle}
          aria-label={layoutTitle}
        >
          {layoutIcon}
        </button>
      </div>

      {bodyContent}
      {ctxMenuEl}
    </div>
  );
};
