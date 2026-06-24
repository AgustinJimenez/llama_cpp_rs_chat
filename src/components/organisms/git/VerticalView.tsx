import React from 'react';

import type { AssignedCommit, GraphEdge } from '../../../utils/gitGraph';

import { BranchPanel } from './BranchPanel';
import { CommitDetailPanel } from './CommitDetailPanel';
import { CommitListPane } from './CommitListPane';
import { DiffViewer } from './DiffViewer';
import { StagingPanel } from './StagingPanel';
import type { FileChange } from './types';

export { RESIZE_PANEL_MIN_W as RESIZE_MIN_W, RESIZE_PANEL_MAX_W as RESIZE_MAX_W } from './constants';

interface VerticalViewProps {
  commits: AssignedCommit[];
  adjCommits: AssignedCommit[];
  adjEdges: GraphEdge[];
  edges: GraphEdge[];
  maxLane: number;
  selectedHash: string | null;
  selectedCommit: AssignedCommit | null;
  commitFiles: FileChange[];
  filesLoading: boolean;
  filteredCommits: AssignedCommit[];
  searchQuery: string;
  stagingActive: boolean;
  stagingDiffFile: string | null;
  stagingDiffKind: 'working' | 'staged';
  selectedDiffFile: string | null;
  wipMsg: string;
  wipChangesCount: number;
  toolbarBusy: 'fetch' | 'pull' | 'push' | null;
  detailPanelWidth: number;
  path: string;
  displayRows: Array<{ commit: AssignedCommit; refIndex: number }>;
  onSelectHash: (hash: string) => void;
  onContextMenu: (clientX: number, clientY: number, hash: string) => void;
  onDoubleClick: (hash: string) => void;
  onSelectParent: (hash: string) => void;
  onSelectFile: (file: string) => void;
  onSelectStagingFile: (file: string, kind: 'working' | 'staged') => void;
  onCommitMsgChange: (v: string) => void;
  onCommitDone: () => void;
  onCloseStagingPanel: () => void;
  onCloseDiff: () => void;
  onCloseStagingDiff: () => void;
  onOpenStaging: () => void;
  onSetWipMsg: (msg: string) => void;
  onSetSearchQuery: (q: string) => void;
  onFetch: () => void;
  onPull: () => void;
  onPush: () => void;
  onStash: () => void;
  onPop: () => void;
  onCreateBranch: () => void;
  onResizeStart: (clientX: number) => void;
}

export const VerticalView: React.FC<VerticalViewProps> = (props) => {
  const {
    commits, adjCommits, adjEdges, maxLane,
    selectedHash, selectedCommit, commitFiles, filesLoading,
    filteredCommits, searchQuery, stagingActive, stagingDiffFile,
    stagingDiffKind, selectedDiffFile, wipMsg, wipChangesCount,
    toolbarBusy, detailPanelWidth, path, displayRows,
    onSelectHash, onContextMenu, onDoubleClick, onSelectParent, onSelectFile,
    onSelectStagingFile, onCommitMsgChange, onCommitDone, onCloseStagingPanel,
    onCloseDiff, onCloseStagingDiff, onOpenStaging, onSetWipMsg, onSetSearchQuery,
    onFetch, onPull, onPush, onStash, onPop, onCreateBranch, onResizeStart,
  } = props;

  const detailPanelEl = selectedCommit && !stagingActive ? (
    <CommitDetailPanel
      commit={selectedCommit}
      files={commitFiles}
      filesLoading={filesLoading}
      width={detailPanelWidth}
      onSelectParent={onSelectParent}
      onSelectFile={onSelectFile}
    />
  ) : null;

  const stagingPanelEl = stagingActive ? (
    <StagingPanel
      repoPath={path}
      width={detailPanelWidth}
      commitMsg={wipMsg}
      onCommitMsgChange={onCommitMsgChange}
      onCommitDone={onCommitDone}
      onClose={onCloseStagingPanel}
      onSelectFile={onSelectStagingFile}
      selectedFile={stagingDiffFile}
    />
  ) : null;

  const rightPanelEl = stagingActive ? stagingPanelEl : detailPanelEl;
  const showResizeHandle = stagingActive || !!detailPanelEl;
  const stagingDiffHash = stagingDiffKind === 'staged' ? 'STAGED' : 'WORKING';

  let centerContent: React.ReactNode;
  if (stagingActive && stagingDiffFile) {
    centerContent = (
      <DiffViewer repoPath={path} hash={stagingDiffHash} file={stagingDiffFile} onClose={onCloseStagingDiff} />
    );
  } else if (selectedDiffFile && selectedHash) {
    centerContent = (
      <DiffViewer repoPath={path} hash={selectedHash} file={selectedDiffFile} onClose={onCloseDiff} />
    );
  } else {
    centerContent = (
      <CommitListPane
        adjCommits={adjCommits}
        adjEdges={adjEdges}
        maxLane={maxLane}
        selectedHash={selectedHash}
        filteredCommits={filteredCommits}
        searchQuery={searchQuery}
        stagingActive={stagingActive}
        wipMsg={wipMsg}
        wipChangesCount={wipChangesCount}
        toolbarBusy={toolbarBusy}
        path={path}
        displayRows={displayRows}
        onSelectHash={onSelectHash}
        onContextMenu={onContextMenu}
        onDoubleClick={onDoubleClick}
        onOpenStaging={onOpenStaging}
        onSetWipMsg={onSetWipMsg}
        onSetSearchQuery={onSetSearchQuery}
        onFetch={onFetch}
        onPull={onPull}
        onPush={onPush}
        onStash={onStash}
        onPop={onPop}
        onCreateBranch={onCreateBranch}
      />
    );
  }

  return (
    <div className="flex min-h-0 flex-1 overflow-hidden">
      <BranchPanel commits={commits} selectedHash={selectedHash} onSelect={onSelectHash} />
      {centerContent}
      {!!showResizeHandle && (
        <button
          type="button"
          aria-label="Resize panel"
          onMouseDown={(e) => { onResizeStart(e.clientX); }}
          className="w-1 shrink-0 cursor-col-resize border-l border-border/40 transition-colors hover:border-primary/60 hover:bg-primary/10"
        />
      )}
      {rightPanelEl}
    </div>
  );
};
