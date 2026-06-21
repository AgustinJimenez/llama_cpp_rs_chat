import React, { useState, useEffect, useRef } from 'react';

export const LoadingIndicator: React.FC = () => {
  const [elapsed, setElapsed] = useState(0);
  const startRef = useRef(0);

  useEffect(() => {
    startRef.current = Date.now();
    const interval = setInterval(() => {
      setElapsed(Date.now() - startRef.current);
    }, 100);
    return () => clearInterval(interval);
  }, []);

  const totalSeconds = elapsed / 1000;
  const timeLabel =
    totalSeconds < 60
      ? `${totalSeconds.toFixed(1)}s`
      : `${Math.floor(totalSeconds / 60)}m ${String(Math.floor(totalSeconds % 60)).padStart(2, '0')}s`;

  return (
    <div className="py-4" data-testid="loading-indicator">
      <div className="inline-flex flex-col">
        <div className="flex items-center gap-2">
          <div
            className="flat-pulse size-3 rounded-full bg-primary"
            style={{ animationDelay: '0ms' }}
            data-testid="loading-dot-1"
          />
          <div
            className="flat-pulse size-3 rounded-full bg-primary"
            style={{ animationDelay: '200ms' }}
            data-testid="loading-dot-2"
          />
          <div
            className="flat-pulse size-3 rounded-full bg-primary"
            style={{ animationDelay: '400ms' }}
            data-testid="loading-dot-3"
          />
        </div>
        <div className="mt-2 text-right text-[10px] text-white/50">{timeLabel}</div>
      </div>
    </div>
  );
};
