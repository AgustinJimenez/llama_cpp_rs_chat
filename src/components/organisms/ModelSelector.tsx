import React from 'react';
import { ChevronDown, FolderOpen, Loader2 } from 'lucide-react';
import { Button } from '../atoms/button';
import type { LoadingAction } from '../../hooks/useModel';

interface ModelSelectorProps {
  currentModelPath?: string;
  isLoading?: boolean;
  loadingAction?: LoadingAction;
  onOpen: () => void;
}

export const ModelSelector: React.FC<ModelSelectorProps> = ({
  currentModelPath,
  isLoading = false,
  loadingAction,
  onOpen,
}) => {
  const getDisplayText = () => {
    if (isLoading) return loadingAction === 'unloading' ? "Unloading..." : "Loading...";
    if (currentModelPath) {
      const fileName = currentModelPath.split(/[/\\]/).pop() || currentModelPath;
      return fileName.replace(/\.gguf$/i, '');
    }
    return "Select a model";
  };

  return (
    <div className="flex items-center" data-testid="model-selector">
      <Button
        data-testid="select-model-button"
        onClick={onOpen}
        disabled={isLoading}
        variant="ghost"
        className="flex items-center gap-1.5 text-sm font-medium px-2"
      >
        {isLoading ? <Loader2 className="h-4 w-4 flex-shrink-0 animate-spin" /> : <FolderOpen className="h-4 w-4 flex-shrink-0" />}
        <span className="truncate max-w-[260px]">{getDisplayText()}</span>
        <ChevronDown className="h-3 w-3 text-muted-foreground flex-shrink-0" />
      </Button>
    </div>
  );
};
