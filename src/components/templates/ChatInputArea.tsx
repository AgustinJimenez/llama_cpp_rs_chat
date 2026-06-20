import { useAgentContext } from '../../contexts/AgentContext';
import { useChatContext } from '../../contexts/ChatContext';
import { useModelContext } from '../../contexts/ModelContext';
import { MessageInput } from '../molecules';

export const ChatInputArea = () => {
  const { status, activeProvider } = useModelContext();
  const { currentConversationId } = useChatContext();
  const { stagedAgent, conversationAgent } = useAgentContext();
  const activeAgent = currentConversationId
    ? conversationAgent
    : (conversationAgent ?? stagedAgent);
  if (activeProvider === 'local' && !activeAgent) return null;
  if (!status.loaded && activeProvider === 'local') return null;

  return (
    <div className="px-6 pb-4 pt-2" data-testid="input-container">
      <div className="mx-auto max-w-3xl">
        <MessageInput />
      </div>
    </div>
  );
};
