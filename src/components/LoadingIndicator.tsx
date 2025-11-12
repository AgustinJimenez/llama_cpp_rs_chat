import React from 'react';

export const LoadingIndicator: React.FC = () => {
  return (
    <div className="w-full py-4" data-testid="loading-indicator">
      <div className="flex gap-2 items-center justify-start">
        <div
          className="w-3 h-3 bg-flat-red rounded-full flat-pulse"
          style={{ animationDelay: '0ms' }}
          data-testid="loading-dot-1"
        ></div>
        <div
          className="w-3 h-3 bg-flat-red rounded-full flat-pulse"
          style={{ animationDelay: '200ms' }}
          data-testid="loading-dot-2"
        ></div>
        <div
          className="w-3 h-3 bg-flat-red rounded-full flat-pulse"
          style={{ animationDelay: '400ms' }}
          data-testid="loading-dot-3"
        ></div>
      </div>
    </div>
  );
};