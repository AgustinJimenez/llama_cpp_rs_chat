import { ChevronDown, ChevronUp, Search, X } from 'lucide-react';
import React from 'react';
import { useTranslation } from 'react-i18next';

export const CommitSearchBar: React.FC<{
  inputRef: React.RefObject<HTMLInputElement>;
  searchQuery: string;
  matchCount: number;
  safeIdx: number;
  onChange: (q: string) => void;
  onClose: () => void;
  onNext: () => void;
  onPrev: () => void;
}> = ({ inputRef, searchQuery, matchCount, safeIdx, onChange, onClose, onNext, onPrev }) => {
  const { t } = useTranslation();
  return (
    <div className="absolute inset-x-0 top-0 z-10 flex justify-center px-4 pt-2">
      <div className="flex items-center gap-2 rounded-lg border border-border bg-background px-4 py-2 shadow-lg">
        <Search className="size-4 shrink-0 text-muted-foreground/60" />
        <input
          ref={inputRef}
          type="text"
          value={searchQuery}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Escape') onClose();
            else if (e.key === 'Enter') {
              if (e.shiftKey) onPrev(); else onNext();
            }
          }}
          placeholder={t('gitGraph.searchPlaceholder')}
          className="w-52 bg-transparent text-sm text-foreground placeholder:text-muted-foreground/40 focus:outline-none"
        />
        {searchQuery.trim() && matchCount > 0 && (
          <span className="shrink-0 text-xs tabular-nums text-muted-foreground/70">
            {safeIdx + 1} / {matchCount}
          </span>
        )}
        {searchQuery.trim() && matchCount === 0 && (
          <span className="shrink-0 text-xs text-destructive/80">{t('gitGraph.noResults')}</span>
        )}
        <div className="mx-1 h-4 w-px bg-border/40" />
        <button type="button" onClick={onPrev} disabled={matchCount === 0} className="rounded p-1 hover:bg-muted disabled:opacity-30" title="Previous (Shift+Enter)">
          <ChevronUp className="size-4" />
        </button>
        <button type="button" onClick={onNext} disabled={matchCount === 0} className="rounded p-1 hover:bg-muted disabled:opacity-30" title="Next (Enter)">
          <ChevronDown className="size-4" />
        </button>
        <button type="button" onClick={onClose} className="rounded p-1 hover:bg-muted" title="Close (Esc)">
          <X className="size-4" />
        </button>
      </div>
    </div>
  );
};
