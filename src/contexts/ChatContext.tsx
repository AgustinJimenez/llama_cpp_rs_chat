import { createContext, useContext, useMemo, type ReactNode } from 'react';
import { useChat } from '../hooks/useChat';
import type { Message } from '../types';
import type { TimingInfo } from '../utils/chatTransport';

interface ChatContextValue {
  messages: Message[];
  isLoading: boolean;
  error: string | null;
  sendMessage: (content: string, imageData?: string[], bypassLoadingCheck?: boolean) => void;
  editMessage: (messageIndex: number, newContent: string) => void;
  regenerateFrom: (messageIndex: number) => void;
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

  // Individual field deps are intentional — useChat() returns a new object every render,
  // so using `chat` directly would defeat memoization.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const value = useMemo(() => chat, [
    chat.messages, chat.isLoading, chat.error,
    chat.sendMessage, chat.editMessage, chat.regenerateFrom, chat.stopGeneration,
    chat.clearMessages, chat.loadConversation,
    chat.currentConversationId, chat.tokensUsed, chat.maxTokens, chat.lastTimings, chat.streamStatus, chat.providerRef,
  ]);

  return (
    <ChatContext.Provider value={value}>
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
