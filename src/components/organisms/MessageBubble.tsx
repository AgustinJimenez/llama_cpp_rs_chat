import React from 'react';
import type { Message } from '../../types';
import type { MessageSegment } from '../../hooks/useMessageParsing';
import { useMessageParsing } from '../../hooks/useMessageParsing';
import { MarkdownContent } from '../molecules/MarkdownContent';
import { ThinkingBlock, CommandExecBlock, ToolCallBlock } from '../molecules/messages';

interface MessageBubbleProps {
  message: Message;
  viewMode?: 'text' | 'markdown' | 'raw';
  isStreaming?: boolean;
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
 * User message component.
 */
const UserMessage: React.FC<{
  message: Message;
  cleanContent: string;
  viewMode: 'text' | 'markdown' | 'raw';
}> = ({ message, cleanContent, viewMode }) => (
  <div
    className="flex w-full justify-end"
    data-testid={`message-${message.role}`}
    data-message-id={message.id}
  >
    <div className="flat-message-user max-w-[85%] px-4 py-3">
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


/**
 * Assistant message component with thinking, tool calls, and command blocks
 * rendered in chronological order.
 */
const AssistantMessage: React.FC<{
  message: Message;
  viewMode: 'text' | 'markdown' | 'raw';
  thinkingContent: string | null;
  isThinkingStreaming?: boolean;
  segments: MessageSegment[];
  isStreaming?: boolean;
}> = ({
  message,
  viewMode,
  thinkingContent,
  isThinkingStreaming,
  segments,
  isStreaming,
}) => (
  <div
    className="w-full flex justify-start"
    data-testid={`message-${message.role}`}
    data-message-id={message.id}
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
          {thinkingContent && <ThinkingBlock content={thinkingContent} isStreaming={isThinkingStreaming} />}

          {/* Interleaved text, command blocks, tool calls, and thinking in chronological order */}
          {segments.map((segment, index) => {
            if (segment.type === 'thinking') {
              return <ThinkingBlock key={`seg-think-${index}`} content={segment.content} />;
            }
            if (segment.type === 'command') {
              return (
                <CommandExecBlock
                  key={`seg-cmd-${index}`}
                  blocks={[{ command: segment.command, output: segment.output }]}
                />
              );
            }
            if (segment.type === 'tool_call') {
              return (
                <ToolCallBlock
                  key={`seg-tc-${index}`}
                  toolCalls={[segment.toolCall]}
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

          {/* Processing indicator: shown when streaming and last segment is a completed command */}
          {isStreaming && segments.length > 0 && segments[segments.length - 1].type === 'command'
            && (segments[segments.length - 1] as { output: string | null }).output !== null && (
            <div className="flex items-center gap-2 px-3 py-2 text-xs text-blue-300/70">
              <span className="inline-block w-3 h-3 border-2 border-blue-400 border-t-transparent rounded-full animate-spin" />
              Processing tool output...
            </div>
          )}
        </>
      )}

    </div>
  </div>
);

/**
 * Message bubble component - renders user, assistant, or system messages.
 */
export const MessageBubble: React.FC<MessageBubbleProps> = ({ message, viewMode = 'text', isStreaming }) => {
  const {
    cleanContent,
    thinkingContent,
    isThinkingStreaming,
    segments,
    isError,
  } = useMessageParsing(message);

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
    return <UserMessage message={message} cleanContent={cleanContent} viewMode={viewMode} />;
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
    />
  );
};
