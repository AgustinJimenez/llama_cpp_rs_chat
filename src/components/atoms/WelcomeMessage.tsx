import React from 'react';
import { MessageSquare, Loader2 } from 'lucide-react';
import type { LoadingAction } from '../../hooks/useModel';

interface WelcomeMessageProps {
  modelLoaded: boolean;
  isModelLoading?: boolean;
  loadingAction?: LoadingAction;
}

export const WelcomeMessage: React.FC<WelcomeMessageProps> = ({ modelLoaded, isModelLoading = false, loadingAction }) => {
  const getLoadingText = () => {
    if (loadingAction === 'unloading') {
      return "Unloading model...";
    }
    return "Loading model...";
  };

  return (
    <div className="flex flex-col items-center justify-center py-16">
      <div className="text-center space-y-6">
        <div className="w-20 h-20 bg-primary rounded-2xl flex items-center justify-center mx-auto">
          {isModelLoading ? (
            <Loader2 className="h-10 w-10 text-white animate-spin" />
          ) : (
            <MessageSquare className="h-10 w-10 text-white" />
          )}
        </div>

        <div className="space-y-2">
          <p className="text-foreground font-medium text-lg">
            {isModelLoading
              ? getLoadingText()
              : modelLoaded
                ? "Start a conversation with your AI assistant"
                : "No model loaded"
            }
          </p>
        </div>
      </div>
    </div>
  );
};