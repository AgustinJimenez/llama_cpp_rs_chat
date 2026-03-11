import React from 'react';
import { FolderOpen, Loader2, X } from 'lucide-react';
import { useModelContext } from '../../contexts/ModelContext';
import { useUIContext } from '../../contexts/UIContext';

interface WelcomeMessageProps {
  children?: React.ReactNode;
}

export const WelcomeMessage: React.FC<WelcomeMessageProps> = ({ children }) => {
  const { status, isLoading, loadingAction, modelName, forceUnload } = useModelContext();
  const { openModelConfig } = useUIContext();

  // Show loading here only when the header is hidden (model not yet loaded).
  // When status.loaded is true, the header is visible and its ModelSelector handles loading/unloading state — only one indicator at a time.
  if (isLoading && !status.loaded) {
    const progress = status.loading_progress;
    const hasProgress = loadingAction === 'loading' && progress != null && progress > 0;
    const text = loadingAction === 'unloading'
      ? 'Unloading model...'
      : hasProgress ? `Loading model... ${progress}%` : 'Loading model...';
    return (
      <div className="flex-1 flex flex-col items-center justify-center">
        {hasProgress ? (
          <div className="w-48 h-1.5 bg-muted rounded-full overflow-hidden">
            <div
              className="h-full bg-primary transition-all duration-300 ease-out rounded-full"
              style={{ width: `${progress}%` }}
            />
          </div>
        ) : (
          <Loader2 className="h-6 w-6 text-muted-foreground animate-spin" />
        )}
        <p className="text-muted-foreground text-sm mt-3">{text}</p>
        {loadingAction === 'loading' ? (
          <button
            type="button"
            onClick={forceUnload}
            className="mt-4 flex items-center gap-1.5 px-3 py-1.5 text-sm text-muted-foreground hover:text-foreground hover:bg-muted rounded-md transition-colors"
            aria-label="Cancel model loading"
          >
            <X className="h-3.5 w-3.5" />
            Cancel
          </button>
        ) : null}
      </div>
    );
  }

  if (status.loaded && modelName) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center">
        <h2 className="text-xl font-semibold mb-6">{modelName}</h2>
        {children}
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col items-center justify-center">
      <button
        type="button"
        onClick={openModelConfig}
        className="flex flex-col items-center gap-3 px-10 py-8 rounded-xl bg-muted/50 hover:bg-muted transition-colors cursor-pointer"
      >
        <FolderOpen className="h-8 w-8 text-muted-foreground" />
        <span className="text-sm font-medium">Load a model to start chatting</span>
      </button>
    </div>
  );
};
