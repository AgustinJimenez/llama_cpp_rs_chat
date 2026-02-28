import React, { useState } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';

interface ParamGroupProps {
  title: string;
  children: React.ReactNode;
  /** Additional CSS classes on the outer wrapper */
  className?: string;
  /** Muted + non-interactive when true */
  disabled?: boolean;
  /** Make the group collapsible with a toggle header */
  collapsible?: boolean;
  /** Initial expanded state when collapsible (default: true) */
  defaultExpanded?: boolean;
  /** Use free-form layout instead of flex-wrap row */
  freeLayout?: boolean;
}

export const ParamGroup: React.FC<ParamGroupProps> = ({
  title, children, className = '', disabled, collapsible, defaultExpanded = true, freeLayout,
}) => {
  const [expanded, setExpanded] = useState(defaultExpanded);

  return (
    <div className={`rounded-md border border-zinc-700 px-3 py-2 ${disabled ? 'opacity-40 pointer-events-none' : ''} ${className}`}>
      {collapsible ? (
        <button
          type="button"
          className="flex items-center justify-between w-full text-left"
          onClick={() => setExpanded(!expanded)}
        >
          <span className="text-xs font-medium">{title}</span>
          {expanded
            ? <ChevronDown className="h-3.5 w-3.5 text-muted-foreground" />
            : <ChevronRight className="h-3.5 w-3.5 text-muted-foreground" />}
        </button>
      ) : (
        <span className="text-xs font-medium">{title}</span>
      )}
      {(!collapsible || expanded) && (
        freeLayout
          ? <div className="mt-1">{children}</div>
          : <div className="flex flex-wrap items-center gap-x-4 gap-y-1 mt-1">{children}</div>
      )}
    </div>
  );
};
