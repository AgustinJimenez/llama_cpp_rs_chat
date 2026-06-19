import { Database } from 'lucide-react';
import { useState, useRef, useEffect } from 'react';

const MIN_BAR_SEGMENT_PCT = 0.3;

import type { TokenBreakdown } from '../../../utils/chatTransport';

interface Props {
  breakdown: TokenBreakdown;
  tokensUsed: number;
  maxTokens: number;
  formatNumber: (n: number) => string;
}

const CATEGORIES = [
  { key: 'system_prompt' as const, label: 'System Prompt', color: '#6366f1' },
  { key: 'tool_definitions' as const, label: 'Tool Definitions', color: '#8b5cf6' },
  { key: 'conversation_messages' as const, label: 'Messages', color: '#3b82f6' },
  { key: 'tool_calls_and_results' as const, label: 'Tool I/O', color: '#f59e0b' },
  { key: 'model_response' as const, label: 'Response', color: '#10b981' },
];

export const TokenBreakdownPopover = ({
  breakdown,
  tokensUsed,
  maxTokens,
  formatNumber,
}: Props) => {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open]);

  const free = maxTokens - tokensUsed;
  const freePct = maxTokens > 0 ? ((free / maxTokens) * 100).toFixed(1) : '0.0';

  return (
    <div ref={ref} className="relative inline-flex">
      <button
        onClick={() => setOpen(!open)}
        className="inline-flex cursor-pointer items-center gap-1 transition-colors hover:text-blue-400"
        title="Click for context breakdown"
      >
        <Database className="h-3 w-3" />
        {formatNumber(tokensUsed)}/{formatNumber(maxTokens)}
      </button>
      {!!open && (
        <div className="absolute bottom-full right-0 z-50 mb-2 w-72 rounded-lg border border-border bg-card p-3 shadow-xl">
          <div className="mb-2 text-xs font-semibold text-foreground">Context Usage</div>
          {/* Stacked bar */}
          <div className="mb-3 flex h-3 overflow-hidden rounded-full bg-muted">
            {CATEGORIES.map(({ key, color }) => {
              const value = breakdown[key];
              const pct = maxTokens > 0 ? (value / maxTokens) * 100 : 0;
              if (pct < MIN_BAR_SEGMENT_PCT) return null;
              return (
                <div
                  key={key}
                  className="h-full"
                  style={{ width: `${pct}%`, backgroundColor: color }}
                />
              );
            })}
          </div>
          {/* Per-category rows */}
          {CATEGORIES.map(({ key, label, color }) => {
            const value = breakdown[key];
            const pct = maxTokens > 0 ? (value / maxTokens) * 100 : 0;
            return (
              <div key={key} className="mb-1 flex items-center gap-2 text-[11px]">
                <div
                  className="h-2 w-2 flex-shrink-0 rounded-full"
                  style={{ backgroundColor: color }}
                />
                <span className="flex-1 text-muted-foreground">{label}</span>
                <span className="tabular-nums text-foreground/80">{formatNumber(value)}</span>
                <span className="w-12 text-right tabular-nums text-muted-foreground">
                  {pct.toFixed(1)}%
                </span>
              </div>
            );
          })}
          {/* Free */}
          <div className="mt-1 flex items-center gap-2 border-t border-border pt-1 text-[11px]">
            <div className="h-2 w-2 flex-shrink-0 rounded-full bg-muted-foreground" />
            <span className="flex-1 text-muted-foreground">Free</span>
            <span className="tabular-nums text-foreground/80">{formatNumber(free)}</span>
            <span className="w-12 text-right tabular-nums text-muted-foreground">{freePct}%</span>
          </div>
        </div>
      )}
    </div>
  );
};
