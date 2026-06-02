import { Terminal, X, Trash2 } from 'lucide-react';
import React, { useState, useEffect, useCallback } from 'react';

const SECONDS_PER_HOUR = 3600;
const MODAL_POLL_INTERVAL_MS = 3000;

import { getBackgroundProcesses, killBackgroundProcess } from '../../utils/tauriCommands';
import type { BackgroundProcessInfo } from '../../utils/tauriCommands';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogDescription,
  DialogTitle,
} from '../atoms/dialog';

interface BackgroundProcessesModalProps {
  isOpen: boolean;
  onClose: () => void;
}

function formatElapsed(startedAt: number): string {
  const now = Date.now();
  const secs = Math.max(0, Math.floor((now - startedAt) / 1000));
  if (secs >= SECONDS_PER_HOUR) {
    const h = Math.floor(secs / SECONDS_PER_HOUR);
    const m = Math.floor((secs % SECONDS_PER_HOUR) / 60);
    return `${h}h ${m}m`;
  }
  if (secs >= 60) {
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return `${m}m ${s}s`;
  }
  return `${secs}s`;
}

function truncateCommand(cmd: string, maxLen = 60): string {
  return cmd.length > maxLen ? `${cmd.slice(0, maxLen)}…` : cmd;
}

export const BackgroundProcessesModal: React.FC<BackgroundProcessesModalProps> = ({
  isOpen,
  onClose,
}) => {
  const [processes, setProcesses] = useState<BackgroundProcessInfo[]>([]);
  const [killing, setKilling] = useState<Set<number>>(new Set());

  const refresh = useCallback(async () => {
    try {
      const procs = await getBackgroundProcesses();
      setProcesses(procs);
    } catch {
      // silent
    }
  }, []);

  useEffect(() => {
    if (!isOpen) return;
    refresh();
    const id = setInterval(refresh, MODAL_POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, [isOpen, refresh]);

  const handleKill = async (pid: number) => {
    setKilling((prev) => new Set(prev).add(pid));
    try {
      await killBackgroundProcess(pid);
      await refresh();
    } catch {
      // silent
    } finally {
      setKilling((prev) => {
        const next = new Set(prev);
        next.delete(pid);
        return next;
      });
    }
  };

  const handleKillAll = async () => {
    const pids = processes.filter((p) => p.alive).map((p) => p.pid);
    setKilling(new Set(pids));
    try {
      await Promise.all(pids.map((pid) => killBackgroundProcess(pid)));
      await refresh();
    } finally {
      setKilling(new Set());
    }
  };

  const aliveCount = processes.filter((p) => p.alive).length;

  return (
    <Dialog open={isOpen} onOpenChange={onClose}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Terminal className="h-5 w-5" />
            Background Processes
          </DialogTitle>
          <DialogDescription className="sr-only">
            View and manage background processes
          </DialogDescription>
        </DialogHeader>

        <div className="max-h-[50vh] space-y-2 overflow-y-auto py-2">
          {processes.length === 0 && (
            <p className="py-6 text-center text-sm text-muted-foreground">
              No background processes running
            </p>
          )}
          {processes.length > 0 &&
            processes.map((proc) => (
              <div
                key={proc.pid}
                className="flex items-center gap-3 rounded-lg bg-muted/50 px-3 py-2 text-sm"
              >
                <div className="min-w-0 flex-1">
                  <div className="truncate font-mono text-xs" title={proc.command}>
                    {truncateCommand(proc.command)}
                  </div>
                  <div className="mt-0.5 flex items-center gap-2 text-xs text-muted-foreground">
                    <span>PID {proc.pid}</span>
                    <span>·</span>
                    <span>{formatElapsed(proc.startedAt)}</span>
                    {!!proc.alive && (
                      <span className="inline-flex items-center gap-1 text-green-400">
                        <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-green-400" />
                        alive
                      </span>
                    )}
                    {!proc.alive && <span className="text-red-400">exited</span>}
                  </div>
                </div>
                {!!proc.alive && (
                  <button
                    onClick={() => handleKill(proc.pid)}
                    disabled={killing.has(proc.pid)}
                    className="flex-shrink-0 rounded-md p-1.5 text-foreground transition-colors hover:bg-accent disabled:opacity-50"
                    title="Kill process"
                  >
                    <X className="h-4 w-4" />
                  </button>
                )}
              </div>
            ))}
        </div>

        {aliveCount > 1 && (
          <div className="flex justify-end pt-1">
            <button
              onClick={handleKillAll}
              className="flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium text-destructive transition-colors hover:bg-destructive/10"
            >
              <Trash2 className="h-3.5 w-3.5" />
              Kill All
            </button>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
};
