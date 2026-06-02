import { X, Cpu, Trash2, RefreshCw } from 'lucide-react';
import React, { useEffect, useState, useCallback } from 'react';

import { listWorkers, deleteWorker } from '../../utils/tauriCommands';
import type { WorkerSummary } from '../../utils/tauriCommands';

interface WorkersModalProps {
  isOpen: boolean;
  onClose: () => void;
  currentConversationWorkerId: string | null;
}

export const WorkersModal: React.FC<WorkersModalProps> = ({
  isOpen,
  onClose,
  currentConversationWorkerId,
}) => {
  const [workers, setWorkers] = useState<WorkerSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [deletingId, setDeletingId] = useState<string | null>(null);

  const fetchWorkers = useCallback(async () => {
    setLoading(true);
    try {
      const data = await listWorkers();
      setWorkers(data.workers);
    } catch {
      setWorkers([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (isOpen) void fetchWorkers();
  }, [isOpen, fetchWorkers]);

  const handleDelete = async (workerId: string) => {
    setDeletingId(workerId);
    try {
      await deleteWorker(workerId);
      await fetchWorkers();
    } finally {
      setDeletingId(null);
    }
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div
        className="absolute inset-0 bg-black/50"
        role="button"
        tabIndex={0}
        onClick={onClose}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') onClose();
        }}
      />
      <div className="relative mx-4 w-full max-w-lg overflow-hidden rounded-xl border border-border bg-card shadow-xl">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-border px-5 py-4">
          <div className="flex items-center gap-2">
            <Cpu className="h-4 w-4 text-muted-foreground" />
            <h2 className="text-sm font-semibold">Active Agents</h2>
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={fetchWorkers}
              className="rounded-md p-1.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
              title="Refresh"
            >
              <RefreshCw className={`h-3.5 w-3.5 ${loading ? 'animate-spin' : ''}`} />
            </button>
            <button
              onClick={onClose}
              className="rounded-md p-1.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            >
              <X className="h-4 w-4" />
            </button>
          </div>
        </div>

        {/* Body */}
        <div className="max-h-96 space-y-2 overflow-y-auto p-4">
          {!!loading && workers.length === 0 && (
            <p className="py-6 text-center text-sm text-muted-foreground">Loading...</p>
          )}
          {!loading && workers.length === 0 && (
            <p className="py-6 text-center text-sm text-muted-foreground">No active agents</p>
          )}
          {workers.length > 0 &&
            workers.map((w) => {
              const isCurrent = w.id === currentConversationWorkerId;
              const modelLabel =
                w.general_name?.trim() ||
                w.model_path
                  ?.split(/[/\\]/)
                  .pop()
                  ?.replace(/\.gguf$/i, '') ||
                'Unknown model';
              // eslint-disable-next-line no-nested-ternary
              const dotColor = w.generating
                ? 'bg-yellow-400'
                : w.loaded
                  ? 'bg-green-400'
                  : 'bg-muted-foreground';
              return (
                <div
                  key={w.id}
                  className={`flex items-center justify-between rounded-lg border px-3 py-2.5 ${
                    isCurrent ? 'border-primary/40 bg-primary/5' : 'border-border bg-muted/30'
                  }`}
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className={`h-1.5 w-1.5 flex-shrink-0 rounded-full ${dotColor}`} />
                      <span className="truncate text-sm font-medium">{modelLabel}</span>
                      {!!isCurrent && (
                        <span className="flex-shrink-0 text-[10px] font-medium text-primary">
                          current
                        </span>
                      )}
                    </div>
                    <div className="ml-3.5 mt-0.5 text-xs text-muted-foreground">
                      {w.id}
                      {!!w.active_conversation_id && (
                        <span className="ml-2 opacity-60">
                          → {w.active_conversation_id.slice(0, 24)}…
                        </span>
                      )}
                      {!w.active_conversation_id && (
                        <span className="ml-2 opacity-40">not bound</span>
                      )}
                    </div>
                  </div>
                  <button
                    onClick={() => handleDelete(w.id)}
                    disabled={deletingId === w.id}
                    className="ml-3 rounded-md p-1.5 text-muted-foreground transition-colors hover:bg-destructive/10 hover:text-destructive disabled:opacity-40"
                    title="Stop agent"
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </button>
                </div>
              );
            })}
        </div>
      </div>
    </div>
  );
};
