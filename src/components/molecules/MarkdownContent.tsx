import React from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { dracula } from 'react-syntax-highlighter/dist/esm/styles/prism';

interface MarkdownContentProps {
  content: string;
  testId?: string;
}

/**
 * Reusable markdown renderer with syntax highlighting.
 * Used for both user and assistant messages.
 */
export const MarkdownContent: React.FC<MarkdownContentProps> = ({ content, testId }) => {
  return (
    <div className="text-sm prose prose-sm max-w-none prose-invert" data-testid={testId}>
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
          p: ({ children }) => <p className="my-2">{children}</p>,
          h1: ({ children }) => (
            <h1 className="font-bold text-2xl my-3 border-b border-border pb-2">{children}</h1>
          ),
          h2: ({ children }) => (
            <h2 className="font-bold text-xl my-3 border-b border-border pb-2">{children}</h2>
          ),
          h3: ({ children }) => (
            <h3 className="font-semibold text-lg my-2">{children}</h3>
          ),
          ul: ({ children }) => <ul className="list-disc ml-4 my-2">{children}</ul>,
          ol: ({ children }) => <ol className="list-decimal ml-4 my-2">{children}</ol>,
          li: ({ children }) => <li>{children}</li>,
          strong: ({ children }) => <strong className="font-bold">{children}</strong>,
          em: ({ children }) => <em className="italic">{children}</em>,
          blockquote: ({ children }) => (
            <blockquote className="border-l-4 border-border pl-4 my-2 italic">{children}</blockquote>
          ),
        }}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
};
