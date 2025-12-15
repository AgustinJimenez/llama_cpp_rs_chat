import React, { useMemo } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { dracula } from 'react-syntax-highlighter/dist/esm/styles/prism';
import type { Message } from '../../types';
import { autoParseToolCalls, stripToolCalls } from '../../utils/toolParser';

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

  // Extract SYSTEM.EXEC blocks (command executions)
  const systemExecBlocks = useMemo(() => {
    const blocks: { command: string; output: string | null }[] = [];
    const execRegex = /<\|\|SYSTEM\.EXEC>([\s\S]*?)<SYSTEM\.EXEC\|\|>/g;
    const outputRegex = /<\|\|SYSTEM\.OUTPUT>([\s\S]*?)<SYSTEM\.OUTPUT\|\|>/g;

    let match;
    while ((match = execRegex.exec(message.content)) !== null) {
      const command = match[1].trim();
      blocks.push({ command, output: null });
    }

    // Match outputs to commands (they should appear in order)
    let outputIndex = 0;
    while ((match = outputRegex.exec(message.content)) !== null) {
      if (outputIndex < blocks.length) {
        blocks[outputIndex].output = match[1].trim();
        outputIndex++;
      }
    }

    return blocks;
  }, [message.content]);

  // Get content without thinking tags AND without SYSTEM.EXEC/OUTPUT tags
  const contentWithoutThinking = useMemo(() => {
    let content = cleanContent;
    // Remove thinking tags
    content = content.replace(/<think>[\s\S]*?<\/think>/g, '');
    // Remove SYSTEM.EXEC blocks
    content = content.replace(/<\|\|SYSTEM\.EXEC>[\s\S]*?<SYSTEM\.EXEC\|\|>/g, '');
    // Remove SYSTEM.OUTPUT blocks
    content = content.replace(/<\|\|SYSTEM\.OUTPUT>[\s\S]*?<SYSTEM\.OUTPUT\|\|>/g, '');
    return content.trim();
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

        {/* Display SYSTEM.EXEC blocks (command executions) */}
        {systemExecBlocks.length > 0 && (
          <div className="space-y-3">
            {systemExecBlocks.map((block, index) => (
              <div
                key={`exec-${index}`}
                className="rounded-lg overflow-hidden border border-green-500/30"
              >
                {/* Command header */}
                <div className="bg-green-950/70 px-3 py-2 flex items-center gap-2">
                  <span className="text-green-400">⚡</span>
                  <span className="text-xs font-medium text-green-300">Command Executed</span>
                </div>
                {/* Command content */}
                <div className="bg-black/40 px-3 py-2">
                  <code className="text-sm text-green-200 font-mono">
                    {block.command}
                  </code>
                </div>
                {/* Output (if present) */}
                {block.output && (
                  <>
                    <div className="bg-gray-900/50 px-3 py-1 border-t border-green-500/20">
                      <span className="text-xs text-gray-400">Output:</span>
                    </div>
                    <div className="bg-black/60 px-3 py-2 max-h-64 overflow-auto">
                      <pre className="text-xs text-gray-300 font-mono whitespace-pre-wrap">
                        {block.output}
                      </pre>
                    </div>
                  </>
                )}
              </div>
            ))}
          </div>
        )}

        {/* Display tool calls if present (OLD system - kept for compatibility) */}
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
                <pre className="text-xs text-white bg-black/20 p-3 rounded overflow-x-auto whitespace-pre-wrap">
                  {(() => {
                    // If arguments is already a string, display it
                    if (typeof toolCall.arguments === 'string') {
                      return toolCall.arguments;
                    }

                    // Format arguments manually to avoid re-escaping
                    const lines: string[] = ['{'];
                    const entries = Object.entries(toolCall.arguments);
                    entries.forEach(([key, value], index) => {
                      const isLast = index === entries.length - 1;

                      if (typeof value === 'string') {
                        // Unescape the string value for display
                        const unescaped = value
                          .replace(/\\n/g, '\n')
                          .replace(/\\"/g, '"')
                          .replace(/\\t/g, '\t')
                          .replace(/\\\\/g, '\\');

                        // For multiline content, display it nicely
                        if (unescaped.includes('\n')) {
                          lines.push(`  "${key}":`);
                          lines.push(unescaped.split('\n').map(line => `    ${line}`).join('\n'));
                        } else {
                          lines.push(`  "${key}": "${unescaped}"${isLast ? '' : ','}`);
                        }
                      } else {
                        lines.push(`  "${key}": ${JSON.stringify(value)}${isLast ? '' : ','}`);
                      }
                    });
                    lines.push('}');

                    return lines.join('\n');
                  })()}
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