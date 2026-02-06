import React from 'react';
import type { Message, ToolCall } from '../../types';
import { useMessageParsing, type ContentSegment } from '../../hooks/useMessageParsing';
import { MarkdownContent } from '../molecules/MarkdownContent';
import { ThinkingBlock, CommandExecBlock, ToolCallBlock } from '../molecules/messages';

interface MessageBubbleProps {
  message: Message;
  viewMode?: 'text' | 'markdown';
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
      className={`max-w-[90%] w-full p-4 rounded-lg ${
        isError
          ? 'bg-red-950 border-2 border-red-500'
          : 'bg-yellow-950 border-2 border-yellow-500'
      }`}
    >
      <div className="flex items-center gap-2 mb-2">
        <span className="text-sm font-bold text-white">
          {isError ? '❌ SYSTEM ERROR' : '⚠️ SYSTEM'}
        </span>
      </div>
      <pre
        className={`text-sm whitespace-pre-wrap leading-relaxed ${
          isError ? 'text-red-200' : 'text-yellow-200'
        }`}
      >
        {cleanContent}
      </pre>
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
      <pre className="text-sm whitespace-pre-wrap leading-relaxed text-muted-foreground mt-2">
        {cleanContent}
      </pre>
    </details>
  </div>
);

/**
 * User message component.
 */
const UserMessage: React.FC<{
  message: Message;
  cleanContent: string;
  viewMode: 'text' | 'markdown';
}> = ({ message, cleanContent, viewMode }) => (
  <div
    className="flex w-full justify-end"
    data-testid={`message-${message.role}`}
    data-message-id={message.id}
  >
    <div className="flat-message-user max-w-[80%] p-4">
      {viewMode === 'markdown' ? (
        <MarkdownContent content={cleanContent} testId="message-content" />
      ) : (
        <p className="text-sm whitespace-pre-wrap leading-relaxed" data-testid="message-content">
          {cleanContent}
        </p>
      )}
    </div>
  </div>
);

/**
 * Assistant message component with thinking, tool calls, and command blocks.
 * Renders content in original order to prevent visual jumps during streaming.
 */
const AssistantMessage: React.FC<{
  message: Message;
  viewMode: 'text' | 'markdown';
  toolCalls: ToolCall[];
  orderedSegments: ContentSegment[];
}> = ({ message, viewMode, toolCalls, orderedSegments }) => (
  <div
    className="w-full flex justify-start space-y-2"
    data-testid={`message-${message.role}`}
    data-message-id={message.id}
  >
    <div className="max-w-[80%] space-y-2">
      {/* Tool calls (legacy system) - show at top for backward compat */}
      <ToolCallBlock toolCalls={toolCalls} />

      {/* Render segments in original order */}
      {orderedSegments.map((segment, index) => {
        if (segment.type === 'thinking') {
          return <ThinkingBlock key={`thinking-${index}`} content={segment.content} />;
        } else if (segment.type === 'exec') {
          return (
            <CommandExecBlock
              key={`exec-${index}`}
              blocks={[{ command: segment.command, output: segment.output }]}
            />
          );
        } else if (segment.type === 'text') {
          return (
            <div key={`text-${index}`} className="flat-message-assistant p-4">
              {viewMode === 'markdown' ? (
                <MarkdownContent content={segment.content} testId="message-content" />
              ) : (
                <p className="text-sm whitespace-pre-wrap leading-relaxed" data-testid="message-content">
                  {segment.content}
                </p>
              )}
            </div>
          );
        }
        return null;
      })}
    </div>
  </div>
);

/**
 * Message bubble component - renders user, assistant, or system messages.
 */
export const MessageBubble: React.FC<MessageBubbleProps> = ({ message, viewMode = 'text' }) => {
  const {
    toolCalls,
    cleanContent,
    isError,
    orderedSegments,
  } = useMessageParsing(message);

  // System messages
  if (message.role === 'system') {
    if (message.isSystemPrompt) {
      return <SystemPromptMessage message={message} cleanContent={cleanContent} />;
    }
    return <SystemMessage message={message} cleanContent={cleanContent} isError={isError} />;
  }

  // User messages
  if (message.role === 'user') {
    return <UserMessage message={message} cleanContent={cleanContent} viewMode={viewMode} />;
  }

  // Assistant messages
  return (
    <AssistantMessage
      message={message}
      viewMode={viewMode}
      toolCalls={toolCalls}
      orderedSegments={orderedSegments}
    />
  );
};
