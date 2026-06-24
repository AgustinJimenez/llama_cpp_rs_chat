import {
  Archive,
  ArchiveRestore,
  ArrowDown,
  ArrowUp,
  ChevronDown,
  ChevronUp,
  GitBranch,
  Pencil,
  RefreshCw,
  Search,
  X,
} from 'lucide-react';
import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import type { AssignedCommit, GraphEdge } from '../../../utils/gitGraph';

import { AUTHOR_COL_W, DATE_COL_W, LANE_W, NODE_R, REFS_COL_W, ROW_H, SVG_PAD_R } from './constants';
import type { FileChange } from './types';
import { VirtualCommitScroll } from './VirtualCommitScroll';

interface CommitListPaneProps {
  adjCommits: AssignedCommit[];
  adjEdges: GraphEdge[];
  maxLane: number;
  selectedHash: string | null;
  filteredCommits: AssignedCommit[];
  searchQuery: string;
  stagingActive: boolean;
  wipMsg: string;
  wipChangesCount: number;
  toolbarBusy: 'fetch' | 'pull' | 'push' | null;
  path: string;
  displayRows: Array<{ commit: AssignedCommit; refIndex: number }>;
  onSelectHash: (hash: string) => void;
  onContextMenu: (clientX: number, clientY: number, hash: string) => void;
  onDoubleClick: (hash: string) => void;
  onOpenStaging: () => void;
  onSetWipMsg: (msg: string) => void;
  onSetSearchQuery: (q: string) => void;
  onFetch: () => void;
  onPull: () => void;
  onPush: () => void;
  onStash: () => void;
  onPop: () => void;
  onCreateBranch: () => void;
}

export type { FileChange };

export const CommitListPane: React.FC<CommitListPaneProps> = ({
  adjCommits, adjEdges, maxLane, selectedHash,
  filteredCommits, searchQuery, stagingActive, wipMsg, wipChangesCount,
  toolbarBusy, path, displayRows,
  onSelectHash, onContextMenu, onDoubleClick, onOpenStaging,
  onSetWipMsg, onSetSearchQuery, onFetch, onPull, onPush, onStash, onPop, onCreateBranch,
}) => {
  const { t } = useTranslation();
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchMatchIdx, setSearchMatchIdx] = useState(0);
  const searchInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => { setSearchOpen(false); setSearchMatchIdx(0); }, [path]);

  const graphColW = (maxLane + 1) * LANE_W + SVG_PAD_R;
  const tbBtnBase = 'flex items-center gap-1.5 rounded px-2.5 py-1.5 text-sm text-foreground/90 hover:bg-muted disabled:opacity-40';
  const wipRowBg = stagingActive ? 'bg-muted/40' : 'hover:bg-muted/20';
  const fetchBtnCls = `size-4 ${toolbarBusy === 'fetch' ? 'animate-spin' : ''}`;
  const pullBtnCls = `size-4 ${toolbarBusy === 'pull' ? 'animate-bounce' : ''}`;
  const pushBtnCls = `size-4 ${toolbarBusy === 'push' ? 'animate-bounce' : ''}`;
  const disabled = !path.trim() || !!toolbarBusy;
  const matchCount = filteredCommits.length;
  const safeIdx = matchCount > 0 ? Math.min(searchMatchIdx, matchCount - 1) : 0;
  const scrollToHash = searchOpen && matchCount > 0 ? filteredCommits[safeIdx].hash : null;

  useEffect(() => {
    if (scrollToHash) onSelectHash(scrollToHash);
  }, [scrollToHash, onSelectHash]);

  useEffect(() => {
    if (searchOpen) searchInputRef.current?.focus();
  }, [searchOpen]);

  const openSearch = () => setSearchOpen(true);
  const closeSearch = () => {
    setSearchOpen(false);
    onSetSearchQuery('');
    setSearchMatchIdx(0);
  };
  const handleSearchQueryChange = (q: string) => {
    onSetSearchQuery(q);
    setSearchMatchIdx(0);
  };
  const searchNext = () => {
    if (matchCount === 0) return;
    setSearchMatchIdx((i) => (i + 1) % matchCount);
  };
  const searchPrev = () => {
    if (matchCount === 0) return;
    setSearchMatchIdx((i) => (i - 1 + matchCount) % matchCount);
  };

  const searchBarEl = searchOpen && (
    <div className="absolute inset-x-0 top-0 z-10 flex justify-center px-4 pt-2">
      <div className="flex items-center gap-2 rounded-lg border border-border bg-background px-4 py-2 shadow-lg">
        <Search className="size-4 shrink-0 text-muted-foreground/60" />
        <input
          ref={searchInputRef}
          type="text"
          value={searchQuery}
          onChange={(e) => handleSearchQueryChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Escape') closeSearch();
            else if (e.key === 'Enter') {
              if (e.shiftKey) searchPrev(); else searchNext();
            }
          }}
          placeholder={t('gitGraph.searchPlaceholder')}
          className="w-52 bg-transparent text-sm text-foreground placeholder:text-muted-foreground/40 focus:outline-none"
        />
        {searchQuery.trim() && matchCount > 0 && (
          <span className="shrink-0 text-xs tabular-nums text-muted-foreground/70">
            {safeIdx + 1} / {matchCount}
          </span>
        )}
        {searchQuery.trim() && matchCount === 0 && (
          <span className="shrink-0 text-xs text-destructive/80">{t('gitGraph.noResults')}</span>
        )}
        <div className="mx-1 h-4 w-px bg-border/40" />
        <button type="button" onClick={searchPrev} disabled={matchCount === 0} className="rounded p-1 hover:bg-muted disabled:opacity-30" title="Previous (Shift+Enter)">
          <ChevronUp className="size-4" />
        </button>
        <button type="button" onClick={searchNext} disabled={matchCount === 0} className="rounded p-1 hover:bg-muted disabled:opacity-30" title="Next (Enter)">
          <ChevronDown className="size-4" />
        </button>
        <button type="button" onClick={closeSearch} className="rounded p-1 hover:bg-muted" title="Close (Esc)">
          <X className="size-4" />
        </button>
      </div>
    </div>
  );

  return (
    <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden">
      {searchBarEl}
      <div className="flex shrink-0 items-center gap-0.5 border-b border-border/40 px-2 py-1.5">
        <button type="button" onClick={onFetch} disabled={disabled} className={tbBtnBase} title={t('gitGraph.fetchTitle')}>
          <RefreshCw className={fetchBtnCls} />
          <span>{t('gitGraph.fetchLabel')}</span>
        </button>
        <button type="button" onClick={onPull} disabled={disabled} className={tbBtnBase} title={t('gitGraph.pullTitle')}>
          <ArrowDown className={pullBtnCls} />
          <span>{t('gitGraph.pullLabel')}</span>
        </button>
        <button type="button" onClick={onPush} disabled={disabled} className={tbBtnBase} title={t('gitGraph.pushTitle')}>
          <ArrowUp className={pushBtnCls} />
          <span>{t('gitGraph.pushLabel')}</span>
        </button>
        <button type="button" onClick={onStash} disabled={disabled} className={tbBtnBase} title={t('gitGraph.stashLabel')}>
          <Archive className="size-4" />
          {t('gitGraph.stashLabel')}
        </button>
        <button type="button" onClick={onPop} disabled={disabled} className={tbBtnBase} title={t('gitGraph.popLabel')}>
          <ArchiveRestore className="size-4" />
          {t('gitGraph.popLabel')}
        </button>
        <button type="button" onClick={onCreateBranch} disabled={disabled} className={tbBtnBase} title={t('gitGraph.branchLabel')}>
          <GitBranch className="size-4" />
          {t('gitGraph.branchLabel')}
        </button>
        <div className="mx-1 h-5 w-px bg-border/40" />
        <button type="button" onClick={openSearch} className={tbBtnBase} title={t('gitGraph.searchPlaceholder')}>
          <Search className="size-4" />
        </button>
      </div>
      <div className="flex shrink-0 items-center border-b border-border/40 bg-background px-4 py-1.5 text-[10px] font-semibold uppercase tracking-widest text-muted-foreground">
        <div style={{ width: REFS_COL_W }} className="shrink-0">{t('gitGraph.colBranchTag')}</div>
        <div style={{ width: graphColW }} className="shrink-0 text-center">{t('gitGraph.colGraph')}</div>
        <div className="min-w-0 flex-1 pl-3">{t('gitGraph.colCommit')}</div>
        <div style={{ width: AUTHOR_COL_W }} className="shrink-0 pl-2">{t('gitGraph.colAuthor')}</div>
        <div style={{ width: DATE_COL_W }} className="shrink-0 pl-2 text-right">{t('gitGraph.colDate')}</div>
      </div>
      <div
        style={{ height: ROW_H }}
        className={`flex shrink-0 cursor-pointer items-center border-b border-border/40 px-4 ${wipRowBg}`}
        role="button"
        tabIndex={0}
        onClick={onOpenStaging}
        onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') onOpenStaging(); }}
      >
        <div style={{ width: REFS_COL_W }} className="shrink-0" />
        <svg style={{ width: graphColW }} height={ROW_H} className="shrink-0 text-muted-foreground/50">
          <circle cx={LANE_W / 2} cy={ROW_H / 2} r={NODE_R} fill="none" stroke="currentColor" strokeWidth={1.5} strokeDasharray="3 2" />
        </svg>
        <div className="flex min-w-0 flex-1 items-center gap-2 border-l border-border/40 pl-2">
          <input
            type="text"
            value={wipMsg}
            onChange={(e) => { onSetWipMsg(e.target.value); onOpenStaging(); }}
            onFocus={onOpenStaging}
            onClick={(e) => e.stopPropagation()}
            placeholder="// WIP"
            style={{ width: `max(100px, ${wipMsg.length + 2}ch)` }}
            className="shrink-0 rounded border border-border/60 bg-muted/30 px-1.5 py-0.5 text-xs text-foreground/80 placeholder:text-muted-foreground/50 focus:border-border focus:outline-none"
          />
          {wipChangesCount > 0 && (
            <span className="flex shrink-0 items-center gap-0.5 text-xs">
              <Pencil className="size-2.5 text-amber-400" />
              <span className="text-foreground">{wipChangesCount}</span>
            </span>
          )}
        </div>
      </div>
      <VirtualCommitScroll displayRows={displayRows} adjCommits={adjCommits} adjEdges={adjEdges} maxLane={maxLane} selectedHash={selectedHash} scrollToHash={scrollToHash} onSelectHash={onSelectHash} onContextMenu={onContextMenu} onDoubleClick={onDoubleClick} />
    </div>
  );
};
