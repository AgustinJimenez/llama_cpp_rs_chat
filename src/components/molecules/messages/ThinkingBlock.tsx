import React from 'react';

interface ThinkingBlockProps {
  content: string;
}

/**
 * Collapsible thinking/reasoning block for models like Qwen3.
 */
export const ThinkingBlock: React.FC<ThinkingBlockProps> = ({ content }) => {
  return (
    <details className="p-3 bg-blue-950/50 rounded-lg border border-blue-500/30">
      <summary className="cursor-pointer text-xs font-medium text-blue-300 mb-2">
        💭 Thinking Process
      </summary>
      <div className="text-xs text-blue-200 whitespace-pre-wrap leading-relaxed mt-2">
        {content}
      </div>
    </details>
  );
};
