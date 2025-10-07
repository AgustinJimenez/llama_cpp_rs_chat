import React from 'react';
import { MessageSquare, Loader2 } from 'lucide-react';

interface WelcomeMessageProps {
  modelLoaded: boolean;
  isModelLoading?: boolean;
}

export const WelcomeMessage: React.FC<WelcomeMessageProps> = ({ modelLoaded, isModelLoading = false }) => {
  return (
    <div className="flex flex-col items-center justify-center py-16">
      <div className="text-center space-y-4">
        <div className="w-16 h-16 bg-gradient-to-br from-slate-600 to-slate-400 rounded-full flex items-center justify-center mx-auto">
          {isModelLoading ? (
            <Loader2 className="h-8 w-8 text-white animate-spin" />
          ) : (
            <MessageSquare className="h-8 w-8 text-white" />
          )}
        </div>
        
        <div className="space-y-2">
          <p className="text-muted-foreground">
            {isModelLoading 
              ? "Loading model..."
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