import { createContext, useContext, type ReactNode } from 'react';
import { useChat } from '../hooks/useChat';
import type { Message } from '../types';
import type { TimingInfo } from '../utils/chatTransport';

interface ChatContextValue {
  messages: Message[];
  isLoading: boolean;
  error: string | null;
  sendMessage: (content: string, imageData?: string[], bypassLoadingCheck?: boolean) => void;
  stopGeneration: () => void;
  clearMessages: () => void;
  loadConversation: (filename: string) => void;
  currentConversationId: string | null;
  tokensUsed?: number;
  maxTokens?: number;
  lastTimings?: TimingInfo;
}

const ChatContext = createContext<ChatContextValue | null>(null);

export function ChatProvider({ children }: { children: ReactNode }) {
  const chat = useChat();

  return (
    <ChatContext.Provider value={chat}>
      {children}
    </ChatContext.Provider>
  );
}

// eslint-disable-next-line react-refresh/only-export-components
export function useChatContext() {
  const ctx = useContext(ChatContext);
  if (!ctx) throw new Error('useChatContext must be used within ChatProvider');
  return ctx;
}
