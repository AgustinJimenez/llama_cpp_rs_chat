import { Zap, Loader2, X } from 'lucide-react';
import React from 'react';

import { useModelContext } from '../../contexts/ModelContext';
import { useUIContext } from '../../hooks/useUIContext';
import { getProviderLabel } from '../../utils/providerLabels';

interface WelcomeMessageProps {
  children?: React.ReactNode;
}

export const WelcomeMessage: React.FC<WelcomeMessageProps> = ({ children }) => {
  const {
    status,
    isLoading,
    loadingAction,
    modelName,
    forceUnload,
    activeProvider,
    activeProviderModel,
  } = useModelContext();
  const { openProviderSelector } = useUIContext();
  const remoteProviderLabel = getProviderLabel(activeProvider);
  const remoteHeading = `${remoteProviderLabel} (${activeProviderModel})`;

  // Show loading here only when the header is hidden (model not yet loaded).
  // When status.loaded is true, the header is visible and its ModelSelector handles loading/unloading state — only one indicator at a time.
  if (isLoading && !status.loaded) {
    const progress = status.loading_progress;
    const isWarmup = loadingAction === 'loading' && progress != null && progress > 100;
    const hasProgress =
      loadingAction === 'loading' && progress != null && progress > 0 && !isWarmup;
    let text = 'Loading model...';
    if (loadingAction === 'unloading') {
      text = 'Unloading model...';
    } else if (isWarmup) {
      text = 'Preparing system prompt...';
    } else if (hasProgress) {
      text = `Loading model... ${progress}%`;
    }
    return (
      <div className="flex-1 flex flex-col items-center justify-center">
        {hasProgress || isWarmup ? (
          <div className="w-48 h-1.5 bg-foreground/20 rounded-full overflow-hidden">
            <div
              className={`h-full bg-foreground rounded-full ${isWarmup ? 'animate-pulse' : 'transition-all duration-300 ease-out'}`}
              style={{ width: isWarmup ? '100%' : `${progress}%` }}
            />
          </div>
        ) : (
          <Loader2 className="h-6 w-6 text-foreground animate-spin" />
        )}
        <p className="text-foreground text-sm mt-3">{text}</p>
        {loadingAction === 'loading' ? (
          <button
            type="button"
            onClick={forceUnload}
            className="mt-4 flex items-center gap-1.5 px-3 py-1.5 text-sm text-foreground hover:bg-muted rounded-md transition-colors"
            aria-label="Cancel model loading"
          >
            <X className="h-3.5 w-3.5" />
            Cancel
          </button>
        ) : null}
      </div>
    );
  }

  if ((status.loaded && modelName) || activeProvider !== 'local') {
    return (
      <div className="flex-1 flex flex-col items-center justify-center">
        <h2 className="text-xl font-semibold mb-6">
          {activeProvider !== 'local' ? remoteHeading : modelName}
        </h2>
        {children}
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col items-center justify-center gap-3">
      <button
        type="button"
        onClick={openProviderSelector}
        className="flat-card flex flex-col items-center gap-3 px-10 py-8 bg-muted/50 hover:bg-muted transition-colors cursor-pointer"
      >
        <Zap className="h-8 w-8 text-foreground" />
        <span className="text-sm font-medium text-foreground">
          Choose a provider to start chatting
        </span>
      </button>
    </div>
  );
};
