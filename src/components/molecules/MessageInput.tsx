import React, { useState, useCallback, KeyboardEvent, useEffect, useRef } from 'react';
import { ArrowUp, Square } from 'lucide-react';

interface MessageInputProps {
  onSendMessage: (message: string) => void;
  onStopGeneration?: () => void;
  disabled?: boolean;
  disabledReason?: string;
}

export const MessageInput: React.FC<MessageInputProps> = ({
  onSendMessage,
  onStopGeneration,
  disabled = false,
  disabledReason,
}) => {
  const [message, setMessage] = useState('');
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (textareaRef.current) {
      textareaRef.current.focus();
    }
  }, []);

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
      if (textareaRef.current) {
        textareaRef.current.style.height = 'auto';
      }
    }
  }, [message, disabled, onSendMessage]);

  const handleKeyDown = useCallback((e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSubmit(e);
    }
  }, [handleSubmit]);

  const autoResize = useCallback((el: HTMLTextAreaElement) => {
    el.style.height = 'auto';
    const lineHeight = 20;
    const maxHeight = lineHeight * 7;
    el.style.height = `${Math.min(el.scrollHeight, maxHeight)}px`;
  }, []);

  const handleTextareaChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setMessage(e.target.value);
    autoResize(e.target);
  }, [autoResize]);

  return (
    <form onSubmit={handleSubmit} data-testid="message-form">
      <div className="flat-input-container flex items-end gap-2 px-4 py-3">
        <textarea
          ref={textareaRef}
          value={message}
          onChange={handleTextareaChange}
          onKeyDown={handleKeyDown}
          placeholder="Message..."
          disabled={disabled}
          className="flex-1 bg-transparent border-none outline-none resize-none text-sm text-foreground placeholder:text-muted-foreground min-h-[24px] py-0.5 overflow-y-auto"
          rows={1}
          data-testid="message-input"
          aria-disabled={disabled}
          aria-label={disabled && disabledReason ? disabledReason : 'Message input'}
        />
        {disabled && onStopGeneration ? (
          <button
            type="button"
            onClick={onStopGeneration}
            className="flex-shrink-0 w-8 h-8 flex items-center justify-center rounded-full bg-white text-black hover:bg-gray-200 transition-colors"
            data-testid="stop-button"
            title="Stop generation"
          >
            <Square className="h-4 w-4" />
          </button>
        ) : (
          <button
            type="submit"
            disabled={disabled || !message.trim()}
            className="flex-shrink-0 w-8 h-8 flex items-center justify-center rounded-full bg-white text-black hover:bg-gray-200 transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
            data-testid="send-button"
            title={disabled && disabledReason ? disabledReason : undefined}
          >
            <ArrowUp className="h-4 w-4" />
          </button>
        )}
      </div>
    </form>
  );
};
