import React, { useState, useRef, useEffect } from 'react';
import { Pencil } from 'lucide-react';
import type { Message } from '../../types';
import type { MessageSegment } from '../../hooks/useMessageParsing';
import { useMessageParsing } from '../../hooks/useMessageParsing';
import { useModelContext } from '../../contexts/ModelContext';
import { MarkdownContent } from '../molecules/MarkdownContent';
import { ThinkingBlock, ToolCallBlock } from '../molecules/messages';

interface MessageBubbleProps {
  message: Message;
  viewMode?: 'text' | 'markdown' | 'raw';
  isStreaming?: boolean;
  messageIndex?: number;
  onEditMessage?: (messageIndex: number, newContent: string) => void;
  isGenerating?: boolean;
}

/**
 * System message component (errors and warnings).
 */
const SystemMessage: React.FC<{ message: Message; cleanContent: string; isError: boolean }> = ({
  message,
  cleanContent,
  isError,
}) => (
  <div
    className="w-full flex justify-center"
    data-testid={`message-${message.role}`}
    data-message-id={message.id}
  >
    <div
      className={`max-w-[90%] w-full px-4 py-2 rounded-lg ${
        isError
          ? 'bg-red-950 border-2 border-red-500'
          : 'bg-yellow-950 border-2 border-yellow-500'
      }`}
    >
      <div className="flex items-center gap-2">
        <span className="text-sm font-bold text-white">
          {isError ? '❌ SYSTEM ERROR' : '⚠️ SYSTEM'}
        </span>
        <span className={`text-sm ${isError ? 'text-red-200' : 'text-yellow-200'}`}>
          {cleanContent}
        </span>
      </div>
    </div>
  </div>
);

/**
 * Collapsed system prompt display.
 */
const SystemPromptMessage: React.FC<{ message: Message; cleanContent: string }> = ({
  message,
  cleanContent,
}) => (
  <div
    className="w-full flex justify-center"
    data-testid={`message-${message.role}`}
    data-message-id={message.id}
  >
    <details className="max-w-[90%] w-full bg-muted border border-border rounded-lg p-3">
      <summary className="text-sm font-semibold cursor-pointer select-none">
        System prompt
      </summary>
      <pre className="text-sm whitespace-pre-wrap leading-relaxed text-white mt-2">
        {cleanContent}
      </pre>
    </details>
  </div>
);

/**
 * User message component with inline edit support.
 */
const UserMessage: React.FC<{
  message: Message;
  cleanContent: string;
  viewMode: 'text' | 'markdown' | 'raw';
  messageIndex?: number;
  onEditMessage?: (messageIndex: number, newContent: string) => void;
  isGenerating?: boolean;
}> = ({ message, cleanContent, viewMode, messageIndex, onEditMessage, isGenerating }) => {
  const [isEditing, setIsEditing] = useState(false);
  const [editContent, setEditContent] = useState('');
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (isEditing && textareaRef.current) {
      textareaRef.current.focus();
      textareaRef.current.setSelectionRange(editContent.length, editContent.length);
    }
  }, [isEditing]);

  const handleStartEdit = () => {
    setEditContent(message.content);
    setIsEditing(true);
  };

  const handleSubmit = () => {
    const trimmed = editContent.trim();
    if (!trimmed || messageIndex == null || !onEditMessage) return;
    setIsEditing(false);
    onEditMessage(messageIndex, trimmed);
  };

  const handleCancel = () => {
    setIsEditing(false);
    setEditContent('');
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    } else if (e.key === 'Escape') {
      handleCancel();
    }
  };

  const canEdit = onEditMessage && messageIndex != null && !isGenerating;

  if (isEditing) {
    return (
      <div
        className="flex w-full justify-end"
        data-testid={`message-${message.role}`}
        data-message-id={message.id}
      >
        <div className="max-w-[85%] w-full space-y-2">
          <textarea
            ref={textareaRef}
            value={editContent}
            onChange={(e) => setEditContent(e.target.value)}
            onKeyDown={handleKeyDown}
            className="w-full px-4 py-3 rounded-2xl bg-muted border border-border text-sm text-foreground resize-none focus:outline-none focus:ring-1 focus:ring-primary"
            rows={Math.min(editContent.split('\n').length + 1, 10)}
          />
          <div className="flex justify-end gap-2">
            <button
              onClick={handleCancel}
              className="px-3 py-1 text-xs rounded-lg text-foreground/70 hover:text-foreground hover:bg-muted transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={handleSubmit}
              disabled={!editContent.trim()}
              className="px-3 py-1 text-xs rounded-lg bg-primary text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
            >
              Save & Submit
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div
      className="group flex w-full justify-end items-start gap-1"
      data-testid={`message-${message.role}`}
      data-message-id={message.id}
    >
      {/* Edit button — appears on hover */}
      {canEdit ? (
        <button
          onClick={handleStartEdit}
          className="opacity-0 group-hover:opacity-100 mt-3 p-1 rounded text-foreground/30 hover:text-foreground/70 transition-all"
          aria-label="Edit message"
          data-testid="edit-message-btn"
        >
          <Pencil size={14} />
        </button>
      ) : null}
      <div className="flat-message-user max-w-[85%] px-4 py-3">
        {/* Attached images */}
        {message.image_data && message.image_data.length > 0 ? (
          <div className="mb-2 flex flex-wrap gap-2">
            {message.image_data.map((img, i) => (
              <img
                key={i}
                src={img}
                alt={`Attached ${i + 1}`}
                className="max-h-64 max-w-full rounded-lg object-contain"
              />
            ))}
          </div>
        ) : null}
        {viewMode === 'raw' ? (
          <pre className="text-xs whitespace-pre-wrap leading-relaxed font-mono" data-testid="message-content">
            {message.content}
          </pre>
        ) : viewMode === 'markdown' ? (
          <MarkdownContent content={cleanContent} testId="message-content" />
        ) : (
          <p className="text-sm whitespace-pre-wrap leading-relaxed" data-testid="message-content">
            {cleanContent}
          </p>
        )}
      </div>
    </div>
  );
};


/**
 * Assistant message component with thinking and tool calls
 * rendered in chronological order.
 */
const AssistantMessage: React.FC<{
  message: Message;
  viewMode: 'text' | 'markdown' | 'raw';
  thinkingContent: string | null;
  isThinkingStreaming?: boolean;
  segments: MessageSegment[];
  isStreaming?: boolean;
  isGenerating?: boolean;
}> = ({
  message,
  viewMode,
  thinkingContent,
  isThinkingStreaming,
  segments,
  isStreaming,
  isGenerating,
}) => (
  <div
    className="w-full flex justify-start"
    data-testid={`message-${message.role}`}
    data-message-id={message.id}
    aria-live={isStreaming ? 'polite' : undefined}
  >
    <div className="w-full space-y-3 overflow-hidden">
      {viewMode === 'raw' ? (
        /* Raw mode: show unprocessed content with no parsing */
        <pre className="text-xs whitespace-pre-wrap leading-relaxed font-mono" data-testid="message-content">
          {message.content}
        </pre>
      ) : (
        <>
          {/* Thinking process (for reasoning models) */}
          {thinkingContent ? <ThinkingBlock content={thinkingContent} isStreaming={isThinkingStreaming && isStreaming} /> : null}

          {/* Interleaved text, command blocks, tool calls, and thinking in chronological order */}
          {segments.map((segment, index) => {
            if (segment.type === 'thinking') {
              return <ThinkingBlock key={`seg-think-${index}`} content={segment.content} />;
            }
            if (segment.type === 'tool_call') {
              return (
                <ToolCallBlock
                  key={`seg-tc-${index}`}
                  toolCalls={[segment.toolCall]}
                  isGenerating={isGenerating}
                />
              );
            }
            // Text segment — no bubble, just text on background
            const text = segment.content;
            if (!text.trim()) return null;
            return (
              <div key={`seg-txt-${index}`}>
                {viewMode === 'markdown' ? (
                  <MarkdownContent content={text} testId="message-content" />
                ) : (
                  <p className="text-sm whitespace-pre-wrap leading-relaxed" data-testid="message-content">
                    {text}
                  </p>
                )}
              </div>
            );
          })}


        </>
      )}

    </div>
  </div>
);

/**
 * Message bubble component - renders user, assistant, or system messages.
 */
export const MessageBubble: React.FC<MessageBubbleProps> = React.memo(({ message, viewMode = 'text', isStreaming, messageIndex, onEditMessage, isGenerating }) => {
  const { status } = useModelContext();
  const {
    cleanContent,
    thinkingContent,
    isThinkingStreaming,
    segments,
    isError,
  } = useMessageParsing(message, status.tool_tags);

  // System messages
  const displayContent = viewMode === 'raw' ? message.content : cleanContent;
  if (message.role === 'system') {
    if (message.isSystemPrompt) {
      return <SystemPromptMessage message={message} cleanContent={displayContent} />;
    }
    return <SystemMessage message={message} cleanContent={displayContent} isError={isError} />;
  }

  // User messages
  if (message.role === 'user') {
    return <UserMessage message={message} cleanContent={cleanContent} viewMode={viewMode} messageIndex={messageIndex} onEditMessage={onEditMessage} isGenerating={isGenerating} />;
  }

  // Assistant messages
  return (
    <AssistantMessage
      message={message}
      viewMode={viewMode}
      thinkingContent={thinkingContent}
      isThinkingStreaming={isThinkingStreaming}
      segments={segments}
      isStreaming={isStreaming}
      isGenerating={isGenerating}
    />
  );
});
MessageBubble.displayName = 'MessageBubble';
