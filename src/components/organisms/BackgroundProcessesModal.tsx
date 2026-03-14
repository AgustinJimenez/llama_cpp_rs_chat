import React, { useState, useEffect, useCallback } from 'react';
import { Terminal, Skull, Trash2 } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogDescription,
  DialogTitle,
} from '../atoms/dialog';
import { getBackgroundProcesses, killBackgroundProcess } from '../../utils/tauriCommands';
import type { BackgroundProcessInfo } from '../../utils/tauriCommands';

interface BackgroundProcessesModalProps {
  isOpen: boolean;
  onClose: () => void;
}

function formatElapsed(startedAt: number): string {
  const now = Date.now();
  const secs = Math.max(0, Math.floor((now - startedAt) / 1000));
  if (secs >= 3600) {
    const h = Math.floor(secs / 3600);
    const m = Math.floor((secs % 3600) / 60);
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
  return cmd.length > maxLen ? cmd.slice(0, maxLen) + '…' : cmd;
}

export const BackgroundProcessesModal: React.FC<BackgroundProcessesModalProps> = ({ isOpen, onClose }) => {
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
    const id = setInterval(refresh, 3000);
    return () => clearInterval(id);
  }, [isOpen, refresh]);

  const handleKill = async (pid: number) => {
    setKilling(prev => new Set(prev).add(pid));
    try {
      await killBackgroundProcess(pid);
      await refresh();
    } catch {
      // silent
    } finally {
      setKilling(prev => {
        const next = new Set(prev);
        next.delete(pid);
        return next;
      });
    }
  };

  const handleKillAll = async () => {
    const pids = processes.filter(p => p.alive).map(p => p.pid);
    setKilling(new Set(pids));
    try {
      await Promise.all(pids.map(pid => killBackgroundProcess(pid)));
      await refresh();
    } finally {
      setKilling(new Set());
    }
  };

  const aliveCount = processes.filter(p => p.alive).length;

  return (
    <Dialog open={isOpen} onOpenChange={onClose}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Terminal className="h-5 w-5" />
            Background Processes
            {aliveCount > 0 ? (
              <span className="ml-auto text-xs font-normal text-muted-foreground">
                {aliveCount} running
              </span>
            ) : null}
          </DialogTitle>
          <DialogDescription className="sr-only">
            View and manage background processes
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-2 py-2 max-h-[50vh] overflow-y-auto">
          {processes.length === 0 ? (
            <p className="text-sm text-muted-foreground text-center py-6">
              No background processes running
            </p>
          ) : (
            processes.map(proc => (
              <div
                key={proc.pid}
                className="flex items-center gap-3 px-3 py-2 rounded-lg bg-muted/50 text-sm"
              >
                <div className="flex-1 min-w-0">
                  <div className="font-mono text-xs truncate" title={proc.command}>
                    {truncateCommand(proc.command)}
                  </div>
                  <div className="flex items-center gap-2 mt-0.5 text-xs text-muted-foreground">
                    <span>PID {proc.pid}</span>
                    <span>·</span>
                    <span>{formatElapsed(proc.startedAt)}</span>
                    {proc.alive ? (
                      <span className="inline-flex items-center gap-1 text-green-400">
                        <span className="w-1.5 h-1.5 rounded-full bg-green-400 animate-pulse" />
                        alive
                      </span>
                    ) : (
                      <span className="text-red-400">exited</span>
                    )}
                  </div>
                </div>
                {proc.alive ? (
                  <button
                    onClick={() => handleKill(proc.pid)}
                    disabled={killing.has(proc.pid)}
                    className="flex-shrink-0 p-1.5 rounded-md hover:bg-destructive/20 text-destructive transition-colors disabled:opacity-50"
                    title="Kill process"
                  >
                    <Skull className="h-4 w-4" />
                  </button>
                ) : null}
              </div>
            ))
          )}
        </div>

        {aliveCount > 1 ? (
          <div className="flex justify-end pt-1">
            <button
              onClick={handleKillAll}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium text-destructive hover:bg-destructive/10 transition-colors"
            >
              <Trash2 className="h-3.5 w-3.5" />
              Kill All
            </button>
          </div>
        ) : null}
      </DialogContent>
    </Dialog>
  );
};
