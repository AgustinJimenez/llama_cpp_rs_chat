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

function buildEntries(messages: Message[]): NavEntry[] {
  return messages.reduce<NavEntry[]>((acc, msg, idx) => {
    if (msg.role === 'user' && !msg.isSystemPrompt) {
      acc.push({
        index: idx,
        title: msg.title,
        preview: msg.content.slice(0, 60).replace(/\n/g, ' '),
      });
    }
    return acc;
  }, []);
}

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

  return (
    <div
      className="relative flex flex-col items-center py-6"
      style={{ width: 24, flexShrink: 0 }}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      {/* Tick marks */}
      <div className="flex flex-col items-center gap-0" style={{ flex: 1 }}>
        {entries.map((entry) => (
          <button
            key={entry.index}
            onClick={() => scrollTo(entry.index)}
            title={entry.title ?? entry.preview}
            aria-label={entry.title ?? entry.preview}
            style={{ flex: 1, minHeight: 8, maxHeight: 32, width: '100%', padding: 0 }}
            className="flex items-center justify-center group"
          >
            <div
              style={{ width: 3, height: '60%', minHeight: 6, borderRadius: 2, opacity: activeIndex === entry.index ? 1 : 0.35 }}
              className={
                activeIndex === entry.index
                  ? 'bg-primary transition-all'
                  : 'bg-foreground group-hover:opacity-70 transition-all'
              }
            />
          </button>
        ))}
      </div>

      {/* Hover tooltip: full list with titles */}
      {hovered && (
        <div
          ref={tooltipRef}
          className="absolute left-full top-0 z-50 ml-2 w-48 rounded-md border border-border bg-popover shadow-md"
          style={{ maxHeight: '80vh', overflowY: 'auto' }}
        >
          {entries.map((entry) => (
            <button
              key={entry.index}
              onClick={() => scrollTo(entry.index)}
              className={`w-full px-3 py-2 text-left text-xs transition-colors hover:bg-accent hover:text-accent-foreground ${
                activeIndex === entry.index ? 'text-primary font-medium' : 'text-muted-foreground'
              }`}
            >
              <span className="block truncate">
                {entry.title ?? entry.preview}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
};
