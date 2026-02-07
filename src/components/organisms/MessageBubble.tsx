import React, { useState, useCallback } from 'react';
import type { Message, ToolCall } from '../../types';
import type { MessageSegment } from '../../hooks/useMessageParsing';
import { useMessageParsing } from '../../hooks/useMessageParsing';
import { MarkdownContent } from '../molecules/MarkdownContent';
import { ThinkingBlock, CommandExecBlock, ToolCallBlock } from '../molecules/messages';
import { isTauriEnv } from '../../utils/tauri';

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
 * Copy button for assistant messages.
 * Uses Tauri clipboard plugin in desktop, falls back to navigator.clipboard in web.
 */
const CopyButton: React.FC<{ text: string }> = ({ text }) => {
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(async () => {
    try {
      if (isTauriEnv()) {
        const { writeText } = await import('@tauri-apps/plugin-clipboard-manager');
        await writeText(text);
      } else {
        await navigator.clipboard.writeText(text);
      }
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Fallback: try navigator.clipboard
      try {
        await navigator.clipboard.writeText(text);
        setCopied(true);
        setTimeout(() => setCopied(false), 2000);
      } catch { /* ignore */ }
    }
  }, [text]);

  return (
    <button
      onClick={handleCopy}
      className="opacity-0 group-hover:opacity-100 transition-opacity text-xs text-muted-foreground hover:text-foreground px-1.5 py-0.5 rounded hover:bg-muted"
      title="Copy response"
    >
      {copied ? 'Copied!' : 'Copy'}
    </button>
  );
};

/**
 * Assistant message component with thinking, tool calls, and command blocks
 * rendered in chronological order.
 */
const AssistantMessage: React.FC<{
  message: Message;
  viewMode: 'text' | 'markdown';
  thinkingContent: string | null;
  segments: MessageSegment[];
  toolCalls: ToolCall[];
  cleanContent: string;
}> = ({
  message,
  viewMode,
  thinkingContent,
  segments,
  toolCalls,
  cleanContent,
}) => (
  <div
    className="w-full flex justify-start space-y-2 group"
    data-testid={`message-${message.role}`}
    data-message-id={message.id}
  >
    <div className="max-w-[80%] space-y-2 overflow-hidden">
      {/* Thinking process (for reasoning models) */}
      {thinkingContent && <ThinkingBlock content={thinkingContent} />}

      {/* Interleaved text and command blocks in chronological order */}
      {segments.map((segment, index) => {
        if (segment.type === 'command') {
          return (
            <CommandExecBlock
              key={`seg-cmd-${index}`}
              blocks={[{ command: segment.command, output: segment.output }]}
            />
          );
        }
        // Text segment
        const text = segment.content;
        if (!text.trim()) return null;
        return (
          <div key={`seg-txt-${index}`} className="flat-message-assistant p-4">
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

      {/* Tool calls (legacy system) */}
      <ToolCallBlock toolCalls={toolCalls} />

      {/* Copy button - appears on hover */}
      {cleanContent.trim() && <CopyButton text={cleanContent} />}
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
    thinkingContent,
    segments,
    isError,
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
      thinkingContent={thinkingContent}
      segments={segments}
      toolCalls={toolCalls}
      cleanContent={cleanContent}
    />
  );
};
