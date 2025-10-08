import React, { useMemo } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import { Card, CardContent } from '@/components/ui/card';
import type { Message } from '../types';
import { autoParseToolCalls, stripToolCalls } from '../utils/toolParser';
import 'highlight.js/styles/github-dark.css';

interface MessageBubbleProps {
  message: Message;
  viewMode?: 'text' | 'markdown';
}

export const MessageBubble: React.FC<MessageBubbleProps> = ({ message, viewMode = 'text' }) => {
  const isUser = message.role === 'user';

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

  if (isUser) {
    // User messages keep the card styling and right alignment
    return (
      <div
        className="flex w-full justify-end"
        data-testid={`message-${message.role}`}
        data-message-id={message.id}
      >
        <Card className="border-0 shadow-md max-w-[80%] bg-gradient-to-br from-slate-600 to-slate-500 text-white">
          <CardContent className="p-3">
            {viewMode === 'markdown' ? (
              <div className="text-sm prose prose-invert prose-sm max-w-none" data-testid="message-content">
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
          </CardContent>
        </Card>
      </div>
    );
  }

  // Assistant messages take full width with no card styling
  return (
    <div
      className="w-full space-y-2"
      data-testid={`message-${message.role}`}
      data-message-id={message.id}
    >
      {/* Display tool calls if present */}
      {toolCalls.length > 0 && (
        <div className="space-y-2">
          {toolCalls.map((toolCall) => (
            <div
              key={toolCall.id}
              className="p-3 bg-blue-900/30 border border-blue-700/50 rounded-lg"
            >
              <div className="flex items-center gap-2 mb-2">
                <span className="text-xs font-mono text-blue-300">🔧 Tool Call</span>
                <span className="text-xs font-semibold text-blue-200">{toolCall.name}</span>
              </div>
              <pre className="text-xs text-blue-100 bg-blue-950/50 p-2 rounded overflow-x-auto">
                {JSON.stringify(toolCall.arguments, null, 2)}
              </pre>
            </div>
          ))}
        </div>
      )}

      {/* Display clean message content */}
      {cleanContent && cleanContent.trim() && (
        <>
          {viewMode === 'markdown' ? (
            <div className="text-sm prose prose-slate prose-sm max-w-none prose-invert text-slate-200" data-testid="message-content">
              <ReactMarkdown
                remarkPlugins={[remarkGfm]}
                rehypePlugins={[rehypeHighlight]}
                components={{
                  pre: ({ children }) => (
                    <pre className="my-0 p-0">{children}</pre>
                  ),
                  code: ({ node, inline, className, children, ...props }: any) => {
                    return inline ? (
                      <code className={`${className} bg-slate-700 text-slate-100 px-1.5 py-0.5 rounded`} {...props}>
                        {children}
                      </code>
                    ) : (
                      <code className={`${className} block bg-slate-800 text-slate-100 p-4 rounded-lg overflow-x-auto`} {...props}>
                        {children}
                      </code>
                    );
                  },
                  p: ({ children }) => (
                    <p className="text-slate-200 my-2">{children}</p>
                  ),
                  h1: ({ children }) => (
                    <h1 className="text-slate-100 font-bold text-xl my-2">{children}</h1>
                  ),
                  h2: ({ children }) => (
                    <h2 className="text-slate-100 font-bold text-lg my-2">{children}</h2>
                  ),
                  h3: ({ children }) => (
                    <h3 className="text-slate-100 font-bold text-base my-2">{children}</h3>
                  ),
                  ul: ({ children }) => (
                    <ul className="text-slate-200 list-disc ml-4 my-2">{children}</ul>
                  ),
                  ol: ({ children }) => (
                    <ol className="text-slate-200 list-decimal ml-4 my-2">{children}</ol>
                  ),
                  li: ({ children }) => (
                    <li className="text-slate-200">{children}</li>
                  ),
                }}
              >
                {cleanContent}
              </ReactMarkdown>
            </div>
          ) : (
            <p className="text-sm whitespace-pre-wrap leading-relaxed text-card-foreground" data-testid="message-content">
              {cleanContent}
            </p>
          )}
        </>
      )}
    </div>
  );
};