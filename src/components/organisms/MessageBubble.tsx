import { Pencil, RefreshCw, Play, Loader2, ChevronDown, ChevronRight } from 'lucide-react';
import React, { useState, useRef, useEffect } from 'react';
import { useTranslation } from 'react-i18next';

import { useModelContext } from '../../contexts/ModelContext';
import type { MessageSegment } from '../../hooks/useMessageParsing';
import { useMessageParsing } from '../../hooks/useMessageParsing';
import type { Message, ToolCall } from '../../types';
import { LoadingIndicator } from '../atoms';
import { MarkdownContent } from '../molecules/MarkdownContent';
import { CompactionSummary, ThinkingBlock, ToolCallBlock } from '../molecules/messages';
import { StreamingText } from '../molecules/messages/StreamingText';

const MIN_VALID_TIMESTAMP_MS = 1_000_000_000_000;

const TOOL_GROUP_THRESHOLD = 3;

type RenderedSegment = MessageSegment | { type: 'tool_call_group'; toolCalls: ToolCall[] };

function groupConsecutiveToolCalls(segments: MessageSegment[]): RenderedSegment[] {
  const result: RenderedSegment[] = [];
  let i = 0;
  while (i < segments.length) {
    const seg = segments[i];
    if (seg.type === 'tool_call') {
      const run: ToolCall[] = [seg.toolCall];
      let j = i + 1;
      while (j < segments.length && segments[j].type === 'tool_call') {
        run.push((segments[j] as { type: 'tool_call'; toolCall: ToolCall }).toolCall);
        j++;
      }
      if (run.length >= TOOL_GROUP_THRESHOLD) {
        result.push({ type: 'tool_call_group', toolCalls: run });
      } else {
        for (const tc of run) result.push({ type: 'tool_call', toolCall: tc });
      }
      i = j;
    } else {
      result.push(seg);
      i++;
    }
  }
  return result;
}

const ToolCallGroup: React.FC<{ toolCalls: ToolCall[]; isGenerating?: boolean }> = ({
  toolCalls,
  isGenerating,
}) => {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);

  const counts = toolCalls.reduce<Record<string, number>>((acc, tc) => {
    acc[tc.name] = (acc[tc.name] ?? 0) + 1;
    return acc;
  }, {});

  const summary = Object.entries(counts)
    .map(([name, n]) => {
      const label = name
        .split('_')
        .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
        .join(' ');
      return n > 1 ? `${label} x${n}` : label;
    })
    .join(', ');

  const Chevron = expanded ? ChevronDown : ChevronRight;
  const expandedPanel = expanded ? (
    <div className="border-t border-border/30 px-3 py-2">
      <ToolCallBlock toolCalls={toolCalls} isGenerating={isGenerating} />
    </div>
  ) : null;

  return (
    <div className="rounded border border-border/50 bg-muted/30">
      <button
        type="button"
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-2 px-3 py-2 text-left text-xs text-foreground/70 hover:text-foreground/90"
      >
        <Chevron className="size-3 flex-shrink-0" />
        <span className="font-medium">
          {t('messageBubble.toolCallsCount', { count: toolCalls.length })}
        </span>
        <span className="text-foreground/40">: {summary}</span>
      </button>
      {expandedPanel}
    </div>
  );
};

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

const SystemMessage: React.FC<{ message: Message; cleanContent: string; isError: boolean }> = ({
  message,
  cleanContent,
  isError,
}) => {
  const label = isError ? 'SYSTEM ERROR' : 'SYSTEM';
  return (
    <div
      className="flex w-full justify-center"
      data-testid={`message-${message.role}`}
      data-message-id={message.id}
    >
      <div
        className={`w-full max-w-[90%] rounded-lg px-4 py-2 ${
          isError
            ? 'border-2 border-red-500 bg-red-950'
            : 'border-2 border-yellow-500 bg-yellow-950'
        }`}
      >
        <div className="flex items-center gap-2">
          <span className="text-sm font-bold text-white">{label}</span>
          <span className={`text-sm ${isError ? 'text-red-200' : 'text-yellow-200'}`}>
            {cleanContent}
          </span>
        </div>
      </div>
    </div>
  );
};

const SystemPromptMessage: React.FC<{ message: Message; cleanContent: string }> = ({
  message,
  cleanContent,
}) => {
  const { t } = useTranslation();
  return (
    <div
      className="flex w-full justify-center"
      data-testid={`message-${message.role}`}
      data-message-id={message.id}
    >
      <details className="w-full max-w-[90%] rounded-lg border border-border bg-muted p-3">
        <summary className="cursor-pointer select-none text-sm font-semibold">
          {t('messageBubble.systemPrompt')}
        </summary>
        <pre className="mt-2 whitespace-pre-wrap text-sm leading-relaxed text-foreground">
          {cleanContent}
        </pre>
      </details>
    </div>
  );
};

const UserMessage: React.FC<{
  message: Message;
  cleanContent: string;
  viewMode: 'text' | 'markdown' | 'raw';
  messageIndex?: number;
  onEditMessage?: (messageIndex: number, newContent: string) => void;
  isGenerating?: boolean;
}> = ({ message, cleanContent, viewMode, messageIndex, onEditMessage, isGenerating }) => {
  const { t } = useTranslation();
  const { status, activeProvider } = useModelContext();
  const modelReady = status.loaded || activeProvider !== 'local';
  // react-doctor-disable-next-line react-doctor/rerender-state-only-in-handlers -- false positive: never read in return because if-guard returns alternate view
  const [isEditing, setIsEditing] = useState(false); // eslint-disable-line react/hook-use-state
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
    window.dispatchEvent(new CustomEvent('edit-message-submitted'));
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

  const renderImage = (img: string, i: number) => {
    // react-doctor-disable-next-line react-doctor/no-array-index-as-key -- base64 images have no stable ID
    return <img key={i} src={img} alt={`Attached ${i + 1}`} className="max-h-64 max-w-full rounded-lg object-contain" />;
  };
  const imageElements = message.image_data?.length ? (
    <div className="mb-2 flex flex-wrap gap-2">
      {message.image_data.map((img, i) => renderImage(img, i))}
    </div>
  ) : null;

  if (isEditing) {
    return (
      <div
        className="flex w-full justify-end"
        data-testid={`message-${message.role}`}
        data-message-id={message.id}
      >
        <div className="w-full max-w-[85%] space-y-2">
          <textarea
            ref={textareaRef}
            value={editContent}
            onChange={(e) => setEditContent(e.target.value)}
            onKeyDown={handleKeyDown}
            className="w-full resize-none rounded-2xl border border-border bg-muted px-4 py-3 text-sm text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
            rows={Math.min(editContent.split('\n').length + 1, 10)}
          />
          <div className="flex justify-end gap-2">
            <button
              onClick={handleCancel}
              className="rounded-lg px-3 py-1 text-xs text-foreground/70 transition-colors hover:bg-muted hover:text-foreground"
            >
              {t('common.cancel')}
            </button>
            <button
              onClick={handleSubmit}
              disabled={!editContent.trim()}
              className="rounded-lg bg-primary px-3 py-1 text-xs text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-50"
            >
              {t('common.submit')}
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div
      className="group flex w-full items-start justify-end gap-1"
      data-testid={`message-${message.role}`}
      data-message-id={message.id}
    >
      {!!canEdit && (
        <button
          onClick={handleStartEdit}
          className="mt-3 rounded p-1 text-foreground/30 opacity-0 transition-all hover:text-foreground/70 group-hover:opacity-100"
          aria-label="Edit message"
          data-testid="edit-message-btn"
        >
          <Pencil size={14} />
        </button>
      )}
      <div className="min-w-0 max-w-[85%]">
        <div className="flat-message-user px-4 py-3">
          {imageElements}
          {(() => {
            if (viewMode === 'raw') {
              return (
                <pre
                  className="whitespace-pre-wrap font-mono text-xs leading-relaxed"
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
                className="whitespace-pre-wrap text-sm leading-relaxed [overflow-wrap:anywhere]"
                data-testid="message-content"
              >
                {cleanContent}
              </p>
            );
          })()}
        </div>
        {!!formatTimestamp(message.timestamp) && (
          <div className="mt-0.5 pr-1 text-right text-[10px] text-white/50">
            {formatTimestamp(message.timestamp)}
          </div>
        )}
      </div>
    </div>
  );
};

// react-doctor-disable-next-line react-doctor/no-many-boolean-props -- compound refactor would reduce readability for this component
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
  const { t } = useTranslation();
  const { status, activeProvider } = useModelContext();
  const modelReady = status.loaded || activeProvider !== 'local';
  const ariaLive = isStreaming ? ('polite' as const) : undefined;
  const thinkingStreamingProp = isThinkingStreaming ? isStreaming : undefined;
  const renderSegmentRow = (segment: RenderedSegment, index: number): React.ReactNode => {
    if (segment.type === 'tool_call_group') {
      // react-doctor-disable-next-line react-doctor/no-array-index-as-key -- positional segments
      return <ToolCallGroup key={`seg-tcg-${index}`} toolCalls={segment.toolCalls} isGenerating={isGenerating} />;
    }
    if (segment.type === 'thinking') {
      const isLastSeg = index === segments.length - 1;
      const streamProp = isLastSeg && isThinkingStreaming ? isStreaming : undefined;
      // react-doctor-disable-next-line react-doctor/no-array-index-as-key -- positional segments
      return <ThinkingBlock key={`seg-think-${index}`} content={segment.content} isStreaming={streamProp} />;
    }
    if (segment.type === 'tool_call') {
      // react-doctor-disable-next-line react-doctor/no-array-index-as-key -- positional segments
      return <ToolCallBlock key={`seg-tc-${index}`} toolCalls={[segment.toolCall]} isGenerating={isGenerating} />;
    }
    if (segment.type === 'tool_call_pending') {
      if (!isStreaming) return null;
      // react-doctor-disable-next-line react-doctor/no-array-index-as-key -- positional segments
      return <div key={`seg-tcp-${index}`} className="flex w-full items-center gap-2 rounded bg-muted px-3 py-2 text-xs text-foreground/70"><Loader2 className="size-3 flex-shrink-0 animate-spin" /><span className="font-medium">{segment.name ?? 'tool call'}</span><span className="text-foreground/40">{t('messageBubble.writingArguments')}</span></div>;
    }
    const text = segment.content;
    if (!text.trim()) return null;
    const isLastTextSeg = index === segments.length - 1;
    const segStreaming = isLastTextSeg ? isStreaming : undefined;
    if (viewMode === 'markdown') {
      // react-doctor-disable-next-line react-doctor/no-array-index-as-key -- positional segments
      return <div key={`seg-txt-${index}`}><MarkdownContent content={text} testId="message-content" /></div>;
    }
    // react-doctor-disable-next-line react-doctor/no-array-index-as-key -- positional segments
    return <div key={`seg-txt-${index}`}><StreamingText content={text} isStreaming={segStreaming} /></div>;
  };
  const renderedSegments = (isStreaming ? segments : groupConsecutiveToolCalls(segments)).map(
    (segment, index) => renderSegmentRow(segment, index),
  );
  return (
    <div
      className="flex w-full justify-start"
      data-testid={`message-${message.role}`}
      data-message-id={message.id}
      aria-live={ariaLive}
    >
      <div className="w-full space-y-3 overflow-hidden">
        {viewMode === 'raw' && (
          <pre
            className="whitespace-pre-wrap font-mono text-xs leading-relaxed"
            data-testid="message-content"
          >
            {message.content}
          </pre>
        )}
        {viewMode !== 'raw' && (
          <>
            {thinkingContent != null && !segments.some((s) => s.type === 'thinking') && (
              <ThinkingBlock content={thinkingContent} isStreaming={thinkingStreamingProp} />
            )}
            {renderedSegments}
            {!!isStreaming && <LoadingIndicator />}
          </>
        )}
        {!isStreaming && !!formatTimestamp(message.timestamp) && (
          <div className="mt-0.5 flex items-center gap-2 pl-1">
            <span className="text-[10px] text-white/50">{formatTimestamp(message.timestamp)}</span>
            {!!isLastAssistant && !isGenerating && !!modelReady && !!onContinue && (
              <button
                onClick={onContinue}
                className="p-0.5 text-white/30 transition-colors hover:text-white/70"
                title="Continue generation"
              >
                <Play className="size-3" />
              </button>
            )}
            {!!isLastAssistant && !isGenerating && !!modelReady && !!onRegenerate && (
              <button
                onClick={onRegenerate}
                className="p-0.5 text-white/30 transition-colors hover:text-white/70"
                title="Regenerate response"
              >
                <RefreshCw className="size-3" />
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
};

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

    const displayContent = viewMode === 'raw' ? message.content : cleanContent;
    if (message.role === 'error') {
      return <SystemMessage message={message} cleanContent={displayContent} isError />;
    }
    if (message.role === 'system') {
      if (message.isSystemPrompt) {
        return <SystemPromptMessage message={message} cleanContent={displayContent} />;
      }
      if (message.content.startsWith('[Conversation summary')) {
        return <CompactionSummary message={message} cleanContent={displayContent} />;
      }
      return <SystemMessage message={message} cleanContent={displayContent} isError={isError} />;
    }

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

    const onContinueProp =
      onContinue && messageIndex != null ? () => onContinue(messageIndex) : undefined;
    const onRegenerateProp =
      onRegenerate && messageIndex != null ? () => onRegenerate(messageIndex) : undefined;
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
        onContinue={onContinueProp}
        onRegenerate={onRegenerateProp}
      />
    );
  },
);
MessageBubble.displayName = 'MessageBubble';
