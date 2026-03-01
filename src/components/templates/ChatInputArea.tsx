import { Square } from 'lucide-react';
import { MessageInput } from '../molecules';
import { useModelContext } from '../../contexts/ModelContext';
import { useChatContext } from '../../contexts/ChatContext';

export function ChatInputArea() {
  const { status } = useModelContext();
  const { isLoading, stopGeneration } = useChatContext();
  if (!status.loaded) return null;

  return (
    <div className="px-6 pb-4 pt-2" data-testid="input-container">
      <div className="max-w-3xl mx-auto">
        <div className="flex justify-end mb-2">
          <button
            type="button"
            onClick={stopGeneration ?? undefined}
            disabled={!isLoading}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs font-medium transition-colors ${
              isLoading
                ? 'bg-red-600 hover:bg-red-700 text-white cursor-pointer'
                : 'bg-muted text-muted-foreground cursor-not-allowed opacity-50'
            }`}
            title="Stop generation"
          >
            <Square className="h-3 w-3" />
            Stop
          </button>
        </div>
        <MessageInput />
      </div>
    </div>
  );
}
