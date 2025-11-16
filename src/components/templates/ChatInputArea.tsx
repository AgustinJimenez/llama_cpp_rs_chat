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
}

export function ChatInputArea({
  modelLoaded,
  currentModelPath,
  isModelLoading,
  modelError,
  isLoading,
  onSendMessage,
  onModelLoad,
}: ChatInputAreaProps) {
  return (
    <div className="border-t border-border bg-card px-6 pt-6 pb-3" data-testid="input-container">
      {modelLoaded ? (
        <MessageInput onSendMessage={onSendMessage} disabled={isLoading} />
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
