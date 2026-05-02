import { useModelContext } from '../../contexts/ModelContext';
import { MessageInput } from '../molecules';

export const ChatInputArea = () => {
  const { status, activeProvider } = useModelContext();
  if (!status.loaded && activeProvider === 'local') return null;

  return (
    <div className="px-6 pb-4 pt-2" data-testid="input-container">
      <div className="max-w-3xl mx-auto">
        <MessageInput />
      </div>
    </div>
  );
};
