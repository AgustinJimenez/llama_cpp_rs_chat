import { createContext, useContext, useMemo, type ReactNode, type MutableRefObject } from 'react';

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
  continueFrom: (messageIndex: number) => void;
  stopGeneration: () => void;
  clearMessages: () => void;
  loadConversation: (filename: string) => void;
  currentConversationId: string | null;
  tokensUsed?: number;
  maxTokens?: number;
  lastTimings?: TimingInfo;
  streamStatus?: string;
  providerRef: MutableRefObject<{ provider: string; model: string }>;
  providerParamsRef: MutableRefObject<Record<string, unknown>>;
}

const ChatContext = createContext<ChatContextValue | null>(null);

export const ChatProvider = ({ children }: { children: ReactNode }) => {
  const chat = useChat();

  const value = useMemo(
    () => chat,
    // eslint-disable-next-line react-hooks/exhaustive-deps -- individual field deps intentional
    [
      chat.messages,
      chat.isLoading,
      chat.error,
      chat.sendMessage,
      chat.editMessage,
      chat.regenerateFrom,
      chat.continueFrom,
      chat.stopGeneration,
      chat.clearMessages,
      chat.loadConversation,
      chat.currentConversationId,
      chat.tokensUsed,
      chat.maxTokens,
      chat.lastTimings,
      chat.streamStatus,
      chat.providerRef,
      chat.providerParamsRef,
    ],
  );

  return <ChatContext.Provider value={value}>{children}</ChatContext.Provider>;
};

// eslint-disable-next-line react-refresh/only-export-components
export function useChatContext() {
  const ctx = useContext(ChatContext);
  if (!ctx) throw new Error('useChatContext must be used within ChatProvider');
  return ctx;
}
