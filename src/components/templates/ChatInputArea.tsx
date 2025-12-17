import { MessageInput } from '../molecules';
import { ModelSelector } from '../organisms';
import type { SamplerConfig } from '../../types';

interface ChatInputAreaProps {
  modelLoaded: boolean;
  currentModelPath?: string;
  isModelLoading: boolean;
  modelError?: string | null;
  isLoading: boolean;
  isWsConnected: boolean;
  currentConversationId?: string | null;
  onSendMessage: (message: string) => void;
  onModelLoad: (modelPath: string, config: SamplerConfig) => void;
}

export function ChatInputArea({
  modelLoaded,
  currentModelPath,
  isModelLoading,
  modelError,
  isLoading,
  isWsConnected,
  currentConversationId,
  onSendMessage,
  onModelLoad,
}: ChatInputAreaProps) {
  const isWsBlock = !!currentConversationId && !isWsConnected;
  return (
    <div className="border-t border-border bg-card px-4 pt-3 pb-2" data-testid="input-container">
      {modelLoaded ? (
        <MessageInput
          onSendMessage={onSendMessage}
          disabled={isLoading || isWsBlock}
          disabledReason={isWsBlock ? 'Disconnected from chat server' : undefined}
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
