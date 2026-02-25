import { MessageInput } from '../molecules';

interface ChatInputAreaProps {
  modelLoaded: boolean;
  isLoading: boolean;
  onSendMessage: (message: string) => void;
  onStopGeneration: () => void;
}

export function ChatInputArea({
  modelLoaded,
  isLoading,
  onSendMessage,
  onStopGeneration,
}: ChatInputAreaProps) {
  if (!modelLoaded) return null;

  return (
    <div className="px-6 pb-4 pt-2" data-testid="input-container">
      <div className="max-w-3xl mx-auto">
        <MessageInput
          onSendMessage={onSendMessage}
          onStopGeneration={onStopGeneration}
          disabled={isLoading}
        />
      </div>
    </div>
  );
}
