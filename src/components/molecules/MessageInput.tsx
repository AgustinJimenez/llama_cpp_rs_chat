import React, { useState, useCallback, KeyboardEvent, useEffect, useRef } from 'react';
import { Send, Square } from 'lucide-react';

interface MessageInputProps {
  onSendMessage: (message: string) => void;
  disabled?: boolean;
  disabledReason?: string;
  onStopGeneration?: () => void;
}

export const MessageInput: React.FC<MessageInputProps> = ({
  onSendMessage,
  disabled = false,
  disabledReason,
  onStopGeneration,
}) => {
  const [message, setMessage] = useState('');
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Auto-focus the input when component mounts
  useEffect(() => {
    if (textareaRef.current) {
      textareaRef.current.focus();
    }
  }, []);

  // Auto-focus when disabled changes from true to false (LLM finishes generating)
  useEffect(() => {
    if (!disabled && textareaRef.current) {
      textareaRef.current.focus();
    }
  }, [disabled]);

  const handleSubmit = useCallback((e: React.FormEvent) => {
    e.preventDefault();
    if (message.trim() && !disabled) {
      onSendMessage(message.trim());
      setMessage('');
    }
  }, [message, disabled, onSendMessage]);

  const handleKeyDown = useCallback((e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSubmit(e);
    }
  }, [handleSubmit]);

  const handleTextareaChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setMessage(e.target.value);
  }, []);

  return (
    <form onSubmit={handleSubmit} className="flex gap-2 items-start" data-testid="message-form">
      <div className="flex-1">
        <textarea
          ref={textareaRef}
          value={message}
          onChange={handleTextareaChange}
          onKeyDown={handleKeyDown}
          placeholder="Type your message..."
          disabled={disabled}
          className="flat-input w-full h-[60px] resize-none"
          rows={2}
          data-testid="message-input"
          aria-disabled={disabled}
          aria-label={disabled && disabledReason ? disabledReason : 'Message input'}
        />
      </div>
      {disabled && onStopGeneration ? (
        <button
          type="button"
          onClick={onStopGeneration}
          className="flat-button bg-destructive text-white px-6 h-[60px] min-w-[60px] flex items-center justify-center hover:bg-destructive/90 active:scale-95"
          data-testid="stop-button"
          title="Stop generation"
        >
          <Square className="h-5 w-5" fill="currentColor" />
        </button>
      ) : (
        <button
          type="submit"
          disabled={disabled || !message.trim()}
          className="flat-button bg-primary text-white px-6 h-[60px] min-w-[60px] flex items-center justify-center disabled:opacity-50 disabled:cursor-not-allowed"
          data-testid="send-button"
          title={disabled && disabledReason ? disabledReason : undefined}
        >
          <Send className="h-5 w-5" />
        </button>
      )}
    </form>
  );
};
