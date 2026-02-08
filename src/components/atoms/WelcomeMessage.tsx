import React from 'react';
import { Loader2 } from 'lucide-react';
import type { LoadingAction } from '../../hooks/useModel';

interface WelcomeMessageProps {
  isModelLoading?: boolean;
  loadingAction?: LoadingAction;
}

export const WelcomeMessage: React.FC<WelcomeMessageProps> = ({ isModelLoading = false, loadingAction }) => {
  const getLoadingText = () => {
    if (loadingAction === 'unloading') return "Unloading model...";
    return "Loading model...";
  };

  if (!isModelLoading) return null;

  return (
    <div className="flex flex-col items-center justify-center py-24">
      <div className="text-center space-y-3">
        <Loader2 className="h-6 w-6 text-muted-foreground animate-spin mx-auto" />
        <p className="text-muted-foreground text-sm">{getLoadingText()}</p>
      </div>
    </div>
  );
};
