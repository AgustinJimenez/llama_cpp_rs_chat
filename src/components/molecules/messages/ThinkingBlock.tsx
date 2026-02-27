import React, { useState, useEffect } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';

interface ThinkingBlockProps {
  content: string;
  isStreaming?: boolean;
}

/**
 * Collapsible thinking/reasoning block for models like Qwen3.
 * Opens automatically and shows animated indicator while streaming.
 */
export const ThinkingBlock: React.FC<ThinkingBlockProps> = ({ content, isStreaming }) => {
  const [isOpen, setIsOpen] = useState(!!isStreaming);

  // Auto-open when streaming starts
  useEffect(() => {
    if (isStreaming) setIsOpen(true);
  }, [isStreaming]);

  return (
    <div
      className="rounded-xl overflow-hidden"
      style={{ border: '1px solid hsl(220 8% 28%)' }}
    >
      <button
        type="button"
        onClick={() => setIsOpen(!isOpen)}
        className="w-full bg-muted px-3 py-2 flex items-center gap-2 text-left hover:bg-accent transition-colors"
      >
        {isStreaming ? (
          <span className="inline-block w-3 h-3 border-2 border-foreground/50 border-t-transparent rounded-full animate-spin flex-shrink-0" />
        ) : null}
        <span className="text-xs font-medium text-foreground flex-1">
          {isStreaming ? 'Thinking...' : 'Thinking'}
        </span>
        {isOpen
          ? <ChevronDown className="h-3.5 w-3.5 text-foreground flex-shrink-0" />
          : <ChevronRight className="h-3.5 w-3.5 text-foreground flex-shrink-0" />
        }
      </button>
      {isOpen ? (
        <pre
          className="text-xs text-foreground font-mono bg-card px-3 py-2 whitespace-pre-wrap leading-relaxed max-h-64 overflow-y-auto"
          style={{ borderTop: '1px solid hsl(220 8% 28%)' }}
        >
          {content}
          {isStreaming ? <span className="inline-block w-1.5 h-3.5 bg-foreground/50 ml-0.5 animate-pulse align-middle" /> : null}
        </pre>
      ) : null}
    </div>
  );
};
