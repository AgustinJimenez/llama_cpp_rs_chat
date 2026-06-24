import { AlignJustify, ChevronDown, ChevronRight, Columns2, Filter, X } from 'lucide-react';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { DIFF_LINE_CLS, HASH_TRUNC_LEN } from './constants';
import type { DiffLine, DiffViewMode, SplitRow } from './types';
import { diffCache, parseDiff, splitSideCls, toSplitRows } from './utils';

const CONTENT_KEY_LEN = 30;

const DIFF_MODES: ReadonlyArray<{
  key: DiffViewMode;
  labelKey: string;
  Icon: React.ComponentType<{ className?: string }>;
}> = [
  { key: 'split', labelKey: 'gitGraph.diffModeSplit', Icon: Columns2 },
  { key: 'inline', labelKey: 'gitGraph.diffModeInline', Icon: AlignJustify },
  { key: 'hunk', labelKey: 'gitGraph.diffModeHunk', Icon: Filter },
];

const DiffInlineTable: React.FC<{ lines: DiffLine[] }> = ({ lines }) => (
  <table className="w-full border-collapse font-mono text-xs">
    <tbody>
      {lines.map((ln) => {
        const cls = DIFF_LINE_CLS[ln.type];
        if (ln.type === 'hunk' || ln.type === 'meta') {
          return (
            <tr key={`${ln.type}-${ln.content.slice(0, CONTENT_KEY_LEN)}`} className={cls}>
              <td colSpan={3} className="whitespace-pre px-3 py-0">
                {ln.content}
              </td>
            </tr>
          );
        }
        const oldNum = ln.type === 'context' || ln.type === 'remove' ? ln.oldLine : undefined;
        const newNum = ln.type === 'context' || ln.type === 'add' ? ln.newLine : undefined;
        return (
          <tr key={`${ln.type}-${oldNum ?? '?'}-${newNum ?? '?'}`} className={cls}>
            <td className="w-10 select-none border-r border-border/20 px-2 text-right text-[10px] text-muted-foreground/40">
              {oldNum}
            </td>
            <td className="w-10 select-none border-r border-border/20 px-2 text-right text-[10px] text-muted-foreground/40">
              {newNum}
            </td>
            <td className="whitespace-pre px-3 py-0">{ln.content}</td>
          </tr>
        );
      })}
    </tbody>
  </table>
);

const DiffSplitTable: React.FC<{ rows: SplitRow[] }> = ({ rows }) => (
  <table className="w-full table-fixed border-collapse font-mono text-xs">
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
            <tr key={`${row.type}-${row.left?.content?.slice(0, CONTENT_KEY_LEN) ?? i}`} className={spanCls}>
              <td colSpan={4} className="whitespace-pre px-3 py-0">
                {row.left?.content ?? ''}
              </td>
            </tr>
          );
        }
        const leftCls = splitSideCls(row.left, 'left');
        const rightCls = splitSideCls(row.right, 'right');
        const leftNum = row.left?.oldLine ?? '';
        const rightNum = row.right?.newLine ?? '';
        const splitKey = `s-${row.left?.oldLine ?? ''}-${row.right?.newLine ?? ''}-${i}`;
        return (
          <tr key={splitKey}>
            <td
              className={`select-none border-r border-border/20 px-2 text-right text-[10px] text-muted-foreground/40 ${leftCls}`}
            >
              {leftNum}
            </td>
            <td className={`whitespace-pre border-r border-border/30 px-2 py-0 ${leftCls}`}>
              {row.left?.content ?? ' '}
            </td>
            <td
              className={`select-none border-r border-border/20 px-2 text-right text-[10px] text-muted-foreground/40 ${rightCls}`}
            >
              {rightNum}
            </td>
            <td className={`whitespace-pre px-2 py-0 ${rightCls}`}>{row.right?.content ?? ' '}</td>
          </tr>
        );
      })}
    </tbody>
  </table>
);

export const DiffViewer: React.FC<{
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
  const [metaCollapsed, setMetaCollapsed] = useState(true);

  useEffect(() => {
    const cacheKey = `${hash}:${file}`;
    const cached = diffCache.get(cacheKey);
    if (cached) {
      setLines(cached);
      setDiffLoading(false);
      return;
    }
    setDiffLoading(true);
    setDiffError(null);
    const url =
      `/api/git/file-diff?path=${encodeURIComponent(repoPath)}` +
      `&hash=${encodeURIComponent(hash)}&file=${encodeURIComponent(file)}`;
    fetch(url)
      .then((r) => r.json())
      .then((data: { diff?: string; error?: string }) => {
        if (data.error) {
          setDiffError(data.error);
          return;
        }
        const parsed = parseDiff(data.diff ?? '');
        diffCache.set(cacheKey, parsed);
        setLines(parsed);
      })
      .catch((error: unknown) => {
        setDiffError(error instanceof Error ? error.message : 'Unknown error');
      })
      .finally(() => {
        setDiffLoading(false);
      });
  }, [repoPath, hash, file]);

  const visibleLines = useMemo(
    () => (metaCollapsed ? lines.filter((l) => l.type !== 'meta') : lines),
    [lines, metaCollapsed],
  );
  const splitRows = useMemo(() => toSplitRows(visibleLines), [visibleLines]);
  const hunkLines = useMemo(() => visibleLines.filter((l) => l.type !== 'context'), [visibleLines]);
  const fileName = file.split('/').pop() ?? file;

  const handleModeChange = useCallback((mode: DiffViewMode) => {
    setViewMode(mode);
    localStorage.setItem('gitGraphDiffMode', mode);
  }, []);

  let viewContent: React.ReactNode = null;
  if (viewMode === 'split') viewContent = <DiffSplitTable rows={splitRows} />;
  else if (viewMode === 'hunk') viewContent = <DiffInlineTable lines={hunkLines} />;
  else viewContent = <DiffInlineTable lines={visibleLines} />;
  const chevronIcon = metaCollapsed ? <ChevronRight className="size-3.5" /> : null;
  const metaBtnTitle = metaCollapsed ? t('gitGraph.showCommitHeader') : t('gitGraph.hideCommitHeader');
  const metaBtnCls = metaCollapsed ? 'text-muted-foreground/50' : 'text-muted-foreground hover:text-foreground';
  const loadingEl = diffLoading ? (
    <p className="p-4 text-xs text-muted-foreground">{t('gitGraph.loadingFiles')}</p>
  ) : null;

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden border-l border-border bg-background">
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-4 py-2">
        <span className="font-mono text-xs text-muted-foreground">
          {hash.slice(0, HASH_TRUNC_LEN)}
        </span>
        <span className="text-muted-foreground/40">·</span>
        <span className="flex-1 truncate font-mono text-sm font-medium text-foreground">
          {fileName}
        </span>
        <div className="flex shrink-0 items-center overflow-hidden rounded border border-border">
          {DIFF_MODES.map((m) => {
            const activeCls =
              viewMode === m.key
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
          onClick={() => setMetaCollapsed((p) => !p)}
          className={`shrink-0 rounded p-1 hover:bg-muted ${metaBtnCls}`}
          title={metaBtnTitle}
        >
          {chevronIcon}
          {!metaCollapsed && <ChevronDown className="size-3.5" />}
        </button>
        <button
          type="button"
          onClick={onClose}
          className="shrink-0 rounded p-1 text-muted-foreground hover:bg-muted hover:text-foreground"
          aria-label={t('gitGraph.closeDiff')}
        >
          <X className="size-3.5" />
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        {loadingEl}
        {!!diffError && <p className="p-4 text-xs text-red-400">{diffError}</p>}
        {!diffLoading && !diffError && viewContent}
      </div>
    </div>
  );
};
