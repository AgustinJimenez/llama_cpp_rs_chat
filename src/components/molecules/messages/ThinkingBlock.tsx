import { ChevronDown, ChevronRight } from 'lucide-react';
import React, { useState, useEffect, useRef } from 'react';

const MIN_ELAPSED_DISPLAY_SECONDS = 0.5;

interface ThinkingBlockProps {
  content: string;
  isStreaming?: boolean;
}

/**
 * Collapsible thinking/reasoning block for models like Qwen3.
 * Opens automatically and shows animated indicator while streaming.
 * Shows elapsed thinking time when > 0.5s.
 */
export const ThinkingBlock: React.FC<ThinkingBlockProps> = ({ content, isStreaming }) => {
  const [isOpen, setIsOpen] = useState(!!isStreaming);
  const [elapsed, setElapsed] = useState(0);
  const startTimeRef = useRef<number | null>(null);

  // Auto-open when streaming starts
  useEffect(() => {
    if (isStreaming) setIsOpen(true);
  }, [isStreaming]);

  // Elapsed time tracking
  useEffect(() => {
    if (isStreaming) {
      if (startTimeRef.current === null) startTimeRef.current = Date.now();
      const interval = setInterval(() => {
        setElapsed((Date.now() - (startTimeRef.current ?? Date.now())) / 1000);
      }, 100);
      return () => clearInterval(interval);
    }
    // Freeze final time when streaming stops
    if (startTimeRef.current !== null) {
      setElapsed((Date.now() - startTimeRef.current) / 1000);
    }
  }, [isStreaming]);

  const timeLabel = elapsed >= MIN_ELAPSED_DISPLAY_SECONDS ? ` (${elapsed.toFixed(1)}s)` : '';
  const thinkingLabel = isStreaming ? `Thinking...${timeLabel}` : `Thinking${timeLabel}`;

  // Qwen3 emits empty <think></think> tags between tool calls — skip rendering
  if (!isStreaming && !content.trim()) return null;

  return (
    <div className="overflow-hidden rounded-xl" style={{ border: '1px solid hsl(220 8% 28%)' }}>
      <button
        type="button"
        onClick={() => setIsOpen(!isOpen)}
        className="flex w-full items-center gap-2 bg-muted px-3 py-2 text-left transition-colors hover:bg-accent"
      >
        <span
          className={`flex-1 text-xs font-medium ${isStreaming ? 'shimmer-text' : 'text-foreground'}`}
        >
          {thinkingLabel}
        </span>
        {!!isOpen && <ChevronDown className="size-3.5 flex-shrink-0 text-foreground" />}
        {!isOpen && <ChevronRight className="size-3.5 flex-shrink-0 text-foreground" />}
      </button>
      {!!isOpen && (
        <pre
          className="max-h-64 overflow-y-auto whitespace-pre-wrap bg-card px-3 py-2 font-mono text-xs leading-relaxed text-foreground"
          style={{ borderTop: '1px solid hsl(220 8% 28%)' }}
        >
          {content}
          {!!isStreaming && (
            <span className="ml-0.5 inline-block h-3.5 w-1.5 animate-pulse bg-foreground/50 align-middle" />
          )}
        </pre>
      )}
    </div>
  );
};
