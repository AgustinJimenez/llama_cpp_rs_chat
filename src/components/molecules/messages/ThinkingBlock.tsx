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
    <div className="bg-blue-950/50 rounded-lg border border-blue-500/30">
      <button
        type="button"
        onClick={() => setIsOpen(!isOpen)}
        className="w-full px-3 py-2 flex items-center justify-between cursor-pointer"
      >
        <span className="text-xs font-medium text-blue-300">
          {isStreaming ? 'Thinking...' : 'Thinking'}
        </span>
        {isOpen
          ? <ChevronDown className="w-3.5 h-3.5 text-blue-400" />
          : <ChevronRight className="w-3.5 h-3.5 text-blue-400" />
        }
      </button>
      {isOpen ? <div className="px-3 pb-3 text-xs text-blue-200 whitespace-pre-wrap leading-relaxed">
          {content}
          {isStreaming ? <span className="inline-block w-1.5 h-3.5 bg-blue-400 ml-0.5 animate-pulse align-middle" /> : null}
        </div> : null}
    </div>
  );
};
