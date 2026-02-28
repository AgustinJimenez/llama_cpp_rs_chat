import React from 'react';
import { RefreshCw, WifiOff } from 'lucide-react';
import { useConnection } from '../../contexts/ConnectionContext';

export const ConnectionBanner: React.FC = () => {
  const { connected, reconnecting } = useConnection();

  if (connected) return null;

  return (
    <div
      role="alert"
      className="flex items-center justify-center gap-2 px-4 py-2 bg-red-900/80 border-b border-red-700 text-red-100 text-sm"
    >
      {reconnecting ? (
        <>
          <RefreshCw size={14} className="animate-spin" />
          <span>Reconnecting to server...</span>
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
