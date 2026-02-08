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
  onStopGeneration: () => void;
  onModelLoad: (modelPath: string, config: SamplerConfig) => void;
}

export function ChatInputArea({
  modelLoaded,
  currentModelPath,
  isModelLoading,
  modelError,
  isLoading,
  onSendMessage,
  onStopGeneration,
  onModelLoad,
}: ChatInputAreaProps) {
  return (
    <div className="px-6 pb-4 pt-2" data-testid="input-container">
      <div className="max-w-3xl mx-auto">
        {modelLoaded ? (
          <MessageInput
            onSendMessage={onSendMessage}
            onStopGeneration={onStopGeneration}
            disabled={isLoading}
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
    </div>
  );
}
