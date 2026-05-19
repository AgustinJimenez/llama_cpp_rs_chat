import { Pencil, RefreshCw, ChevronDown, ChevronRight, Archive, Play, Trash2 } from 'lucide-react';
import React, { useState, useRef, useEffect, useCallback } from 'react';

import { useChatContext } from '../../contexts/ChatContext';
import { useModelContext } from '../../contexts/ModelContext';
import type { MessageSegment } from '../../hooks/useMessageParsing';
import { useMessageParsing } from '../../hooks/useMessageParsing';
import type { Message } from '../../types';
import { LoadingIndicator } from '../atoms';
import { MarkdownContent } from '../molecules/MarkdownContent';
import { ThinkingBlock, ToolCallBlock } from '../molecules/messages';
import { updateConversationSummary, deleteConversationSummary } from '../../utils/tauriCommands';

const MIN_VALID_TIMESTAMP_MS = 1_000_000_000_000;

/** Format a message timestamp for display. Returns null for fake counter values. */
function formatTimestamp(ts: number): string | null {
  if (ts < MIN_VALID_TIMESTAMP_MS) return null; // fake counter or missing
  const date = new Date(ts);
  const now = new Date();
  const isToday = date.toDateString() === now.toDateString();
  const time = date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false });
  if (isToday) return time;
  const day = date.toLocaleDateString([], { month: 'short', day: 'numeric' });
  return `${day}, ${time}`;
}

interface MessageBubbleProps {
  message: Message;
  viewMode?: 'text' | 'markdown' | 'raw';
  isStreaming?: boolean;
  messageIndex?: number;
  onEditMessage?: (messageIndex: number, newContent: string) => void;
  onRegenerate?: (messageIndex: number) => void;
  onContinue?: (messageIndex: number) => void;
  isGenerating?: boolean;
  isLastMessage?: boolean;
}

/**
 * Compaction summary divider — shows when older messages have been summarized.
 */
const CompactionSummary: React.FC<{ message: Message; cleanContent: string }> = ({
  message,
  cleanContent,
}) => {
  const { currentConversationId, loadConversation } = useChatContext();
  const { status, activeProvider } = useModelContext();
  const modelReady = status.loaded || activeProvider !== 'local';
  const [expanded, setExpanded] = useState(false);
  const [isEditing, setIsEditing] = useState(false);
  const [editText, setEditText] = useState('');
  const [isSaving, setIsSaving] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Extract the summary text (after the header line)
  const lines = cleanContent.split('\n');
  const summaryBody = lines.slice(1).join('\n').trim();

  const handleStartEdit = useCallback(() => {
    setEditText(summaryBody);
    setIsEditing(true);
    setExpanded(true);
  }, [summaryBody]);

  useEffect(() => {
    if (isEditing && textareaRef.current) {
      textareaRef.current.focus();
      textareaRef.current.setSelectionRange(editText.length, editText.length);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isEditing]);

  const handleSave = useCallback(async () => {
    if (!currentConversationId || !editText.trim()) return;
    setIsSaving(true);
    try {
      await updateConversationSummary(currentConversationId, editText.trim());
      window.dispatchEvent(new CustomEvent('conversation-compacted'));
      setIsEditing(false);
    } catch { /* ignore */ } finally {
      setIsSaving(false);
    }
  }, [currentConversationId, editText]);

  const handleDelete = useCallback(async () => {
    if (!currentConversationId) return;
    try {
      await deleteConversationSummary(currentConversationId);
      window.dispatchEvent(new CustomEvent('conversation-compacted'));
    } catch { /* ignore */ }
  }, [currentConversationId]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleSave(); }
    else if (e.key === 'Escape') setIsEditing(false);
  };

  return (
    <div
      className="w-full flex justify-center my-2"
      data-testid="compaction-summary"
      data-message-id={message.id}
    >
      <div className="max-w-[90%] w-full">
        <div className="group w-full flex items-center gap-2 px-3 py-1.5 text-xs text-white/70 border-t border-b border-white/10 hover:border-white/20">
          <Archive className="h-3 w-3 shrink-0" />
          <button
            onClick={() => setExpanded(!expanded)}
            className="flex-1 text-left hover:text-white transition-colors truncate"
          >
            Earlier messages summarized
          </button>
          {modelReady ? (
            <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
              <button
                onClick={handleStartEdit}
                className="p-1 rounded hover:text-white/70 transition-colors"
                title="Edit summary"
              >
                <Pencil className="h-3 w-3" />
              </button>
              <button
                onClick={handleDelete}
                className="p-1 rounded hover:text-red-400 transition-colors"
                title="Delete summary (reverts compaction)"
              >
                <Trash2 className="h-3 w-3" />
              </button>
            </div>
          ) : null}
          <button onClick={() => setExpanded(!expanded)} className="hover:text-white transition-colors">
            {expanded ? (
              <ChevronDown className="h-3 w-3 shrink-0" />
            ) : (
              <ChevronRight className="h-3 w-3 shrink-0" />
            )}
          </button>
        </div>
        {expanded ? (
          <div className="bg-white/5 border-b border-white/10">
            {isEditing ? (
              <div className="p-2 space-y-2">
                <textarea
                  ref={textareaRef}
                  value={editText}
                  onChange={(e) => setEditText(e.target.value)}
                  onKeyDown={handleKeyDown}
                  className="w-full px-2 py-1.5 text-xs bg-black/20 border border-white/20 rounded text-white/70 resize-none focus:outline-none focus:ring-1 focus:ring-primary"
                  rows={Math.min(editText.split('\n').length + 1, 20)}
                />
                <div className="flex justify-end gap-2">
                  <button
                    onClick={() => setIsEditing(false)}
                    className="px-2 py-1 text-xs rounded text-white/50 hover:text-white/70 hover:bg-white/10 transition-colors"
                  >
                    Cancel
                  </button>
                  <button
                    onClick={handleSave}
                    disabled={isSaving || !editText.trim() || !modelReady}
                    className="px-2 py-1 text-xs rounded bg-primary text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
                  >
                    {isSaving ? 'Saving…' : 'Save'}
                  </button>
                </div>
              </div>
            ) : summaryBody ? (
              <div className="px-3 py-2 text-xs text-white/80 whitespace-pre-wrap">
                {summaryBody}
              </div>
            ) : null}
          </div>
        ) : null}
      </div>
    </div>
  );
};

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
        isError ? 'bg-red-950 border-2 border-red-500' : 'bg-yellow-950 border-2 border-yellow-500'
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
      <summary className="text-sm font-semibold cursor-pointer select-none">System prompt</summary>
      <pre className="text-sm whitespace-pre-wrap leading-relaxed text-foreground mt-2">
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
  const { status, activeProvider } = useModelContext();
  const modelReady = status.loaded || activeProvider !== 'local';
  const [isEditing, setIsEditing] = useState(false);
  const [editContent, setEditContent] = useState('');
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (isEditing && textareaRef.current) {
      textareaRef.current.focus();
      textareaRef.current.setSelectionRange(editContent.length, editContent.length);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only re-run when editing state changes, not on every keystroke
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

  const canEdit = onEditMessage && messageIndex != null && !isGenerating && modelReady;

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
              Submit
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
      <div className="max-w-[85%]">
        <div className="flat-message-user px-4 py-3">
          {/* Attached images */}
          {message.image_data && message.image_data.length > 0 ? (
            <div className="mb-2 flex flex-wrap gap-2">
              {/* eslint-disable react/no-array-index-key -- base64 images have no stable ID */}
              {message.image_data.map((img, i) => (
                <img
                  key={i}
                  src={img}
                  alt={`Attached ${i + 1}`}
                  className="max-h-64 max-w-full rounded-lg object-contain"
                />
              ))}
              {/* eslint-enable react/no-array-index-key */}
            </div>
          ) : null}
          {(() => {
            if (viewMode === 'raw') {
              return (
                <pre
                  className="text-xs whitespace-pre-wrap leading-relaxed font-mono"
                  data-testid="message-content"
                >
                  {message.content}
                </pre>
              );
            }
            if (viewMode === 'markdown') {
              return <MarkdownContent content={cleanContent} testId="message-content" />;
            }
            return (
              <p
                className="text-sm whitespace-pre-wrap leading-relaxed"
                data-testid="message-content"
              >
                {cleanContent}
              </p>
            );
          })()}
        </div>
        {formatTimestamp(message.timestamp) ? (
          <div className="text-[10px] text-white/50 mt-0.5 text-right pr-1">
            {formatTimestamp(message.timestamp)}
          </div>
        ) : null}
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
  isLastAssistant?: boolean;
  onRegenerate?: () => void;
  onContinue?: () => void;
}> = ({
  message,
  viewMode,
  thinkingContent,
  isThinkingStreaming,
  segments,
  isStreaming,
  isGenerating,
  isLastAssistant,
  onRegenerate,
  onContinue,
}) => {
  const { status, activeProvider } = useModelContext();
  const modelReady = status.loaded || activeProvider !== 'local';
  return (
    <div
      className="w-full flex justify-start"
      data-testid={`message-${message.role}`}
      data-message-id={message.id}
      aria-live={isStreaming ? 'polite' : undefined}
    >
      <div className="w-full space-y-3 overflow-hidden">
        {viewMode === 'raw' ? (
          /* Raw mode: show unprocessed content with no parsing */
          <pre
            className="text-xs whitespace-pre-wrap leading-relaxed font-mono"
            data-testid="message-content"
          >
            {message.content}
          </pre>
        ) : (
          <>
            {/* Thinking process — only when not already placed chronologically in segments */}
            {thinkingContent != null && !segments.some((s) => s.type === 'thinking') ? (
              <ThinkingBlock
                content={thinkingContent}
                isStreaming={isThinkingStreaming ? isStreaming : undefined}
              />
            ) : null}

            {/* Interleaved text, command blocks, tool calls, and thinking in chronological order */}
            {/* eslint-disable react/no-array-index-key -- segments are positional with no stable ID */}
            {segments.map((segment, index) => {
              if (segment.type === 'thinking') {
                const isLastSeg = index === segments.length - 1;
                return (
                  <ThinkingBlock
                    key={`seg-think-${index}`}
                    content={segment.content}
                    isStreaming={isLastSeg && isThinkingStreaming ? isStreaming : undefined}
                  />
                );
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
                    <p
                      className="text-sm whitespace-pre-wrap leading-relaxed"
                      data-testid="message-content"
                    >
                      {text}
                    </p>
                  )}
                </div>
              );
            })}
            {/* eslint-enable react/no-array-index-key */}
            {isStreaming ? <LoadingIndicator /> : null}
          </>
        )}
        {!isStreaming && formatTimestamp(message.timestamp) ? (
          <div className="flex items-center gap-2 mt-0.5 pl-1">
            <span className="text-[10px] text-white/50">{formatTimestamp(message.timestamp)}</span>
            {isLastAssistant && !isGenerating && modelReady && onContinue ? (
              <button
                onClick={onContinue}
                className="text-white/30 hover:text-white/70 transition-colors p-0.5"
                title="Continue generation"
              >
                <Play className="h-3 w-3" />
              </button>
            ) : null}
            {isLastAssistant && !isGenerating && modelReady && onRegenerate ? (
              <button
                onClick={onRegenerate}
                className="text-white/30 hover:text-white/70 transition-colors p-0.5"
                title="Regenerate response"
              >
                <RefreshCw className="h-3 w-3" />
              </button>
            ) : null}
          </div>
        ) : null}
      </div>
    </div>
  );
};

/**
 * Message bubble component - renders user, assistant, or system messages.
 */
export const MessageBubble: React.FC<MessageBubbleProps> = React.memo(
  ({
    message,
    viewMode = 'text',
    isStreaming,
    messageIndex,
    onEditMessage,
    onRegenerate,
    onContinue,
    isGenerating,
    isLastMessage,
  }) => {
    const { status } = useModelContext();
    const { cleanContent, thinkingContent, isThinkingStreaming, segments, isError } =
      useMessageParsing(message, status.tool_tags);

    // System messages
    const displayContent = viewMode === 'raw' ? message.content : cleanContent;
    if (message.role === 'system') {
      if (message.isSystemPrompt) {
        return <SystemPromptMessage message={message} cleanContent={displayContent} />;
      }
      // Compaction summary messages
      if (message.content.startsWith('[Conversation summary')) {
        return <CompactionSummary message={message} cleanContent={displayContent} />;
      }
      return <SystemMessage message={message} cleanContent={displayContent} isError={isError} />;
    }

    // User messages
    if (message.role === 'user') {
      return (
        <UserMessage
          message={message}
          cleanContent={cleanContent}
          viewMode={viewMode}
          messageIndex={messageIndex}
          onEditMessage={onEditMessage}
          isGenerating={isGenerating}
        />
      );
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
        isLastAssistant={isLastMessage}
        onContinue={onContinue && messageIndex != null ? () => onContinue(messageIndex) : undefined}
        onRegenerate={
          onRegenerate && messageIndex != null ? () => onRegenerate(messageIndex) : undefined
        }
      />
    );
  },
);
MessageBubble.displayName = 'MessageBubble';
