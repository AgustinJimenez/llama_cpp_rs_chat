import { Minus, Plus, RefreshCw, X } from 'lucide-react';
import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import {
  fetchGitStatus,
  gitAmend,
  gitCommit,
  gitStage,
  gitUnstage,
} from '../../../utils/gitGraph';
import type { GitStatusResult } from '../../../utils/gitGraph';

import { commitMsgLenCls } from './utils';

interface StagingFileRowProps {
  path: string;
  status: string;
  kind: 'working' | 'staged';
  isSelected: boolean;
  onSelect: (file: string, kind: 'working' | 'staged') => void;
  onAction: (file: string) => void;
  actionTitle: string;
  ActionIcon: React.ComponentType<{ className?: string }>;
}

const StagingFileRow: React.FC<StagingFileRowProps> = ({
  path, status, kind, isSelected, onSelect, onAction, actionTitle, ActionIcon,
}) => {
  let scls = 'text-amber-400';
  if (status === 'A' || status === '?') scls = 'text-emerald-400';
  else if (status === 'D') scls = 'text-red-400';
  const rowCls = `flex cursor-pointer items-center gap-1.5 px-3 py-0.5 hover:bg-muted/40 ${isSelected ? 'bg-muted/60' : ''}`;
  return (
    <div
      className={rowCls}
      role="button"
      tabIndex={0}
      onClick={() => onSelect(path, kind)}
      onKeyDown={(e) => { if (e.key === 'Enter') onSelect(path, kind); }}
    >
      <span className={`w-3 shrink-0 text-[10px] font-bold ${scls}`}>{status}</span>
      <span className="min-w-0 flex-1 truncate text-xs text-foreground/70">{path}</span>
      <button
        type="button"
        onClick={(e) => { e.stopPropagation(); onAction(path); }}
        className="shrink-0 rounded p-0.5 hover:bg-muted"
        title={actionTitle}
      >
        <ActionIcon className="size-3" />
      </button>
    </div>
  );
};

export const StagingPanel: React.FC<{
  repoPath: string;
  width: number;
  commitMsg: string;
  onCommitMsgChange: (v: string) => void;
  onCommitDone: () => void;
  onClose: () => void;
  onSelectFile: (file: string, kind: 'working' | 'staged') => void;
  selectedFile: string | null;
}> = ({ repoPath, width, commitMsg, onCommitMsgChange, onCommitDone, onClose, onSelectFile, selectedFile }) => {
  const { t } = useTranslation();
  const [status, setStatus] = useState<GitStatusResult | null>(null);
  const [statusLoading, setStatusLoading] = useState(false);
  const [commitDesc, setCommitDesc] = useState('');
  const [isCommitting, setIsCommitting] = useState(false);
  const [opError, setOpError] = useState<string | null>(null);
  const [amendMode, setAmendMode] = useState(false);

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
    } finally {
      setStatusLoading(false);
    }
  }, [repoPath]);

  useEffect(() => { void refreshStatus(); }, [refreshStatus]);

  const handleStageFile = useCallback(async (file: string) => {
    const res = await gitStage(repoPath, [file]);
    if (!res.ok) setOpError(res.error);
    else void refreshStatus();
  }, [repoPath, refreshStatus]);

  const handleUnstageFile = useCallback(async (file: string) => {
    const res = await gitUnstage(repoPath, [file]);
    if (!res.ok) setOpError(res.error);
    else void refreshStatus();
  }, [repoPath, refreshStatus]);

  const handleStageAll = useCallback(async () => {
    setOpError(null);
    const res = await gitStage(repoPath, []);
    if (!res.ok) setOpError(res.error);
    else void refreshStatus();
  }, [repoPath, refreshStatus]);

  const handleUnstageAll = useCallback(async () => {
    setOpError(null);
    const res = await gitUnstage(repoPath, []);
    if (!res.ok) setOpError(res.error);
    else void refreshStatus();
  }, [repoPath, refreshStatus]);

  const handleCommit = useCallback(async () => {
    if (!commitMsg.trim() && !amendMode) return;
    setIsCommitting(true);
    setOpError(null);
    try {
      const res = amendMode
        ? await gitAmend(repoPath, commitMsg.trim(), commitDesc.trim())
        : await gitCommit(repoPath, commitMsg.trim(), commitDesc.trim());
      if (res.ok) {
        onCommitMsgChange('');
        setCommitDesc('');
        onCommitDone();
      } else setOpError(res.error ?? 'Commit failed');
    } catch (error) {
      setOpError(error instanceof Error ? error.message : 'Commit error');
    } finally {
      setIsCommitting(false);
    }
  }, [repoPath, commitMsg, commitDesc, amendMode, onCommitDone, onCommitMsgChange]);

  const stagedCount = status?.staged.length ?? 0;
  const unstagedCount = status?.unstaged.length ?? 0;
  const canCommit = (amendMode || stagedCount > 0) && (amendMode || commitMsg.trim().length > 0) && !isCommitting;
  const refreshIconCls = `size-3 ${statusLoading ? 'animate-spin' : ''}`;
  const commitBtnLabel = isCommitting ? t('gitGraph.committing') : `${t('gitGraph.commitButton')} (${stagedCount})`;
  const statusLoadingEl = statusLoading && !status ? (
    <p className="px-3 py-1 text-xs text-muted-foreground">{t('gitGraph.loadingFiles')}</p>
  ) : null;

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
      <div className="flex shrink-0 items-center gap-2 border-b border-border/40 px-3 py-1">
        <span className="flex-1 text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/50">{t('gitGraph.unstaged')} ({unstagedCount})</span>
        {unstagedCount > 0 && (
          <button type="button" onClick={() => void handleStageAll()} className="rounded px-1.5 py-0.5 text-[10px] text-emerald-400 hover:bg-muted">{t('gitGraph.stageAll')}</button>
        )}
      </div>
      <div className="shrink-0 overflow-y-auto" style={{ maxHeight: 120 }}>
        {statusLoadingEl}
        {(status?.unstaged ?? []).map((f) => (
          <StagingFileRow
            key={f.path}
            path={f.path}
            status={f.status}
            kind="working"
            isSelected={selectedFile === f.path}
            onSelect={onSelectFile}
            onAction={(file) => void handleStageFile(file)}
            actionTitle={t('gitGraph.stageFile')}
            ActionIcon={Plus}
          />
        ))}
      </div>
      <div className="flex shrink-0 items-center gap-2 border-b border-t border-border/40 px-3 py-1">
        <span className="flex-1 text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/50">{t('gitGraph.staged')} ({stagedCount})</span>
        {stagedCount > 0 && (
          <button type="button" onClick={() => void handleUnstageAll()} className="rounded px-1.5 py-0.5 text-[10px] text-muted-foreground hover:bg-muted">{t('gitGraph.unstageAll')}</button>
        )}
      </div>
      <div className="shrink-0 overflow-y-auto" style={{ maxHeight: 120 }}>
        {(status?.staged ?? []).map((f) => (
          <StagingFileRow
            key={f.path}
            path={f.path}
            status={f.status}
            kind="staged"
            isSelected={selectedFile === f.path}
            onSelect={onSelectFile}
            onAction={(file) => void handleUnstageFile(file)}
            actionTitle={t('gitGraph.unstageFile')}
            ActionIcon={Minus}
          />
        ))}
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-2 border-t border-border/40 p-3">
        <label className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <input type="checkbox" checked={amendMode} onChange={(e) => setAmendMode(e.target.checked)} className="size-3" />
          {t('gitGraph.amendLabel')}
        </label>
        <div className="flex items-center gap-1">
          <input
            type="text"
            value={commitMsg}
            onChange={(e) => onCommitMsgChange(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter' && canCommit) void handleCommit(); }}
            placeholder={t('gitGraph.commitMsgPlaceholder')}
            className="min-w-0 flex-1 rounded border border-border/40 bg-muted/30 px-2 py-1.5 text-xs text-foreground placeholder:text-muted-foreground/50 focus:outline-none focus:ring-1 focus:ring-ring"
          />
          <span className={`shrink-0 text-[10px] tabular-nums ${commitMsgLenCls(commitMsg.length)}`}>{commitMsg.length}</span>
        </div>
        <textarea
          value={commitDesc}
          onChange={(e) => setCommitDesc(e.target.value)}
          placeholder={t('gitGraph.commitDescPlaceholder')}
          rows={2}
          className="w-full resize-none rounded border border-border/40 bg-muted/30 px-2 py-1.5 text-xs text-foreground placeholder:text-muted-foreground/50 focus:outline-none focus:ring-1 focus:ring-ring"
        />
        <button type="button" onClick={() => void handleCommit()} disabled={!canCommit} className="w-full rounded bg-emerald-600 py-1.5 text-xs font-semibold text-white hover:bg-emerald-700 disabled:opacity-40">
          {commitBtnLabel}
        </button>
      </div>
    </div>
  );
};
