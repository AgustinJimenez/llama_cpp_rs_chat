import React, { useMemo } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { dracula } from 'react-syntax-highlighter/dist/esm/styles/prism';
import type { Message } from '../types';
import { autoParseToolCalls, stripToolCalls } from '../utils/toolParser';

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

  // Strip tool call markers from content and handle thinking tags
  const cleanContent = useMemo(() => {
    let content = message.content;

    if (toolCalls.length > 0) {
      content = stripToolCalls(content);
    }

    return content;
  }, [message.content, toolCalls.length]);

  // Extract thinking content if present (for reasoning models like Qwen3)
  const thinkingContent = useMemo(() => {
    const thinkMatch = message.content.match(/<think>([\s\S]*?)<\/think>/);
    return thinkMatch ? thinkMatch[1].trim() : null;
  }, [message.content]);

  // Get content without thinking tags
  const contentWithoutThinking = useMemo(() => {
    return cleanContent.replace(/<think>[\s\S]*?<\/think>/g, '').trim();
  }, [cleanContent]);

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
                components={{
                  code: ({ node, inline, className, children, ...props }: any) => {
                    const match = /language-(\w+)/.exec(className || '');
                    const language = match ? match[1] : '';

                    return !inline && language ? (
                      <SyntaxHighlighter
                        style={dracula}
                        language={language}
                        PreTag="div"
                        customStyle={{
                          margin: '0',
                          padding: '0',
                          background: 'transparent',
                        }}
                        {...props}
                      >
                        {String(children).replace(/\n$/, '')}
                      </SyntaxHighlighter>
                    ) : (
                      <code className={`${className} bg-muted px-2 py-1 rounded font-mono text-sm`} {...props}>
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
                  strong: ({ children }) => (
                    <strong className="font-bold">{children}</strong>
                  ),
                  em: ({ children }) => (
                    <em className="italic">{children}</em>
                  ),
                  blockquote: ({ children }) => (
                    <blockquote className="border-l-4 border-border pl-4 my-2 italic">{children}</blockquote>
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
        {/* Display thinking process if present (for reasoning models) */}
        {thinkingContent && (
          <details className="p-3 bg-blue-950/50 rounded-lg border border-blue-500/30">
            <summary className="cursor-pointer text-xs font-medium text-blue-300 mb-2">
              💭 Thinking Process
            </summary>
            <div className="text-xs text-blue-200 whitespace-pre-wrap leading-relaxed mt-2">
              {thinkingContent}
            </div>
          </details>
        )}

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
        {contentWithoutThinking && contentWithoutThinking.trim() && (
          <div className="flat-message-assistant p-4">
          {viewMode === 'markdown' ? (
            <div className="text-sm prose prose-sm max-w-none prose-invert" data-testid="message-content">
              <ReactMarkdown
                remarkPlugins={[remarkGfm]}
                components={{
                  code: ({ node, inline, className, children, ...props }: any) => {
                    const match = /language-(\w+)/.exec(className || '');
                    const language = match ? match[1] : '';

                    return !inline && language ? (
                      <SyntaxHighlighter
                        style={dracula}
                        language={language}
                        PreTag="div"
                        customStyle={{
                          margin: '0',
                          padding: '0',
                          background: 'transparent',
                        }}
                        {...props}
                      >
                        {String(children).replace(/\n$/, '')}
                      </SyntaxHighlighter>
                    ) : (
                      <code className={`${className} bg-muted px-2 py-1 rounded font-mono text-sm`} {...props}>
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
                  strong: ({ children }) => (
                    <strong className="font-bold">{children}</strong>
                  ),
                  em: ({ children }) => (
                    <em className="italic">{children}</em>
                  ),
                  blockquote: ({ children }) => (
                    <blockquote className="border-l-4 border-border pl-4 my-2 italic">{children}</blockquote>
                  ),
                }}
              >
                {contentWithoutThinking}
              </ReactMarkdown>
            </div>
          ) : (
            <p className="text-sm whitespace-pre-wrap leading-relaxed" data-testid="message-content">
              {contentWithoutThinking}
            </p>
          )}
        </div>
        )}
      </div>
    </div>
  );
};