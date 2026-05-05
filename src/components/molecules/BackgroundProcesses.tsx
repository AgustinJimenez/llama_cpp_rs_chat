import { Terminal } from 'lucide-react';
import { useState, useEffect, useCallback } from 'react';

import { BackgroundProcessesModal } from '../organisms/BackgroundProcessesModal';

const BACKGROUND_POLL_INTERVAL_MS = 10000;

interface BackgroundProcess {
  pid: number;
  alive: boolean;
}

async function fetchProcesses(): Promise<BackgroundProcess[]> {
  try {
    const res = await fetch('/api/system/processes');
    return (await res.json()) as BackgroundProcess[];
  } catch {
    return [];
  }
}

/** Sidebar indicator for background processes. Opens the shared modal on click. */
export const BackgroundProcesses = () => {
  const [aliveCount, setAliveCount] = useState(0);
  const [modalOpen, setModalOpen] = useState(false);

  const refresh = useCallback(async () => {
    const procs = await fetchProcesses();
    setAliveCount(procs.filter((p) => p.alive).length);
  }, []);

  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, BACKGROUND_POLL_INTERVAL_MS);
    return () => clearInterval(interval);
  }, [refresh]);

  if (aliveCount === 0) return null;

  return (
    <>
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
      <BackgroundProcessesModal isOpen={modalOpen} onClose={() => { setModalOpen(false); refresh(); }} />
    </>
  );
};
