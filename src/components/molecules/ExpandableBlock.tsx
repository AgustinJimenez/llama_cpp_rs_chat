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
      className="absolute top-2 right-2 opacity-40 group-hover:opacity-100 transition-opacity"
    >
      <button
        onClick={() => setOpen(!open)}
        className="p-1.5 bg-black/60 text-white rounded-full hover:bg-black/80 transition-colors backdrop-blur"
        title="Options"
      >
        <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
          <circle cx="8" cy="3" r="1.5" />
          <circle cx="8" cy="8" r="1.5" />
          <circle cx="8" cy="13" r="1.5" />
        </svg>
      </button>
      {open ? (
        <div className="absolute right-0 mt-1 bg-card border border-border rounded-lg shadow-lg py-1 min-w-[120px] z-50">
          {actions.map((a) => (
            <button
              key={a.label}
              onClick={() => {
                a.onClick();
                setOpen(false);
              }}
              className="w-full px-3 py-1.5 text-left text-sm text-foreground hover:bg-muted transition-colors"
            >
              {a.label}
            </button>
          ))}
        </div>
      ) : null}
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
        className={`my-2 relative group cursor-pointer ${className || ''}`}
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
      {expanded
        ? createPortal(
            <div
              className="fixed inset-0 z-[9999] bg-black/95 flex items-center justify-center cursor-pointer"
              role="button"
              tabIndex={0}
              onClick={() => setExpanded(false)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === 'Escape') setExpanded(false);
              }}
            >
              <div
                className="max-w-[90vw] max-h-[90vh] flex items-center justify-center overflow-auto"
                role="presentation"
                onClick={(e) => e.stopPropagation()}
              >
                <div className="[&>*]:mx-auto [&_svg]:mx-auto [&_canvas]:mx-auto">{children}</div>
              </div>
              <button
                onClick={() => setExpanded(false)}
                className="absolute top-4 right-4 p-2 bg-white/20 text-white rounded-full hover:bg-white/30 transition-colors backdrop-blur"
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
          )
        : null}
    </>
  );
};
