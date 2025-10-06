import React from 'react';

export const LoadingIndicator: React.FC = () => {
  return (
    <div className="w-full" data-testid="loading-indicator">
      <div className="flex gap-1">
        <div className="w-2 h-2 bg-slate-400 rounded-full animate-pulse" data-testid="loading-dot-1"></div>
        <div className="w-2 h-2 bg-slate-400 rounded-full animate-pulse [animation-delay:200ms]" data-testid="loading-dot-2"></div>
        <div className="w-2 h-2 bg-slate-400 rounded-full animate-pulse [animation-delay:400ms]" data-testid="loading-dot-3"></div>
      </div>
    </div>
  );
};