import React, { useMemo } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import type { Message } from '../types';
import { autoParseToolCalls, stripToolCalls } from '../utils/toolParser';
import 'highlight.js/styles/github-dark.css';

interface MessageBubbleProps {
  message: Message;
  viewMode?: 'text' | 'markdown';
}

export const MessageBubble: React.FC<MessageBubbleProps> = ({ message, viewMode = 'text' }) => {
  const isUser = message.role === 'user';
  const isSystem = message.role === 'system';

  // Parse tool calls from assistant messages
  const toolCalls = useMemo(() => {
    if (message.role === 'assistant') {
      return autoParseToolCalls(message.content);
    }
    return [];
  }, [message.content, message.role]);

  // Strip tool call markers from content
  const cleanContent = useMemo(() => {
    if (toolCalls.length > 0) {
      return stripToolCalls(message.content);
    }
    return message.content;
  }, [message.content, toolCalls.length]);

  // Detect error messages (contain ❌ or "Error" or "Panic")
  const isError = isSystem && (
    message.content.includes('❌') ||
    message.content.includes('Generation Crashed') ||
    message.content.includes('Error')
  );

  // System messages (especially errors)
  if (isSystem) {
    return (
      <div
        className="w-full flex justify-center"
        data-testid={`message-${message.role}`}
        data-message-id={message.id}
      >
        <div className={`max-w-[90%] w-full p-4 rounded-lg ${
          isError
            ? 'bg-red-950 border-2 border-red-500'
            : 'bg-yellow-950 border-2 border-yellow-500'
        }`}>
          <div className="flex items-center gap-2 mb-2">
            <span className="text-sm font-bold text-white">
              {isError ? '❌ SYSTEM ERROR' : '⚠️ SYSTEM'}
            </span>
          </div>
          <pre className={`text-sm whitespace-pre-wrap leading-relaxed ${
            isError ? 'text-red-200' : 'text-yellow-200'
          }`}>
            {cleanContent}
          </pre>
        </div>
      </div>
    );
  }

  if (isUser) {
    // User messages keep the card styling and right alignment
    return (
      <div
        className="flex w-full justify-end"
        data-testid={`message-${message.role}`}
        data-message-id={message.id}
      >
        <div className="flat-message-user max-w-[80%] p-4">
          {viewMode === 'markdown' ? (
            <div className="text-sm prose prose-sm max-w-none prose-invert" data-testid="message-content">
              <ReactMarkdown
                remarkPlugins={[remarkGfm]}
                rehypePlugins={[rehypeHighlight]}
              >
                {cleanContent}
              </ReactMarkdown>
            </div>
          ) : (
            <p className="text-sm whitespace-pre-wrap leading-relaxed" data-testid="message-content">
              {cleanContent}
            </p>
          )}
        </div>
      </div>
    );
  }

  // Assistant messages aligned to the left with max width
  return (
    <div
      className="w-full flex justify-start space-y-2"
      data-testid={`message-${message.role}`}
      data-message-id={message.id}
    >
      <div className="max-w-[80%] space-y-2">
        {/* Display tool calls if present */}
        {toolCalls.length > 0 && (
          <div className="space-y-3">
            {toolCalls.map((toolCall) => (
              <div
                key={toolCall.id}
                className="p-3 bg-flat-purple rounded-lg"
              >
                <div className="flex items-center gap-2 mb-2">
                  <span className="text-xs font-medium text-white">🔧 Tool Call</span>
                  <span className="text-xs font-medium text-white">{toolCall.name}</span>
                </div>
                <pre className="text-xs text-white bg-black/20 p-3 rounded overflow-x-auto">
                  {JSON.stringify(toolCall.arguments, null, 2)}
                </pre>
              </div>
            ))}
          </div>
        )}

        {/* Display clean message content */}
        {cleanContent && cleanContent.trim() && (
          <div className="flat-message-assistant p-4">
          {viewMode === 'markdown' ? (
            <div className="text-sm prose prose-sm max-w-none dark:prose-invert" data-testid="message-content">
              <ReactMarkdown
                remarkPlugins={[remarkGfm]}
                rehypePlugins={[rehypeHighlight]}
                components={{
                  pre: ({ children }) => (
                    <pre className="my-2 p-0">{children}</pre>
                  ),
                  code: ({ node, inline, className, children, ...props }: any) => {
                    return inline ? (
                      <code className={`${className} bg-muted px-2 py-1 rounded font-mono`} {...props}>
                        {children}
                      </code>
                    ) : (
                      <code className={`${className} block bg-gray-900 text-green-400 p-4 rounded-lg overflow-x-auto font-mono`} {...props}>
                        {children}
                      </code>
                    );
                  },
                  p: ({ children }) => (
                    <p className="my-2">{children}</p>
                  ),
                  h1: ({ children }) => (
                    <h1 className="font-bold text-2xl my-3 border-b border-border pb-2">{children}</h1>
                  ),
                  h2: ({ children }) => (
                    <h2 className="font-bold text-xl my-3 border-b border-border pb-2">{children}</h2>
                  ),
                  h3: ({ children }) => (
                    <h3 className="font-semibold text-lg my-2">{children}</h3>
                  ),
                  ul: ({ children }) => (
                    <ul className="list-disc ml-4 my-2">{children}</ul>
                  ),
                  ol: ({ children }) => (
                    <ol className="list-decimal ml-4 my-2">{children}</ol>
                  ),
                  li: ({ children }) => (
                    <li>{children}</li>
                  ),
                }}
              >
                {cleanContent}
              </ReactMarkdown>
            </div>
          ) : (
            <p className="text-sm whitespace-pre-wrap leading-relaxed" data-testid="message-content">
              {cleanContent}
            </p>
          )}
        </div>
        )}
      </div>
    </div>
  );
};