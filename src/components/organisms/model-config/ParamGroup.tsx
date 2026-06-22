import { ChevronDown, ChevronRight } from 'lucide-react';
import React, { useState } from 'react';

interface ParamGroupProps {
  title: React.ReactNode;
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
  title,
  children,
  className = '',
  disabled,
  collapsible,
  defaultExpanded = true,
  freeLayout,
}) => {
  const [expanded, setExpanded] = useState(() => defaultExpanded);
  const collapseChevron = expanded ? (
    <ChevronDown className="size-3.5 text-muted-foreground" />
  ) : (
    <ChevronRight className="size-3.5 text-muted-foreground" />
  );
  const titleEl = collapsible ? (
    <button
      type="button"
      className="flex w-full items-center justify-between text-left"
      onClick={() => setExpanded((v) => !v)}
    >
      <span className="text-xs font-medium">{title}</span>
      {collapseChevron}
    </button>
  ) : (
    <span className="text-xs font-medium">{title}</span>
  );
  const childrenEl =
    !collapsible || expanded
      ? (() => {
          if (freeLayout) {
            return <div className="mt-1">{children}</div>;
          }
          return <div className="mt-1 flex flex-wrap items-center gap-x-4 gap-y-1">{children}</div>;
        })()
      : null;

  return (
    <div
      className={`rounded-md border border-border px-3 py-2 ${disabled ? 'pointer-events-none opacity-40' : ''} ${className}`}
    >
      {titleEl}
      {childrenEl}
    </div>
  );
};
