import { Cpu, Loader2, Trash2, Zap } from 'lucide-react';
import { useEffect, useState } from 'react';
import { toast } from 'react-hot-toast';

import { useChatContext } from '../../contexts/ChatContext';
import { useModelContext } from '../../contexts/ModelContext';
import { isTauriEnv } from '../../utils/tauri';
import { deleteWorker, listWorkers, type WorkerSummary } from '../../utils/tauriCommands';
import { Button } from '../atoms/button';

function workerDisplayName(worker: WorkerSummary): string {
  if (worker.general_name?.trim()) return worker.general_name;
  if (worker.model_path?.trim()) {
    const parts = worker.model_path.split(/[/\\]/);
    return parts[parts.length - 1].replace(/\.gguf$/i, '');
  }
  return worker.id === 'default' ? 'Default worker' : worker.id;
}

const POLL_INTERVAL_MS = 3000;

interface WorkerStatusPanelProps {
  enabled: boolean;
}

export const WorkerStatusPanel = ({ enabled }: WorkerStatusPanelProps) => {
  const { unloadModel } = useModelContext();
  const { currentConversationWorkerId, setCurrentConversationWorkerId } = useChatContext();
  const [workers, setWorkers] = useState<WorkerSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null);

  const refreshWorkers = async () => {
    if (isTauriEnv()) return;
    setLoading(true);
    try {
      const data = await listWorkers();
      setWorkers([...data.workers].sort((a, b) => a.id.localeCompare(b.id)));
    } catch (error) {
      toast.error(error instanceof Error ? error.message : 'Failed to load workers');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (!enabled || isTauriEnv()) return;
    void refreshWorkers();
    const interval = setInterval(() => {
      void refreshWorkers();
    }, POLL_INTERVAL_MS);
    return () => clearInterval(interval);
  }, [enabled]); // eslint-disable-line react-hooks/exhaustive-deps

  const handleRemove = async (worker: WorkerSummary) => {
    setPendingDeleteId(worker.id);
    try {
      if (worker.id === 'default') {
        await unloadModel();
      } else {
        await deleteWorker(worker.id);
        if (currentConversationWorkerId === worker.id) {
          setCurrentConversationWorkerId(null);
        }
      }
      await refreshWorkers();
    } catch (error) {
      toast.error(error instanceof Error ? error.message : 'Failed to remove worker');
    } finally {
      setPendingDeleteId(null);
    }
  };

  if (isTauriEnv()) {
    return (
      <div className="rounded-lg border border-border p-3 text-sm text-muted-foreground">
        Tauri mode currently exposes only the default local worker. Multi-worker management is
        available in web mode.
      </div>
    );
  }

  const refreshLabel = loading ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : 'Refresh';
  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold text-foreground">Workers</h3>
        <Button variant="ghost" size="sm" onClick={() => void refreshWorkers()} disabled={loading}>
          {refreshLabel}
        </Button>
      </div>

      {workers.length === 0 && (
        <div className="rounded-lg border border-border p-3 text-sm text-muted-foreground">
          No active local workers.
        </div>
      )}
      {workers.length > 0 && (
        <div className="space-y-2">
          {workers.map((worker) => {
            const isCurrent =
              (currentConversationWorkerId ?? 'default') ===
              (worker.id === 'default' ? 'default' : worker.id);
            let workerStatusLabel = 'Idle';
            if (worker.loading) workerStatusLabel = 'Loading model…';
            else if (worker.generating) workerStatusLabel = 'Generating';
            else if (worker.loaded) workerStatusLabel = 'Ready';
            const workerIcon =
              worker.id === 'default' ? (
                <Cpu className="h-4 w-4 text-emerald-400" />
              ) : (
                <Zap
                  className={`h-4 w-4 ${worker.generating ? 'text-yellow-400' : 'text-cyan-400'}`}
                />
              );
            const deleteIcon =
              pendingDeleteId === worker.id ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Trash2 className="h-4 w-4 text-destructive" />
              );
            const conversationSuffix = worker.active_conversation_id
              ? ` · conversation ${worker.active_conversation_id}`
              : '';
            const contextSuffix = worker.context_size
              ? ` · ${worker.context_size.toLocaleString()} ctx`
              : '';
            const removeTitle =
              worker.id === 'default' ? 'Unload default worker model' : 'Remove worker';
            return (
              <div key={worker.id} className="rounded-lg border border-border bg-muted/40 p-3">
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      {workerIcon}
                      <span className="truncate font-medium text-foreground">
                        {workerDisplayName(worker)}
                      </span>
                      <span className="rounded bg-background px-1.5 py-0.5 text-[10px] text-muted-foreground">
                        {worker.id}
                      </span>
                      {!!isCurrent && (
                        <span className="rounded bg-primary/15 px-1.5 py-0.5 text-[10px] text-primary">
                          current
                        </span>
                      )}
                    </div>
                    <div className="mt-1 text-xs text-muted-foreground">
                      {workerStatusLabel}
                      {conversationSuffix}
                      {contextSuffix}
                    </div>
                  </div>

                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => void handleRemove(worker)}
                    disabled={pendingDeleteId === worker.id || worker.loading}
                    title={removeTitle}
                  >
                    {deleteIcon}
                  </Button>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};
