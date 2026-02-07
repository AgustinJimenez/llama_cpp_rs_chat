import { useRef, useEffect, useState } from 'react';
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
  const containerRef = useRef<HTMLDivElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const lastScrollAtRef = useRef<number>(0);
  const scrollRafRef = useRef<number | null>(null);
  const [shouldAutoScroll, setShouldAutoScroll] = useState(true);

  const scrollToBottom = (behavior: ScrollBehavior) => {
    messagesEndRef.current?.scrollIntoView({ behavior });
  };

  useEffect(() => {
    const now = Date.now();
    const behavior: ScrollBehavior = isLoading ? 'auto' : 'smooth';

    // Avoid scheduling expensive smooth scrolls for every token while streaming.
    if (isLoading && now - lastScrollAtRef.current < 200) {
      return;
    }
    lastScrollAtRef.current = now;

    if (scrollRafRef.current !== null) {
      cancelAnimationFrame(scrollRafRef.current);
    }

    scrollRafRef.current = requestAnimationFrame(() => {
      if (shouldAutoScroll) {
        scrollToBottom(behavior);
      }
      scrollRafRef.current = null;
    });
  }, [messages, isLoading, shouldAutoScroll]);

  const handleScroll = () => {
    const container = containerRef.current;
    if (!container) return;
    const threshold = 120;
    const isNearBottom =
      container.scrollTop + container.clientHeight >= container.scrollHeight - threshold;
    setShouldAutoScroll(isNearBottom);
  };

  return (
    <div
      ref={containerRef}
      className="flex-1 overflow-y-auto overflow-x-hidden p-6 space-y-4"
      data-testid="messages-container"
      onScroll={handleScroll}
    >
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
