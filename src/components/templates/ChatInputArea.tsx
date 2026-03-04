import { MessageInput } from '../molecules';
import { useModelContext } from '../../contexts/ModelContext';

export function ChatInputArea() {
  const { status } = useModelContext();
  if (!status.loaded) return null;

  return (
    <div className="px-6 pb-4 pt-2" data-testid="input-container">
      <div className="max-w-3xl mx-auto">
        <MessageInput />
      </div>
    </div>
  );
}
