import { useRef, useEffect } from 'react';
import { LoadingIndicator, WelcomeMessage } from '../atoms';
import { MessageBubble } from '../organisms';
import type { Message, ViewMode } from '../../types';
import type { LoadingAction } from '../../hooks/useModel';

interface MessagesAreaProps {
  messages: Message[];
  isLoading: boolean;
  modelLoaded: boolean;
  isModelLoading: boolean;
  loadingAction?: LoadingAction;
  viewMode: ViewMode;
}

export function MessagesArea({
  messages,
  isLoading,
  modelLoaded,
  isModelLoading,
  loadingAction,
  viewMode,
}: MessagesAreaProps) {
  const messagesEndRef = useRef<HTMLDivElement>(null);

  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

  useEffect(() => {
    scrollToBottom();
  }, [messages, isLoading]);

  return (
    <div className="flex-1 overflow-y-auto p-6 space-y-4" data-testid="messages-container">
      {messages.length === 0 ? (
        <WelcomeMessage modelLoaded={modelLoaded} isModelLoading={isModelLoading} loadingAction={loadingAction} />
      ) : (
        <>
          {messages.map((message) => (
            <MessageBubble key={message.id} message={message} viewMode={viewMode} />
          ))}
          {isLoading && <LoadingIndicator />}
          <div ref={messagesEndRef} />
        </>
      )}
    </div>
  );
}
