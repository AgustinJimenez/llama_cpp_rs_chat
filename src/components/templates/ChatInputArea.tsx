import { MessageInput } from '../molecules';
import { ModelSelector } from '../organisms';
import type { SamplerConfig } from '../../types';

interface ChatInputAreaProps {
  modelLoaded: boolean;
  currentModelPath?: string;
  isModelLoading: boolean;
  modelError?: string | null;
  isLoading: boolean;
  onSendMessage: (message: string) => void;
  onModelLoad: (modelPath: string, config: SamplerConfig) => void;
  onStopGeneration?: () => void;
}

export function ChatInputArea({
  modelLoaded,
  currentModelPath,
  isModelLoading,
  modelError,
  isLoading,
  onSendMessage,
  onModelLoad,
  onStopGeneration,
}: ChatInputAreaProps) {
  return (
    <div className="border-t border-border bg-card px-4 pt-3 pb-2" data-testid="input-container">
      {modelLoaded ? (
        <MessageInput
          onSendMessage={onSendMessage}
          disabled={isLoading}
          onStopGeneration={onStopGeneration}
        />
      ) : (
        <div className="flex justify-center">
          <ModelSelector
            onModelLoad={onModelLoad}
            currentModelPath={currentModelPath}
            isLoading={isModelLoading}
            error={modelError ?? undefined}
          />
        </div>
      )}
    </div>
  );
}
