import { RefreshCw, WifiOff } from 'lucide-react';
import React, { useEffect, useState } from 'react';

import { useConnection } from '../../hooks/useConnection';

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
    if (!disconnectedAt) {
      setElapsed('');
      return;
    }
    const update = () => setElapsed(formatElapsed(Date.now() - disconnectedAt));
    update();
    const id = setInterval(update, 1000);
    return () => clearInterval(id);
  }, [disconnectedAt]);

  if (connected) return null;

  const attemptLabel = attempt > 0 ? ` (attempt ${attempt})` : '';
  const elapsedLabel = elapsed ? ` — ${elapsed} ago` : '';

  return (
    <div
      role="alert"
      className="flex items-center justify-center gap-2 border-b border-red-700 bg-red-900/80 px-4 py-2 text-sm text-red-100"
    >
      {!!reconnecting && (
        <>
          <RefreshCw size={14} className="animate-spin" />
          <span>
            Server unreachable — retrying{attemptLabel}
            {elapsedLabel}
          </span>
        </>
      )}
      {!reconnecting && (
        <>
          <WifiOff size={14} />
          <span>Server disconnected</span>
        </>
      )}
    </div>
  );
};
