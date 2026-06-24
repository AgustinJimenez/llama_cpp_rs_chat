import {
  ArrowLeftRight,
  ArrowUpDown,
  FolderOpen,
  GitBranch,
  RefreshCw,
} from 'lucide-react';
import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { ContextMenu } from './git/ContextMenu';
import { HorizontalView } from './git/HorizontalView';
import { PathInput } from './git/PathInput';
import type { CtxMenuState, FileChange } from './git/types';
import { useGitActions } from './git/useGitActions';
import { useGitGraph } from './git/useGitGraph';
import { useResizePanel } from './git/useResizePanel';
import { pickDirectory } from './git/utils';
import { VerticalView } from './git/VerticalView';

export const GitGraphView: React.FC = () => {
  const { t } = useTranslation();
  const [searchQuery, setSearchQuery] = useState('');
  const [isHorizontal, setIsHorizontal] = useState(
    () => localStorage.getItem('gitGraphLayout') === 'horizontal',
  );
  const [ctxMenu, setCtxMenu] = useState<CtxMenuState | null>(null);
  const [commitFiles, setCommitFiles] = useState<FileChange[]>([]);
  const [filesLoading, setFilesLoading] = useState(false);
  const [selectedDiffFile, setSelectedDiffFile] = useState<string | null>(null);
  const [stagingDiffFile, setStagingDiffFile] = useState<string | null>(null);
  const [stagingDiffKind, setStagingDiffKind] = useState<'working' | 'staged'>('working');
  const [stagingActive, setStagingActive] = useState(false);
  const [wipMsg, setWipMsg] = useState('');
  const { detailPanelWidth, onResizeStart } = useResizePanel();

  const {
    path, setPath, recentPaths, loading, loadError, commits, edges, maxLane,
    wipChangesCount, selectedHash, setSelectedHash,
    filteredCommits, displayRows, adjCommits, adjEdges, loadGraph,
  } = useGitGraph(searchQuery);

  const {
    toolbarBusy, handleStash, handlePop, handleCreateBranch,
    handleFetch, handlePull, handlePush, handleDoubleClick, handleCtxAction,
  } = useGitActions({ path, loadGraph });

  const handleSelectHash = useCallback((hash: string) => {
    setSelectedHash(hash);
    setStagingActive(false);
  }, [setSelectedHash]);

  const handleContextMenu = useCallback(
    (clientX: number, clientY: number, hash: string) => {
      const commit = commits.find((c) => c.hash === hash);
      if (!commit) return;
      setCtxMenu({ x: clientX, y: clientY, commit });
    },
    [commits],
  );

  // Reset panel state when switching repos
  useEffect(() => { setStagingActive(false); setStagingDiffFile(null); setSearchQuery(''); }, [path]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    setSelectedDiffFile(null);
    if (!selectedHash || !path.trim()) { setCommitFiles([]); return; }
    setFilesLoading(true);
    fetch(`/api/git/commit-files?path=${encodeURIComponent(path.trim())}&hash=${selectedHash}`)
      .then((r) => r.json())
      .then((data: { files?: FileChange[] }) => { setCommitFiles(data.files ?? []); })
      .catch(() => { setCommitFiles([]); })
      .finally(() => { setFilesLoading(false); });
  }, [selectedHash, path]);

  const handleBrowse = useCallback(async () => {
    const picked = await pickDirectory();
    if (picked) { setPath(picked); await loadGraph(picked); }
  }, [loadGraph, setPath]);

  const handleLoad = useCallback(() => { void loadGraph(path); }, [path, loadGraph]);
  const toggleLayout = useCallback(() => {
    setIsHorizontal((p) => {
      const next = !p;
      localStorage.setItem('gitGraphLayout', next ? 'horizontal' : 'vertical');
      return next;
    });
  }, []);

  const selectedCommit = commits.find((c) => c.hash === selectedHash) ?? null;
  const refreshCls = loading ? 'size-3.5 animate-spin' : 'size-3.5';
  const layoutIcon = isHorizontal ? <ArrowUpDown className="size-3.5" /> : <ArrowLeftRight className="size-3.5" />;
  const layoutTitle = isHorizontal ? t('gitGraph.switchVertical') : t('gitGraph.switchHorizontal');

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
    bodyContent = <HorizontalView commits={commits} edges={edges} maxLane={maxLane} selectedHash={selectedHash} onSelectHash={handleSelectHash} onContextMenu={handleContextMenu} onDoubleClick={handleDoubleClick} />;
  } else {
    bodyContent = (
      <VerticalView
        commits={commits} adjCommits={adjCommits} adjEdges={adjEdges} edges={edges}
        maxLane={maxLane} selectedHash={selectedHash} selectedCommit={selectedCommit}
        commitFiles={commitFiles} filesLoading={filesLoading}
        filteredCommits={filteredCommits} searchQuery={searchQuery}
        stagingActive={stagingActive} stagingDiffFile={stagingDiffFile}
        stagingDiffKind={stagingDiffKind} selectedDiffFile={selectedDiffFile}
        wipMsg={wipMsg} wipChangesCount={wipChangesCount} toolbarBusy={toolbarBusy}
        detailPanelWidth={detailPanelWidth} path={path} displayRows={displayRows}
        onSelectHash={handleSelectHash} onContextMenu={handleContextMenu}
        onDoubleClick={handleDoubleClick} onSelectParent={handleSelectHash}
        onSelectFile={setSelectedDiffFile}
        onSelectStagingFile={(file, kind) => { setStagingDiffFile(file); setStagingDiffKind(kind); }}
        onCommitMsgChange={setWipMsg} onCommitDone={() => { setStagingActive(false); void loadGraph(path); }}
        onCloseStagingPanel={() => { setStagingActive(false); setStagingDiffFile(null); }}
        onCloseDiff={() => setSelectedDiffFile(null)} onCloseStagingDiff={() => setStagingDiffFile(null)}
        onOpenStaging={() => setStagingActive(true)} onSetWipMsg={setWipMsg}
        onSetSearchQuery={setSearchQuery}
        onFetch={() => void handleFetch()} onPull={() => void handlePull()}
        onPush={() => void handlePush()} onStash={() => void handleStash()}
        onPop={() => void handlePop()} onCreateBranch={() => void handleCreateBranch()}
        onResizeStart={onResizeStart}
      />
    );
  }

  return (
    <div className="flex h-full w-full flex-col overflow-hidden bg-background">
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-2">
        <GitBranch className="size-4 shrink-0 text-muted-foreground" />
        <span className="shrink-0 text-sm font-medium text-foreground">{t('gitGraph.title')}</span>
        <PathInput
          value={path}
          recentPaths={recentPaths}
          placeholder={t('gitGraph.placeholder')}
          onChange={setPath}
          onLoad={(p) => { void loadGraph(p); }}
        />
        <button type="button" onClick={handleBrowse} className="btn-icon shrink-0" title={t('gitGraph.browseFolderAria')} aria-label={t('gitGraph.browseFolderAria')}>
          <FolderOpen className="size-3.5" />
        </button>
        <button type="button" onClick={handleLoad} disabled={loading || !path.trim()} className="btn-icon shrink-0 disabled:opacity-40" title={t('gitGraph.reloadAria')} aria-label={t('gitGraph.reloadAria')}>
          <RefreshCw className={refreshCls} />
        </button>
        <button type="button" onClick={toggleLayout} disabled={commits.length === 0} className="btn-icon shrink-0 disabled:opacity-40" title={layoutTitle} aria-label={layoutTitle}>
          {layoutIcon}
        </button>
      </div>
      {bodyContent}
      {!!ctxMenu && (
        <ContextMenu
          state={ctxMenu}
          onClose={() => setCtxMenu(null)}
          onAction={(key, commit) => void handleCtxAction(key, commit)}
        />
      )}
    </div>
  );
};
