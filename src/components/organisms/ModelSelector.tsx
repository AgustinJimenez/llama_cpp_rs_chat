import { ChevronDown, Loader2 } from 'lucide-react';
import React from 'react';

import type { LoadingAction } from '../../hooks/useModel';
import { Button } from '../atoms/button';

interface ModelSelectorProps {
  currentModelPath?: string;
  isLoading?: boolean;
  loadingAction?: LoadingAction;
  loadingProgress?: number;
  onOpen: () => void;
}

export const ModelSelector: React.FC<ModelSelectorProps> = ({
  currentModelPath,
  isLoading = false,
  loadingAction,
  loadingProgress,
  onOpen,
}) => {
  const getDisplayText = () => {
    if (isLoading) {
      if (loadingAction === 'unloading') return 'Unloading...';
      if (loadingProgress != null && loadingProgress > 100) return 'Preparing...';
      if (loadingProgress != null && loadingProgress > 0) return `Loading ${loadingProgress}%`;
      return 'Loading...';
    }
    if (currentModelPath) {
      const fileName = currentModelPath.split(/[/\\]/).pop() || currentModelPath;
      return fileName.replace(/\.gguf$/i, '');
    }
    return 'Select a provider';
  };

  const showProgressBar =
    isLoading && loadingAction === 'loading' && loadingProgress != null && loadingProgress > 0;

  return (
    <div className="flex items-center" data-testid="model-selector">
      <div className="relative">
        <Button
          data-testid="select-model-button"
          onClick={onOpen}
          disabled={isLoading}
          variant="ghost"
          className={`flex items-center gap-1.5 text-sm font-medium px-2 ${isLoading ? 'disabled:opacity-100' : ''}`}
        >
          {isLoading ? (
            <Loader2 className="h-4 w-4 flex-shrink-0 animate-spin text-muted-foreground" />
          ) : null}
          <span className={`truncate max-w-[260px] ${isLoading ? 'text-muted-foreground' : ''}`}>
            {getDisplayText()}
          </span>
          <ChevronDown className="h-3 w-3 text-muted-foreground flex-shrink-0" />
        </Button>
        {showProgressBar ? (
          <div className="absolute bottom-0 left-1 right-1 h-0.5 bg-muted overflow-hidden rounded-full">
            <div
              className={`h-full bg-primary ${(loadingProgress ?? 0) > 100 ? 'animate-pulse' : 'transition-all duration-300 ease-out'}`}
              style={{ width: (loadingProgress ?? 0) > 100 ? '100%' : `${loadingProgress}%` }}
            />
          </div>
        ) : null}
      </div>
    </div>
  );
};
