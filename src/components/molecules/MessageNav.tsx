import React, { useCallback, useEffect, useRef, useState } from 'react';

import type { Message } from '../../types';

interface MessageNavProps {
  messages: Message[];
}

interface NavEntry {
  index: number;
  title?: string;
  preview: string;
}

const TICK_INACTIVE_OPACITY = 0.35;

function buildEntries(messages: Message[]): NavEntry[] {
  return messages.reduce<NavEntry[]>((acc, msg, idx) => {
    if (msg.role === 'user' && !msg.isSystemPrompt) {
      acc.push({
        index: idx,
        title: msg.title,
        preview: msg.content.slice(0, 60).replaceAll('\n', ' '),
      });
    }
    return acc;
  }, []);
}

const TickButton: React.FC<{
  entry: NavEntry;
  isActive: boolean;
  onScrollTo: (index: number) => void;
}> = ({ entry, isActive, onScrollTo }) => {
  const tickClass = isActive
    ? 'bg-primary transition-all'
    : 'bg-foreground group-hover:opacity-70 transition-all';
  const tickOpacity = isActive ? 1 : TICK_INACTIVE_OPACITY;
  return (
    <button
      onClick={() => onScrollTo(entry.index)}
      title={entry.title ?? entry.preview}
      aria-label={entry.title ?? entry.preview}
      style={{ flex: 1, minHeight: 8, maxHeight: 32, width: '100%', padding: 0 }}
      className="flex items-center justify-center group"
    >
      <div
        style={{ width: 3, height: '60%', minHeight: 6, borderRadius: 2, opacity: tickOpacity }}
        className={tickClass}
      />
    </button>
  );
};

const TooltipButton: React.FC<{
  entry: NavEntry;
  isActive: boolean;
  onScrollTo: (index: number) => void;
}> = ({ entry, isActive, onScrollTo }) => {
  const cls = isActive
    ? 'w-full px-3 py-2 text-left text-xs transition-colors hover:bg-accent hover:text-accent-foreground text-primary font-medium'
    : 'w-full px-3 py-2 text-left text-xs transition-colors hover:bg-accent hover:text-accent-foreground text-muted-foreground';
  return (
    <button onClick={() => onScrollTo(entry.index)} className={cls}>
      <span className="block truncate">{entry.title ?? entry.preview}</span>
    </button>
  );
};

export const MessageNav: React.FC<MessageNavProps> = ({ messages }) => {
  const entries = buildEntries(messages);
  const [activeIndex, setActiveIndex] = useState<number | null>(null);
  const [hovered, setHovered] = useState(false);
  const tooltipRef = useRef<HTMLDivElement>(null);

  // Reset active index when the conversation changes (messages array identity changes).
  const prevFirstIdRef = useRef<string | undefined>(undefined);
  useEffect(() => {
    const firstId = messages[0]?.id;
    if (firstId !== prevFirstIdRef.current) {
      prevFirstIdRef.current = firstId;
      setActiveIndex(null);
    }
  }, [messages]);

  const scrollTo = useCallback((index: number) => {
    setActiveIndex(index);
    window.dispatchEvent(new CustomEvent('scroll-to-message', { detail: { index } }));
  }, []);

  if (entries.length === 0) return null;

  const tooltip = hovered ? (
    <div
      ref={tooltipRef}
      className="absolute left-full top-0 z-50 ml-2 w-48 rounded-md border border-border bg-popover shadow-md"
      style={{ maxHeight: '80vh', overflowY: 'auto' }}
    >
      {entries.map((entry) => (
        <TooltipButton
          key={entry.index}
          entry={entry}
          isActive={activeIndex === entry.index}
          onScrollTo={scrollTo}
        />
      ))}
    </div>
  ) : null;

  return (
    <div
      className="relative flex flex-col items-center py-6"
      style={{ width: 24, flexShrink: 0 }}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <div className="flex flex-col items-center gap-0" style={{ flex: 1 }}>
        {entries.map((entry) => (
          <TickButton
            key={entry.index}
            entry={entry}
            isActive={activeIndex === entry.index}
            onScrollTo={scrollTo}
          />
        ))}
      </div>
      {tooltip}
    </div>
  );
};
