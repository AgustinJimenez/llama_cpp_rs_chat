import React, { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

/** Reusable 3-dot menu for visual blocks (charts, diagrams, images). */
export const ThreeDotMenu: React.FC<{ actions: { label: string; onClick: () => void }[] }> = ({
  actions,
}) => {
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

  return (
    <div
      ref={ref}
      className="absolute right-2 top-2 opacity-40 transition-opacity group-hover:opacity-100"
    >
      <button
        onClick={() => setOpen(!open)}
        className="rounded-full bg-black/60 p-1.5 text-white backdrop-blur transition-colors hover:bg-black/80"
        title="Options"
      >
        <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
          <circle cx="8" cy="3" r="1.5" />
          <circle cx="8" cy="8" r="1.5" />
          <circle cx="8" cy="13" r="1.5" />
        </svg>
      </button>
      {!!open && (
        <div className="absolute right-0 z-50 mt-1 min-w-[120px] rounded-lg border border-border bg-card py-1 shadow-lg">
          {actions.map((a) => (
            <button
              key={a.label}
              onClick={() => {
                a.onClick();
                setOpen(false);
              }}
              className="w-full px-3 py-1.5 text-left text-sm text-foreground transition-colors hover:bg-muted"
            >
              {a.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
};

/** Expandable visual block: 3-dot menu with expand + actions. */
export const ExpandableBlock: React.FC<{
  children: React.ReactNode;
  actions: { label: string; onClick: () => void }[];
  className?: string;
}> = ({ children, actions, className }) => {
  const [expanded, setExpanded] = useState(false);
  const allActions = actions;

  return (
    <>
      <div
        className={`group relative my-2 cursor-pointer ${className || ''}`}
        onClick={() => setExpanded(true)}
        role="button"
        tabIndex={0}
        onKeyDown={(e) => {
          if (e.key === 'Enter') setExpanded(true);
        }}
      >
        {children}
        <ThreeDotMenu actions={allActions} />
      </div>
      {!!expanded &&
        createPortal(
          <div
            className="fixed inset-0 z-[9999] flex cursor-pointer items-center justify-center bg-black/95"
            role="button"
            tabIndex={0}
            onClick={() => setExpanded(false)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === 'Escape') setExpanded(false);
            }}
          >
            <div
              className="flex max-h-[90vh] max-w-[90vw] items-center justify-center overflow-auto"
              role="presentation"
              onClick={(e) => e.stopPropagation()}
            >
              <div className="[&>*]:mx-auto [&_canvas]:mx-auto [&_svg]:mx-auto">{children}</div>
            </div>
            <button
              onClick={() => setExpanded(false)}
              className="absolute right-4 top-4 rounded-full bg-white/20 p-2 text-white backdrop-blur transition-colors hover:bg-white/30"
              title="Close"
            >
              <svg
                width="20"
                height="20"
                viewBox="0 0 20 20"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
              >
                <path d="M5 5l10 10M15 5L5 15" />
              </svg>
            </button>
          </div>,
          document.body,
        )}
    </>
  );
};
