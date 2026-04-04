import React, { useEffect, useRef, useState, useCallback } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import mermaid from 'mermaid';
import { SyntaxHighlighter, dracula } from '../../utils/syntaxHighlighterSetup';
import type { Components } from 'react-markdown';

// Initialize mermaid with dark theme
mermaid.initialize({
  startOnLoad: false,
  theme: 'dark',
  securityLevel: 'loose',
  fontFamily: 'inherit',
});

type CodeBlockProps = {
  inline?: boolean;
  className?: string;
  children?: React.ReactNode;
};

interface MarkdownContentProps {
  content: string;
  testId?: string;
}

/** Renders a mermaid diagram from source code. */
const MermaidBlock: React.FC<{ code: string }> = ({ code }) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const [svg, setSvg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const id = `mermaid-${Math.random().toString(36).slice(2, 9)}`;
    mermaid.render(id, code.trim()).then(({ svg: renderedSvg }) => {
      if (!cancelled) setSvg(renderedSvg);
    }).catch((err) => {
      if (!cancelled) setError(String(err));
    });
    return () => { cancelled = true; };
  }, [code]);

  const handleExport = useCallback(() => {
    if (!containerRef.current) return;
    const svgEl = containerRef.current.querySelector('svg');
    if (!svgEl) return;
    // Export as PNG via canvas
    const svgData = new XMLSerializer().serializeToString(svgEl);
    const canvas = document.createElement('canvas');
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const img = new Image();
    img.onload = () => {
      canvas.width = img.width * 2;
      canvas.height = img.height * 2;
      ctx.scale(2, 2);
      ctx.drawImage(img, 0, 0);
      const a = document.createElement('a');
      a.download = 'diagram.png';
      a.href = canvas.toDataURL('image/png');
      a.click();
    };
    img.src = 'data:image/svg+xml;base64,' + btoa(unescape(encodeURIComponent(svgData)));
  }, []);

  if (error) {
    return (
      <div className="my-2 p-3 bg-red-900/30 border border-red-700 rounded text-sm">
        <div className="text-red-400 font-medium mb-1">Mermaid Error</div>
        <pre className="text-xs text-red-300 whitespace-pre-wrap">{error}</pre>
        <pre className="text-xs text-muted-foreground mt-2 whitespace-pre-wrap">{code}</pre>
      </div>
    );
  }

  if (!svg) {
    return <div className="my-2 p-4 bg-muted rounded animate-pulse text-sm text-muted-foreground">Rendering diagram...</div>;
  }

  return (
    <div className="my-2">
      <div
        ref={containerRef}
        className="bg-[#1a1a2e] rounded-lg p-4 overflow-x-auto"
        dangerouslySetInnerHTML={{ __html: svg }}
      />
      <button
        onClick={handleExport}
        className="mt-1 text-xs text-muted-foreground hover:text-foreground transition-colors"
      >
        Export PNG
      </button>
    </div>
  );
};

/**
 * Reusable markdown renderer with syntax highlighting.
 * Used for both user and assistant messages.
 */
const CodeBlock = ({ inline, className, children }: CodeBlockProps) => {
  const match = /language-(\w+)/.exec(className || '');
  const language = match ? match[1] : '';
  const content = String(children ?? '').replace(/\n$/, '');

  // Render mermaid diagrams
  if (!inline && language === 'mermaid') {
    return <MermaidBlock code={content} />;
  }

  return !inline && language ? (
    <SyntaxHighlighter
      style={dracula}
      language={language}
      PreTag="div"
    >
      {content}
    </SyntaxHighlighter>
  ) : (
    <code className={`${className ?? ''} bg-muted px-2 py-1 rounded font-mono text-sm`}>
      {content}
    </code>
  );
};

export const MarkdownContent: React.FC<MarkdownContentProps> = ({ content, testId }) => {
  const components: Components = {
    code: CodeBlock,
    pre: ({ children }) => <div className="my-2">{children}</div>,
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
  };

  return (
    <div className="text-sm prose prose-sm max-w-none prose-invert" data-testid={testId}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={components}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
};
