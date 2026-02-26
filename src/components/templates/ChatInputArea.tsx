import { MessageInput } from '../molecules';

interface ChatInputAreaProps {
  modelLoaded: boolean;
  isLoading: boolean;
  onSendMessage: (message: string, imageData?: string[]) => void;
  onStopGeneration: () => void;
  hasVision?: boolean;
}

export function ChatInputArea({
  modelLoaded,
  isLoading,
  onSendMessage,
  onStopGeneration,
  hasVision,
}: ChatInputAreaProps) {
  if (!modelLoaded) return null;

  return (
    <div className="px-6 pb-4 pt-2" data-testid="input-container">
      <div className="max-w-3xl mx-auto">
        <MessageInput
          onSendMessage={onSendMessage}
          onStopGeneration={onStopGeneration}
          disabled={isLoading}
          hasVision={hasVision}
        />
      </div>
    </div>
  );
}
