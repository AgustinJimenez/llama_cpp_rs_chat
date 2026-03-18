import { useState, useEffect, useCallback } from 'react';
import { Terminal, X, RefreshCw } from 'lucide-react';

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
    return (data as BackgroundProcess[]).filter(p => p.alive);
  } catch {
    return [];
  }
}

async function killProcess(pid: number): Promise<boolean> {
  try {
    const res = await fetch('/api/system/processes/kill', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ pid }),
    });
    return res.ok;
  } catch {
    return false;
  }
}

export function BackgroundProcesses() {
  const [processes, setProcesses] = useState<BackgroundProcess[]>([]);
  const [expanded, setExpanded] = useState(false);

  const refresh = useCallback(async () => {
    const procs = await fetchProcesses();
    setProcesses(procs);
  }, []);

  // Poll every 10 seconds
  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, 10000);
    return () => clearInterval(interval);
  }, [refresh]);

  const handleKill = async (pid: number) => {
    await killProcess(pid);
    await refresh();
  };

  if (processes.length === 0) return null;

  const truncateCmd = (cmd: string, max = 40) =>
    cmd.length > max ? cmd.substring(0, max) + '...' : cmd;

  const elapsed = (startedAt: number) => {
    const secs = Math.floor(Date.now() / 1000 - startedAt);
    if (secs < 60) return `${secs}s`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m`;
    return `${Math.floor(secs / 3600)}h`;
  };

  return (
    <div className="mx-2 mb-2">
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full flex items-center gap-2 px-2 py-1.5 text-xs text-green-400 hover:bg-green-400/10 rounded transition-colors"
      >
        <Terminal className="h-3 w-3 animate-pulse" />
        <span>{processes.length} background process{processes.length > 1 ? 'es' : ''}</span>
        <RefreshCw
          className="h-3 w-3 ml-auto hover:text-green-300 cursor-pointer"
          onClick={(e) => { e.stopPropagation(); refresh(); }}
        />
      </button>
      {expanded && (
        <div className="mt-1 space-y-1">
          {processes.map((proc) => (
            <div
              key={proc.pid}
              className="flex items-center gap-1.5 px-2 py-1 bg-zinc-800/50 rounded text-[10px]"
            >
              <span className="w-2 h-2 bg-green-500 rounded-full shrink-0 animate-pulse" />
              <span className="text-zinc-300 font-mono truncate flex-1" title={proc.command}>
                {truncateCmd(proc.command)}
              </span>
              <span className="text-zinc-500 shrink-0">{elapsed(proc.startedAt)}</span>
              <span className="text-zinc-600 shrink-0">PID:{proc.pid}</span>
              <button
                onClick={() => handleKill(proc.pid)}
                className="text-red-400/60 hover:text-red-400 transition-colors shrink-0"
                title="Kill process"
              >
                <X className="h-3 w-3" />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
