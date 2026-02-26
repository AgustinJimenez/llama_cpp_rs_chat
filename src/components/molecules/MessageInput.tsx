import React, { useState, useCallback, KeyboardEvent, useEffect, useRef } from 'react';
import { ArrowUp, Square, X, Image as ImageIcon } from 'lucide-react';

interface MessageInputProps {
  onSendMessage: (message: string, imageData?: string[]) => void;
  onStopGeneration?: () => void;
  disabled?: boolean;
  disabledReason?: string;
  hasVision?: boolean;
}

export const MessageInput: React.FC<MessageInputProps> = ({
  onSendMessage,
  onStopGeneration,
  disabled = false,
  disabledReason,
  hasVision = false,
}) => {
  const [message, setMessage] = useState('');
  const [attachedImages, setAttachedImages] = useState<string[]>([]);
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
    const hasContent = message.trim() || attachedImages.length > 0;
    if (hasContent && !disabled) {
      onSendMessage(
        message.trim() || 'What is in this image?',
        attachedImages.length > 0 ? attachedImages : undefined,
      );
      setMessage('');
      setAttachedImages([]);
      if (textareaRef.current) {
        textareaRef.current.style.height = 'auto';
      }
    }
  }, [message, attachedImages, disabled, onSendMessage]);

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

  // Handle clipboard paste for images
  const handlePaste = useCallback((e: React.ClipboardEvent<HTMLTextAreaElement>) => {
    if (!hasVision) return;

    const items = e.clipboardData?.items;
    if (!items) return;

    const imageFiles: File[] = [];
    for (const item of items) {
      if (item.type.startsWith('image/')) {
        const file = item.getAsFile();
        if (file) imageFiles.push(file);
      }
    }

    if (imageFiles.length === 0) return;
    e.preventDefault();

    for (const file of imageFiles) {
      const reader = new FileReader();
      reader.onload = (ev) => {
        const dataUrl = ev.target?.result as string;
        if (dataUrl) {
          setAttachedImages(prev => [...prev, dataUrl]);
        }
      };
      reader.readAsDataURL(file);
    }
  }, [hasVision]);

  const removeImage = useCallback((index: number) => {
    setAttachedImages(prev => prev.filter((_, i) => i !== index));
  }, []);

  const isMultiline = message.includes('\n') || (textareaRef.current?.scrollHeight ?? 0) > 40;
  const hasContent = message.trim() || attachedImages.length > 0;

  return (
    <form onSubmit={handleSubmit} data-testid="message-form">
      {/* Image previews */}
      {attachedImages.length > 0 ? (
        <div className="px-5 pt-2 pb-1 flex flex-wrap gap-2">
          {attachedImages.map((img, i) => (
            <div key={i} className="relative inline-block">
              <img
                src={img}
                alt="Attached"
                className="max-h-24 max-w-48 rounded-lg border border-border object-cover"
              />
              <button
                type="button"
                onClick={() => removeImage(i)}
                className="absolute -top-2 -right-2 w-5 h-5 flex items-center justify-center rounded-full bg-red-500 text-white hover:bg-red-600 transition-colors"
                title="Remove image"
              >
                <X className="h-3 w-3" />
              </button>
            </div>
          ))}
        </div>
      ) : null}

      <div className={`flat-input-container flex items-end gap-2 px-5 py-2.5 ${isMultiline ? '!rounded-2xl' : ''}`}>
        {/* Vision indicator */}
        {hasVision && attachedImages.length === 0 ? (
          <div className="flex-shrink-0 flex items-center py-1 opacity-30" title="Paste an image (Ctrl+V)">
            <ImageIcon className="h-4 w-4" />
          </div>
        ) : null}

        <textarea
          ref={textareaRef}
          value={message}
          onChange={handleTextareaChange}
          onKeyDown={handleKeyDown}
          onPaste={handlePaste}
          placeholder={disabled && disabledReason ? disabledReason : "Ask anything"}
          disabled={disabled}
          className="flex-1 bg-transparent border-none outline-none resize-none text-sm text-foreground placeholder:text-muted-foreground min-h-[28px] py-1 overflow-y-auto"
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
            <Square className="h-3.5 w-3.5" />
          </button>
        ) : (
          <button
            type="submit"
            disabled={disabled || !hasContent}
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
