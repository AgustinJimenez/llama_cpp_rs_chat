import React from 'react';
import { Card, CardContent } from '@/components/ui/card';
import type { Message } from '../types';

interface MessageBubbleProps {
  message: Message;
}

export const MessageBubble: React.FC<MessageBubbleProps> = ({ message }) => {
  const isUser = message.role === 'user';
  
  if (isUser) {
    // User messages keep the card styling and right alignment
    return (
      <div 
        className="flex w-full justify-end"
        data-testid={`message-${message.role}`}
        data-message-id={message.id}
      >
        <Card className="border-0 shadow-md max-w-[80%] bg-gradient-to-br from-slate-600 to-slate-500 text-white">
          <CardContent className="p-3">
            <p className="text-sm whitespace-pre-wrap leading-relaxed" data-testid="message-content">
              {message.content}
            </p>
          </CardContent>
        </Card>
      </div>
    );
  }
  
  // Assistant messages take full width with no card styling
  return (
    <div 
      className="w-full"
      data-testid={`message-${message.role}`}
      data-message-id={message.id}
    >
      <p className="text-sm whitespace-pre-wrap leading-relaxed text-card-foreground" data-testid="message-content">
        {message.content}
      </p>
    </div>
  );
};