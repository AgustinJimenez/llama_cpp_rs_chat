import React, { useEffect, useState } from 'react';
import { RefreshCw, WifiOff } from 'lucide-react';
import { useConnection } from '../../contexts/ConnectionContext';

function formatElapsed(ms: number): string {
  const secs = Math.floor(ms / 1000);
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  const remainSecs = secs % 60;
  return `${mins}m ${remainSecs}s`;
}

export const ConnectionBanner: React.FC = () => {
  const { connected, reconnecting, attempt, disconnectedAt } = useConnection();
  const [elapsed, setElapsed] = useState('');

  useEffect(() => {
    if (!disconnectedAt) { setElapsed(''); return; }
    const update = () => setElapsed(formatElapsed(Date.now() - disconnectedAt));
    update();
    const id = setInterval(update, 1000);
    return () => clearInterval(id);
  }, [disconnectedAt]);

  if (connected) return null;

  return (
    <div
      role="alert"
      className="flex items-center justify-center gap-2 px-4 py-2 bg-red-900/80 border-b border-red-700 text-red-100 text-sm"
    >
      {reconnecting ? (
        <>
          <RefreshCw size={14} className="animate-spin" />
          <span>
            Server unreachable — retrying{attempt > 0 ? ` (attempt ${attempt})` : ''}
            {elapsed ? ` — ${elapsed} ago` : ''}
          </span>
        </>
      ) : (
        <>
          <WifiOff size={14} />
          <span>Server disconnected</span>
        </>
      )}
    </div>
  );
};
