import { Cpu, FolderOpen, Loader2, Plus, Zap } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { toast } from 'react-hot-toast';

import {
  bindConversationWorker,
  createWorker,
  getWorkerStatus,
  listWorkers,
  pickFile,
  type WorkerSummary,
} from '../../utils/tauriCommands';
import { Button } from '../atoms/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../atoms/dialog';

interface WorkerSelectorModalProps {
  isOpen: boolean;
  onClose: () => void;
  conversationId: string | null;
  currentWorkerId: string | null;
  onWorkerChanged: (workerId: string | null) => void;
}

function workerLabel(worker: WorkerSummary): string {
  if (worker.general_name?.trim()) return worker.general_name;
  if (worker.model_path?.trim()) {
    const parts = worker.model_path.split(/[/\\]/);
    return parts[parts.length - 1].replace(/\.gguf$/i, '');
  }
  return worker.id === 'default' ? 'Default worker' : worker.id;
}

// eslint-disable-next-line max-lines-per-function
export const WorkerSelectorModal = ({
  isOpen,
  onClose,
  conversationId,
  currentWorkerId,
  onWorkerChanged,
}: WorkerSelectorModalProps) => {
  const [workers, setWorkers] = useState<WorkerSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [creating, setCreating] = useState(false);
  const [modelPath, setModelPath] = useState('');
  const [selectedWorkerId, setSelectedWorkerId] = useState<string | null>(currentWorkerId);

  const refreshWorkers = async () => {
    setLoading(true);
    try {
      const data = await listWorkers();
      const next = [...data.workers].sort((a, b) => a.id.localeCompare(b.id));
      setWorkers(next);
      if (selectedWorkerId && selectedWorkerId !== 'default') {
        const stillExists = next.some((worker) => worker.id === selectedWorkerId);
        if (!stillExists) {
          setSelectedWorkerId(null);
        }
      }
    } catch (error) {
      toast.error(error instanceof Error ? error.message : 'Failed to load workers');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (!isOpen) return;
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setSelectedWorkerId(currentWorkerId);
    void refreshWorkers();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen, currentWorkerId]);

  const selectedId = useMemo(
    () => (selectedWorkerId && selectedWorkerId !== 'default' ? selectedWorkerId : null),
    [selectedWorkerId],
  );

  const handleBind = async (workerId: string | null) => {
    try {
      if (conversationId) {
        await bindConversationWorker(conversationId, workerId);
      }
      onWorkerChanged(workerId);
      onClose();
    } catch (error) {
      toast.error(error instanceof Error ? error.message : 'Failed to update worker binding');
    }
  };

  const handlePickFile = async () => {
    try {
      const path = await pickFile();
      if (path) setModelPath(path);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : 'Failed to open file picker');
    }
  };

  const handleCreate = async () => {
    const trimmedPath = modelPath.trim();
    if (!trimmedPath) return;

    setCreating(true);
    try {
      const { worker_id } = await createWorker({ model_path: trimmedPath });
      const worker = await getWorkerStatus(worker_id).catch(() => null);
      setWorkers((prev) => {
        const rest = prev.filter((entry) => entry.id !== worker_id);
        return [
          ...rest,
          worker ?? { id: worker_id, loaded: true, loading: false, generating: false },
        ];
      });
      await handleBind(worker_id);
      setModelPath('');
    } catch (error) {
      toast.error(error instanceof Error ? error.message : 'Failed to create worker');
    } finally {
      setCreating(false);
    }
  };

  return (
    <Dialog open={isOpen} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>Conversation Worker</DialogTitle>
          <DialogDescription>
            Bind this conversation to an existing local worker or spawn a new one with a different
            model. Unbound conversations use the default worker.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
                Active Workers
              </span>
              {(() => {
                const refreshLabel = loading ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  'Refresh'
                );
                return (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => void refreshWorkers()}
                    disabled={loading}
                  >
                    {refreshLabel}
                  </Button>
                );
              })()}
            </div>

            <div className="space-y-2">
              <button
                onClick={() => setSelectedWorkerId(null)}
                className={`w-full rounded-lg border p-3 text-left transition-colors ${
                  selectedId === null
                    ? 'border-primary bg-primary/10'
                    : 'border-border hover:bg-muted/50'
                }`}
              >
                <div className="flex items-center gap-3">
                  <Cpu className="h-4 w-4 text-emerald-400" />
                  <div className="min-w-0 flex-1">
                    <div className="font-medium text-foreground">Default worker</div>
                    <div className="text-xs text-muted-foreground">
                      Uses the legacy local model slot and `/api/model/*` compatibility flow.
                    </div>
                  </div>
                </div>
              </button>

              {workers
                .filter((worker) => worker.id !== 'default')
                .map((worker) => {
                  let zapColor = 'text-muted-foreground';
                  if (worker.generating) zapColor = 'text-yellow-400';
                  else if (worker.loaded) zapColor = 'text-cyan-400';
                  let workerStatusLabel = 'Idle';
                  if (worker.loading) workerStatusLabel = 'Loading model…';
                  else if (worker.generating) workerStatusLabel = 'Generating';
                  else if (worker.loaded) workerStatusLabel = 'Ready';
                  const ctxLabel = worker.context_size
                    ? ` · ${worker.context_size.toLocaleString()} ctx`
                    : '';
                  return (
                    <button
                      key={worker.id}
                      onClick={() => setSelectedWorkerId(worker.id)}
                      className={`w-full rounded-lg border p-3 text-left transition-colors ${
                        selectedId === worker.id
                          ? 'border-primary bg-primary/10'
                          : 'border-border hover:bg-muted/50'
                      }`}
                    >
                      <div className="flex items-start gap-3">
                        <Zap className={`mt-0.5 h-4 w-4 ${zapColor}`} />
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-2">
                            <span className="font-medium text-foreground">
                              {workerLabel(worker)}
                            </span>
                            <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
                              {worker.id}
                            </span>
                          </div>
                          <div className="mt-1 text-xs text-muted-foreground">
                            {`${workerStatusLabel}${ctxLabel}`}
                          </div>
                        </div>
                      </div>
                    </button>
                  );
                })}
            </div>
          </div>

          <div className="space-y-2 rounded-lg border border-border p-3">
            <div className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
              Spawn New Worker
            </div>
            <div className="flex gap-2">
              <input
                type="text"
                value={modelPath}
                onChange={(event) => setModelPath(event.target.value)}
                placeholder="E:/models/your-model.gguf"
                className="flex-1 rounded-md border border-border bg-muted px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary"
              />
              <Button
                variant="outline"
                onClick={() => void handlePickFile()}
                title="Pick model file"
              >
                <FolderOpen className="h-4 w-4" />
              </Button>
              {(() => {
                const createIcon = creating ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Plus className="h-4 w-4" />
                );
                return (
                  <Button
                    onClick={() => void handleCreate()}
                    disabled={creating || !modelPath.trim()}
                  >
                    {createIcon}
                  </Button>
                );
              })()}
            </div>
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={() => void handleBind(selectedId)}>Use Worker</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
