import { useRef, useEffect } from 'react';
import { LoadingIndicator, WelcomeMessage } from '../atoms';
import { MessageBubble } from '../organisms';
import type { Message, ViewMode } from '../../types';

interface MessagesAreaProps {
  messages: Message[];
  isLoading: boolean;
  modelLoaded: boolean;
  isModelLoading: boolean;
  viewMode: ViewMode;
}

export function MessagesArea({
  messages,
  isLoading,
  modelLoaded,
  isModelLoading,
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
        <WelcomeMessage modelLoaded={modelLoaded} isModelLoading={isModelLoading} />
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
