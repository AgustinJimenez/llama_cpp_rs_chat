import React from 'react';
import { FolderOpen, Loader2 } from 'lucide-react';
import { useModelContext } from '../../contexts/ModelContext';
import { useUIContext } from '../../contexts/UIContext';

interface WelcomeMessageProps {
  children?: React.ReactNode;
}

export const WelcomeMessage: React.FC<WelcomeMessageProps> = ({ children }) => {
  const { status, isLoading, loadingAction, modelName } = useModelContext();
  const { openModelConfig } = useUIContext();

  if (isLoading) {
    const text = loadingAction === 'unloading' ? 'Unloading model...' : 'Loading model...';
    return (
      <div className="flex-1 flex flex-col items-center justify-center">
        <Loader2 className="h-6 w-6 text-muted-foreground animate-spin" />
        <p className="text-muted-foreground text-sm mt-3">{text}</p>
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
