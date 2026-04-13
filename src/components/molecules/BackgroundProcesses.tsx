import { Terminal, X, RefreshCw, Clock, Hash, Activity } from 'lucide-react';
import { useState, useEffect, useCallback } from 'react';
import { toast } from 'react-hot-toast';

const SECONDS_PER_HOUR = 3600;
const BACKGROUND_POLL_INTERVAL_MS = 10000;
const MODAL_POLL_INTERVAL_MS = 3000;

import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '../atoms/dialog';

interface BackgroundProcess {
  pid: number;
  command: string;
  conversationId: string | null;
  startedAt: number;
  alive: boolean;
}

async function fetchProcesses(): Promise<BackgroundProcess[]> {
  try {
    const res = await fetch('/api/system/processes');
    const data = await res.json();
    return data as BackgroundProcess[];
  } catch {
    return [];
  }
}

async function killProcess(pid: number): Promise<{ success: boolean; message?: string }> {
  try {
    const res = await fetch('/api/system/processes/kill', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ pid }),
    });
    const data = await res.json();
    return { success: data.success ?? res.ok, message: data.message };
  } catch {
    return { success: false, message: 'Failed to connect to server' };
  }
}

function elapsed(startedAt: number): string {
  const secs = Math.floor(Date.now() / 1000 - startedAt);
  if (secs < 60) return `${secs}s`;
  if (secs < SECONDS_PER_HOUR) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
  const h = Math.floor(secs / SECONDS_PER_HOUR);
  const m = Math.floor((secs % SECONDS_PER_HOUR) / 60);
  return `${h}h ${m}m`;
}

function formatTime(ts: number): string {
  return new Date(ts * 1000).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  });
}

export const BackgroundProcesses = () => {
  const [processes, setProcesses] = useState<BackgroundProcess[]>([]);
  const [modalOpen, setModalOpen] = useState(false);
  const [killing, setKilling] = useState<number | null>(null);

  const refresh = useCallback(async () => {
    const procs = await fetchProcesses();
    setProcesses(procs);
  }, []);

  // Poll every 10 seconds
  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, BACKGROUND_POLL_INTERVAL_MS);
    return () => clearInterval(interval);
  }, [refresh]);

  // Poll faster when modal is open
  useEffect(() => {
    if (!modalOpen) return;
    refresh();
    const interval = setInterval(refresh, MODAL_POLL_INTERVAL_MS);
    return () => clearInterval(interval);
  }, [modalOpen, refresh]);

  const handleKill = async (pid: number) => {
    setKilling(pid);
    const result = await killProcess(pid);
    await refresh();
    setKilling(null);
    if (result.success) {
      toast.success(`Process ${pid} killed`, { duration: 2000 });
    } else {
      toast.error(result.message || `Failed to kill process ${pid}`, { duration: 4000 });
    }
  };

  const handleKillAll = async () => {
    for (const proc of processes.filter((p) => p.alive)) {
      await killProcess(proc.pid);
    }
    await refresh();
  };

  const aliveCount = processes.filter((p) => p.alive).length;

  if (processes.length === 0) return null;

  return (
    <>
      {/* Sidebar indicator */}
      <div className="mx-2 mb-2">
        <button
          onClick={() => setModalOpen(true)}
          className="w-full flex items-center gap-2 px-2 py-1.5 text-xs text-green-400 hover:bg-green-400/10 rounded transition-colors"
        >
          <Terminal className="h-3 w-3 animate-pulse" />
          <span>
            {aliveCount} background process{aliveCount !== 1 ? 'es' : ''}
          </span>
        </button>
      </div>

      {/* Modal with full details */}
      <Dialog open={modalOpen} onOpenChange={setModalOpen}>
        <DialogContent className="max-w-xl">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Terminal className="h-5 w-5 text-green-400" />
              Background Processes
            </DialogTitle>
            <DialogDescription className="text-foreground/80">
              Processes started by the model that are running in the background.
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-2 max-h-[60vh] overflow-y-auto">
            {processes.length === 0 ? (
              <p className="text-sm text-muted-foreground text-center py-4">
                No background processes
              </p>
            ) : (
              processes.map((proc) => (
                <div
                  key={proc.pid}
                  className={`p-3 rounded-lg border ${proc.alive ? 'bg-card border-border' : 'bg-card/50 border-border opacity-60'}`}
                >
                  {/* Command */}
                  <div className="flex items-start gap-2 mb-2">
                    <span
                      className={`w-2 h-2 rounded-full shrink-0 mt-1.5 ${proc.alive ? 'bg-green-500 animate-pulse' : 'bg-muted-foreground'}`}
                    />
                    <code className="text-xs text-foreground font-mono break-all flex-1">
                      {proc.command}
                    </code>
                  </div>

                  {/* Details row */}
                  <div className="flex items-center gap-4 text-[11px] text-foreground/80 ml-4">
                    <span className="flex items-center gap-1" title="Process ID">
                      <Hash className="h-3 w-3" />
                      PID {proc.pid}
                    </span>
                    <span className="flex items-center gap-1" title="Started at">
                      <Clock className="h-3 w-3" />
                      {formatTime(proc.startedAt)}
                    </span>
                    <span className="flex items-center gap-1" title="Elapsed time">
                      <Activity className="h-3 w-3" />
                      {elapsed(proc.startedAt)}
                    </span>
                    <span
                      className={`ml-auto font-medium ${proc.alive ? 'text-green-400' : 'text-muted-foreground'}`}
                    >
                      {proc.alive ? 'Running' : 'Exited'}
                    </span>
                  </div>

                  {/* Kill button */}
                  {proc.alive ? (
                    <div className="mt-2 ml-4">
                      <button
                        onClick={() => handleKill(proc.pid)}
                        disabled={killing === proc.pid}
                        className="flex items-center gap-1.5 px-2.5 py-1 text-xs text-red-400 hover:text-red-300 hover:bg-red-400/10 rounded transition-colors disabled:opacity-50"
                      >
                        <X className="h-3 w-3" />
                        {killing === proc.pid ? 'Killing...' : 'Kill Process'}
                      </button>
                    </div>
                  ) : null}
                </div>
              ))
            )}
          </div>

          {/* Footer actions */}
          {aliveCount > 1 && (
            <div className="flex justify-end pt-2 border-t border-border">
              <button
                onClick={handleKillAll}
                className="flex items-center gap-1.5 px-3 py-1.5 text-xs text-red-400 hover:text-red-300 hover:bg-red-400/10 rounded transition-colors"
              >
                <X className="h-3 w-3" />
                Kill All ({aliveCount})
              </button>
            </div>
          )}

          <div className="flex justify-between items-center pt-1 text-[10px] text-foreground/80">
            <span>Auto-refreshes every 3s while open</span>
            <button
              onClick={refresh}
              className="flex items-center gap-1 hover:text-foreground transition-colors"
            >
              <RefreshCw className="h-3 w-3" />
              Refresh
            </button>
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
};
