import { ArrowUp, X, FileText, Loader2, Paperclip } from 'lucide-react';
import type { KeyboardEvent } from 'react';
import React, { useState, useCallback, useEffect, useRef } from 'react';

const LINE_HEIGHT_PX = 20;
const MAX_VISIBLE_LINES = 7;
const MULTILINE_SCROLL_THRESHOLD_PX = 40;

import type { TimingInfo } from '../../utils/chatTransport';

import { StatsBar } from './LiveStreamingStats';
import {
  type AttachedFile,
  FILE_ACCEPT,
  formatCharCount,
  buildFinalMessage,
  useFileAttachments,
} from './MessageInputAttachments';
import { useInputState, getPlaceholder } from './useMessageInputState';

const IMG_KEY_SUFFIX_LEN = 32;

const ImagePreviews = ({
  images,
  onRemove,
}: {
  images: string[];
  onRemove: (i: number) => void;
}) => {
  if (images.length === 0) return null;
  return (
    <div className="px-5 pt-2 pb-1 flex flex-wrap gap-2">
      {images.map((img, i) => (
        <div key={`img-${img.slice(-IMG_KEY_SUFFIX_LEN)}`} className="relative inline-block">
          <img
            src={img}
            alt="Attached"
            className="max-h-24 max-w-48 rounded-lg border border-border object-cover"
          />
          <button
            type="button"
            onClick={() => onRemove(i)}
            className="absolute -top-2 -right-2 w-5 h-5 flex items-center justify-center rounded-full bg-red-500 text-white hover:bg-red-600 transition-colors"
            title="Remove image"
          >
            <X className="h-3 w-3" />
          </button>
        </div>
      ))}
    </div>
  );
};

const FilePreviews = ({
  files,
  onRemove,
}: {
  files: AttachedFile[];
  onRemove: (id: string) => void;
}) => {
  if (files.length === 0) return null;
  return (
    <div className="px-5 pt-2 pb-1 flex flex-wrap gap-2">
      {files.map((file) => (
        <div
          key={file.id}
          className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-muted/60 border border-border text-xs"
        >
          <FileText className="h-3.5 w-3.5 text-muted-foreground flex-shrink-0" />
          <span className="font-medium truncate max-w-[150px]" title={file.name}>
            {file.name}
          </span>
          <span className="text-muted-foreground">{formatCharCount(file.text.length)} chars</span>
          <button
            type="button"
            onClick={() => onRemove(file.id)}
            className="ml-0.5 w-4 h-4 flex items-center justify-center rounded-full hover:bg-red-500/20 text-muted-foreground hover:text-red-500 transition-colors"
            title="Remove file"
          >
            <X className="h-3 w-3" />
          </button>
        </div>
      ))}
    </div>
  );
};

const DragOverlay = () => (
  <div className="absolute inset-0 z-10 flex items-center justify-center rounded-2xl border-2 border-dashed border-primary/50 bg-primary/5 backdrop-blur-sm pointer-events-none">
    <div className="flex items-center gap-2 text-sm font-medium text-primary">
      <FileText className="h-5 w-5" />
      Drop files here
    </div>
  </div>
);

const ExtractingIndicator = ({ count }: { count: number }) =>
  count > 0 ? (
    <div className="px-5 pt-1 pb-1 flex items-center gap-2 text-xs text-muted-foreground">
      <Loader2 className="h-3 w-3 animate-spin" />
      Extracting text from {count} file{count > 1 ? 's' : ''}...
    </div>
  ) : null;

interface MessageInputProps {
  disabledReason?: string;
  timings?: TimingInfo;
  tokensUsed?: number;
  maxTokens?: number;
  streamStatus?: string;
}

export const MessageInput: React.FC<MessageInputProps> = ({
  disabledReason,
  timings,
  tokensUsed,
  maxTokens,
  streamStatus,
}) => {
  const {
    onSendMessage,
    isLoading,
    stopGeneration,
    hasVision,
    isModelBusy,
    isGeneratingElsewhere,
    disabled,
    estimatedConvTokens,
    modelContextSize,
    loadingAction,
  } = useInputState();

  const [message, setMessage] = useState('');
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const {
    attachedImages,
    attachedFiles,
    isDragging,
    isExtracting,
    fileInputRef,
    handlePaste,
    handleDragEnter,
    handleDragLeave,
    handleDragOver,
    handleDrop,
    handleFileButtonClick,
    handleFileInputChange,
    removeImage,
    removeFile,
    clearAll,
  } = useFileAttachments(hasVision);

  useEffect(() => {
    if (!disabled && textareaRef.current) textareaRef.current.focus();
  }, [disabled]); // also handles initial mount

  const handleSubmit = useCallback(
    (e: React.FormEvent) => {
      e.preventDefault();
      const hasContent = message.trim() || attachedImages.length > 0 || attachedFiles.length > 0;
      if (!hasContent || disabled || isExtracting > 0) return;

      const finalMessage = buildFinalMessage(message, attachedFiles, attachedImages);
      onSendMessage(finalMessage, attachedImages.length > 0 ? attachedImages : undefined);
      setMessage('');
      clearAll();
      if (textareaRef.current) textareaRef.current.style.height = 'auto';
    },
    [message, attachedImages, attachedFiles, disabled, isExtracting, onSendMessage, clearAll],
  );

  const handleKeyDown = useCallback(
    (e: KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        handleSubmit(e);
      }
    },
    [handleSubmit],
  );

  const handleTextareaChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setMessage(e.target.value);
    const el = e.target;
    el.style.height = 'auto';
    const maxHeight = LINE_HEIGHT_PX * MAX_VISIBLE_LINES;
    el.style.height = `${Math.min(el.scrollHeight, maxHeight)}px`;
  }, []);

  const isMultiline =
    message.includes('\n') ||
    (textareaRef.current?.scrollHeight ?? 0) > MULTILINE_SCROLL_THRESHOLD_PX;
  const hasContent = message.trim() || attachedImages.length > 0 || attachedFiles.length > 0;
  const placeholder = getPlaceholder(
    isModelBusy,
    loadingAction,
    disabled,
    isGeneratingElsewhere,
    disabledReason,
  );

  return (
    <form
      onSubmit={handleSubmit}
      onDragEnter={handleDragEnter}
      onDragLeave={handleDragLeave}
      onDragOver={handleDragOver}
      onDrop={handleDrop}
      data-testid="message-form"
      className="relative"
    >
      {isDragging ? <DragOverlay /> : null}
      <StatsBar
        timings={timings}
        tokensUsed={tokensUsed}
        maxTokens={maxTokens}
        streamStatus={streamStatus}
        disabled={disabled}
        isLoading={isLoading}
        stopGeneration={stopGeneration}
        estimatedConvTokens={estimatedConvTokens}
        modelContextSize={modelContextSize}
      />
      <ImagePreviews images={attachedImages} onRemove={removeImage} />
      <FilePreviews files={attachedFiles} onRemove={removeFile} />
      <ExtractingIndicator count={isExtracting} />
      <input
        ref={fileInputRef}
        type="file"
        multiple
        className="hidden"
        onChange={handleFileInputChange}
        accept={FILE_ACCEPT}
      />
      <div
        className={`flat-input-container flat-card flex items-end gap-2 px-5 py-2.5 ${isMultiline ? '!rounded-2xl' : ''}`}
      >
        <button
          type="button"
          onClick={handleFileButtonClick}
          className="flex-shrink-0 flex items-center py-1 opacity-40 hover:opacity-70 transition-opacity"
          title="Attach files"
          aria-label="Attach files"
        >
          <Paperclip className="h-4 w-4" />
        </button>
        <textarea
          ref={textareaRef}
          value={message}
          onChange={handleTextareaChange}
          onKeyDown={handleKeyDown}
          onPaste={handlePaste}
          placeholder={placeholder}
          disabled={disabled}
          className="flex-1 bg-transparent border-none outline-none resize-none text-sm text-foreground placeholder:text-muted-foreground min-h-[28px] py-1 overflow-y-auto"
          rows={1}
          data-testid="message-input"
          aria-disabled={disabled}
          aria-label={disabled && disabledReason ? disabledReason : 'Message input'}
        />
        <button
          type="submit"
          disabled={disabled || !hasContent || isExtracting > 0}
          className="flex-shrink-0 w-8 h-8 flex items-center justify-center rounded-full bg-foreground text-background hover:opacity-80 transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
          data-testid="send-button"
          aria-label="Send message"
        >
          <ArrowUp className="h-4 w-4" />
        </button>
      </div>
    </form>
  );
};
