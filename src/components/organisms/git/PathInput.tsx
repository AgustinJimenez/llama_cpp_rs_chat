import { Check } from 'lucide-react';
import React, { useEffect, useState } from 'react';

const BLUR_DELAY = 150;

export const PathInput: React.FC<{
  value: string;
  recentPaths: string[];
  placeholder: string;
  onChange: (v: string) => void;
  onLoad: (p: string) => void;
}> = ({ value, recentPaths, placeholder, onChange, onLoad }) => {
  const [open, setOpen] = useState(false);

  useEffect(() => { setOpen(false); }, [value]);

  const dropdownEl = open && recentPaths.length > 0 && (
    <div className="absolute left-0 right-0 top-full z-20 mt-0.5 max-h-60 overflow-y-auto rounded border border-border bg-background shadow-lg">
      {recentPaths.map((p) => (
        <button
          key={p}
          type="button"
          onMouseDown={(e) => { e.preventDefault(); onChange(p); setOpen(false); onLoad(p); }}
          className="flex w-full items-center gap-2 px-3 py-2 text-left text-xs text-foreground hover:bg-muted"
        >
          <span className="truncate flex-1">{p}</span>
          {p === value && <Check className="size-3 shrink-0 text-muted-foreground" />}
        </button>
      ))}
    </div>
  );

  return (
    <div className="relative min-w-0 flex-1">
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') { setOpen(false); onLoad(value); }
          else if (e.key === 'Escape') setOpen(false);
        }}
        onFocus={() => setOpen(true)}
        onBlur={() => setTimeout(() => setOpen(false), BLUR_DELAY)}
        placeholder={placeholder}
        className="w-full rounded border border-border bg-muted/30 px-2 py-1 text-xs text-foreground placeholder:text-muted-foreground/50 focus:outline-none focus:ring-1 focus:ring-ring"
      />
      {dropdownEl}
    </div>
  );
};
