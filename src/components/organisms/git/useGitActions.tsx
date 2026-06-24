import { useCallback, useState } from 'react';
import toast from 'react-hot-toast';
import { useTranslation } from 'react-i18next';

import {
  fetchGitStatus,
  gitCheckout,
  gitCherryPick,
  gitCreateBranch,
  gitFetch,
  gitPull,
  gitPush,
  gitReset,
  gitRevert,
  gitStashPop,
  gitStashPush,
} from '../../../utils/gitGraph';
import type { AssignedCommit } from '../../../utils/gitGraph';

type ToolbarBusy = 'fetch' | 'pull' | 'push' | null;

interface UseGitActionsOptions {
  path: string;
  loadGraph: (p: string) => Promise<void>;
}

interface UseGitActionsResult {
  toolbarBusy: ToolbarBusy;
  handleStash: () => Promise<void>;
  handlePop: () => Promise<void>;
  handleCreateBranch: () => Promise<void>;
  handleFetch: () => Promise<void>;
  handlePull: () => Promise<void>;
  handlePush: () => Promise<void>;
  handleDoubleClick: (hash: string) => Promise<void>;
  handleCtxAction: (key: string, commit: AssignedCommit) => Promise<void>;
}

async function doStashAndCheckout(
  path: string,
  hash: string,
  toastId: string,
  loadGraph: (p: string) => Promise<void>,
  t: (key: string) => string,
): Promise<void> {
  toast.dismiss(toastId);
  const stash = await gitStashPush(path);
  if (!stash.ok) { toast.error(stash.error ?? t('gitGraph.stashError')); return; }
  const co = await gitCheckout(path, hash);
  if (co.ok) { void loadGraph(path); }
  else toast.error(co.error ?? t('gitGraph.checkoutError'));
}

async function handleResetAction(
  path: string,
  key: string,
  hash: string,
  loadGraph: (p: string) => Promise<void>,
  t: (key: string) => string,
): Promise<void> {
  let mode: 'soft' | 'mixed' | 'hard' = 'hard';
  if (key === 'resetSoft') mode = 'soft';
  else if (key === 'resetMixed') mode = 'mixed';
  if (mode === 'hard') {
    const ok = window.confirm(t('gitGraph.resetHardConfirm'));
    if (!ok) return;
  }
  const res = await gitReset(path, mode, hash);
  if (res.ok) { void loadGraph(path); }
  else toast.error(res.error ?? t('gitGraph.resetError'));
}

export function useGitActions({ path, loadGraph }: UseGitActionsOptions): UseGitActionsResult {
  const { t } = useTranslation();
  const [toolbarBusy, setToolbarBusy] = useState<ToolbarBusy>(null);

  const handleDoubleClick = useCallback(
    async (hash: string) => {
      const status = await fetchGitStatus(path);
      const isDirty = status.staged.length > 0 || status.unstaged.length > 0;
      if (!isDirty) {
        const res = await gitCheckout(path, hash);
        if (res.ok) { void loadGraph(path); }
        else toast.error(res.error ?? t('gitGraph.checkoutError'));
        return;
      }
      toast.custom(
        (toastInstance) => (
          <div className="flex items-center gap-3 rounded-lg border border-border bg-card px-4 py-3 text-sm shadow-lg">
            <span className="text-foreground">{t('gitGraph.checkoutDirtyPrompt')}</span>
            <button type="button" className="rounded bg-primary px-2 py-1 text-xs font-medium text-primary-foreground hover:bg-primary/90" onClick={() => void doStashAndCheckout(path, hash, toastInstance.id, loadGraph, t)}>
              {t('gitGraph.stashAndCheckout')}
            </button>
            <button type="button" className="rounded px-2 py-1 text-xs text-muted-foreground hover:text-foreground" onClick={() => toast.dismiss(toastInstance.id)}>
              {t('common.cancel')}
            </button>
          </div>
        ),
        { duration: 8000 },
      );
    },
    [path, loadGraph, t],
  );

  const handleCtxAction = useCallback(
    async (key: string, commit: AssignedCommit) => {
      if (key === 'checkout') { await handleDoubleClick(commit.hash); return; }
      if (key === 'createBranch') {
        const name = window.prompt(t('gitGraph.branchNamePrompt'), '');
        if (!name?.trim()) return;
        const res = await gitCreateBranch(path, name.trim(), commit.hash);
        if (res.ok) { void loadGraph(path); } else toast.error(res.error ?? t('gitGraph.branchError'));
        return;
      }
      if (key === 'cherryPick') {
        const res = await gitCherryPick(path, commit.hash);
        if (res.ok) { void loadGraph(path); } else toast.error(res.error ?? t('gitGraph.cherryPickError'));
        return;
      }
      if (key === 'revert') {
        const res = await gitRevert(path, commit.hash);
        if (res.ok) { void loadGraph(path); } else toast.error(res.error ?? t('gitGraph.revertError'));
        return;
      }
      if (key === 'resetSoft' || key === 'resetMixed' || key === 'resetHard') {
        await handleResetAction(path, key, commit.hash, loadGraph, t);
      }
    },
    [path, loadGraph, t, handleDoubleClick],
  );

  const handleStash = useCallback(async () => {
    const res = await gitStashPush(path);
    if (res.ok) { void loadGraph(path); } else toast.error(res.error ?? t('gitGraph.stashError'));
  }, [path, loadGraph, t]);

  const handlePop = useCallback(async () => {
    const res = await gitStashPop(path);
    if (res.ok) { void loadGraph(path); } else toast.error(res.error ?? t('gitGraph.popError'));
  }, [path, loadGraph, t]);

  const handleCreateBranch = useCallback(async () => {
    const name = window.prompt(t('gitGraph.branchNamePrompt'), '');
    if (!name?.trim()) return;
    const res = await gitCreateBranch(path, name.trim());
    if (res.ok) { void loadGraph(path); } else toast.error(res.error ?? t('gitGraph.branchError'));
  }, [path, loadGraph, t]);

  const handleFetch = useCallback(async () => {
    if (!path.trim()) return;
    setToolbarBusy('fetch');
    try {
      const res = await gitFetch(path.trim());
      if (res.ok) { void loadGraph(path); } else toast.error(res.error ?? t('gitGraph.fetchError'));
    } catch (error) {
      toast.error(error instanceof Error ? error.message : 'Fetch error');
    } finally {
      setToolbarBusy(null);
    }
  }, [path, loadGraph, t]);

  const handlePull = useCallback(async () => {
    if (!path.trim()) return;
    setToolbarBusy('pull');
    try {
      const res = await gitPull(path.trim());
      if (res.ok) { void loadGraph(path); } else toast.error(res.error ?? t('gitGraph.pullError'));
    } catch (error) {
      toast.error(error instanceof Error ? error.message : 'Pull error');
    } finally {
      setToolbarBusy(null);
    }
  }, [path, loadGraph, t]);

  const handlePush = useCallback(async () => {
    if (!path.trim()) return;
    setToolbarBusy('push');
    try {
      const res = await gitPush(path.trim());
      if (res.ok) toast.success(res.output || t('gitGraph.pushDone'));
      else toast.error(res.error ?? t('gitGraph.pushError'));
    } catch (error) {
      toast.error(error instanceof Error ? error.message : 'Push error');
    } finally {
      setToolbarBusy(null);
    }
  }, [path, t]);

  return {
    toolbarBusy,
    handleStash,
    handlePop,
    handleCreateBranch,
    handleFetch,
    handlePull,
    handlePush,
    handleDoubleClick,
    handleCtxAction,
  };
}
