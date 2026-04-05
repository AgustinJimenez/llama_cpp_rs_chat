import { useState, useRef, useEffect } from 'react';
import { Database } from 'lucide-react';
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

export function TokenBreakdownPopover({ breakdown, tokensUsed, maxTokens, formatNumber }: Props) {
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

  return (
    <div ref={ref} className="relative inline-flex">
      <button
        onClick={() => setOpen(!open)}
        className="inline-flex items-center gap-1 hover:text-blue-400 transition-colors cursor-pointer"
        title="Click for context breakdown"
      >
        <Database className="h-3 w-3" />
        {formatNumber(tokensUsed)}/{formatNumber(maxTokens)}
      </button>
      {open && (
        <div className="absolute bottom-full mb-2 right-0 w-72 bg-card border border-border rounded-lg shadow-xl p-3 z-50">
          <div className="text-xs font-semibold text-white mb-2">Context Usage</div>
          {/* Stacked bar */}
          <div className="h-3 bg-muted rounded-full overflow-hidden flex mb-3">
            {CATEGORIES.map(({ key, color }) => {
              const value = breakdown[key];
              const pct = maxTokens > 0 ? (value / maxTokens) * 100 : 0;
              if (pct < 0.3) return null;
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
              <div key={key} className="flex items-center gap-2 mb-1 text-[11px]">
                <div className="w-2 h-2 rounded-full flex-shrink-0" style={{ backgroundColor: color }} />
                <span className="text-muted-foreground flex-1">{label}</span>
                <span className="text-foreground/80 tabular-nums">{formatNumber(value)}</span>
                <span className="text-muted-foreground w-12 text-right tabular-nums">{pct.toFixed(1)}%</span>
              </div>
            );
          })}
          {/* Free */}
          <div className="flex items-center gap-2 mt-1 pt-1 border-t border-border text-[11px]">
            <div className="w-2 h-2 rounded-full flex-shrink-0 bg-muted-foreground" />
            <span className="text-muted-foreground flex-1">Free</span>
            <span className="text-foreground/80 tabular-nums">{formatNumber(free)}</span>
            <span className="text-muted-foreground w-12 text-right tabular-nums">{maxTokens > 0 ? ((free / maxTokens) * 100).toFixed(1) : '0.0'}%</span>
          </div>
        </div>
      )}
    </div>
  );
}
