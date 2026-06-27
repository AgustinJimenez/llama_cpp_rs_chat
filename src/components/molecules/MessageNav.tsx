import { useVirtualizer } from '@tanstack/react-virtual';
import React, { useCallback, useEffect, useRef, useState } from 'react';

import type { Message } from '../../types';

interface MessageNavProps {
  messages: Message[];
}

interface NavEntry {
  index: number;
  label: string;
  bars: string[];
}

const LABEL_MAX_LEN = 50;
const TOTAL_BARS = 5;
const BAR_ADD = '#3fb950';
const BAR_DEL = '#f85149';
const BAR_NEU = 'rgba(127,127,127,0.3)';
const ROW_HEIGHT = 30;
const SCROLL_HIDE_DELAY_MS = 800;


function computeBars(messages: Message[], fromIdx: number): string[] {
  let writes = 0;
  let errors = 0;
  for (let i = fromIdx + 1; i < messages.length; i++) {
    const m = messages[i];
    if (m.role === 'user' && !m.isSystemPrompt) break;
    if (m.role === 'assistant') {
      if (/write_file|edit_file|insert_text/.test(m.content)) writes++;
      if (m.content.includes('[TOOL_RESULT:error]')) errors++;
    }
  }
  if (writes === 0 && errors === 0) return Array(TOTAL_BARS).fill(BAR_NEU);
  const addBars = writes > 0 ? Math.max(1, Math.min(4, writes)) : 0;
  const delBars = errors > 0 ? Math.min(2, errors) : 0;
  const neu = Math.max(0, TOTAL_BARS - addBars - delBars);
  return [...Array(addBars).fill(BAR_ADD), ...Array(delBars).fill(BAR_DEL), ...Array(neu).fill(BAR_NEU)];
}

function buildEntries(messages: Message[]): NavEntry[] {
  return messages.reduce<NavEntry[]>((acc, msg, idx) => {
    if (msg.role === 'user' && !msg.isSystemPrompt) {
      const raw = msg.title ?? msg.content.replaceAll(/\s+/g, ' ').trim().slice(0, LABEL_MAX_LEN);
      acc.push({ index: idx, label: raw || '…', bars: computeBars(messages, idx) });
    }
    return acc;
  }, []);
}

const BarsSvg: React.FC<{ bars: string[] }> = ({ bars }) => (
  <svg
    xmlns="http://www.w3.org/2000/svg"
    viewBox="0 0 18 14"
    fill="none"
    style={{ width: 18, height: 14, flexShrink: 0 }}
    aria-hidden="true"
  >
    {bars.slice(0, TOTAL_BARS).map((color, i) => (
      // eslint-disable-next-line react/no-array-index-key
      <rect key={i} x={i * 4} width="2" height="14" rx="1" fill={color} />
    ))}
  </svg>
);

export const MessageNav: React.FC<MessageNavProps> = ({ messages }) => {
  const entries = buildEntries(messages);
  const [activeIndex, setActiveIndex] = useState<number | null>(null);
  const scrollRef = useRef<HTMLElement>(null);
  const scrollTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const handleNavScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    el.classList.add('is-scrolling');
    if (scrollTimerRef.current) clearTimeout(scrollTimerRef.current);
    scrollTimerRef.current = setTimeout(() => el.classList.remove('is-scrolling'), SCROLL_HIDE_DELAY_MS);
  }, []);

  const prevFirstIdRef = useRef<string | undefined>(undefined);
  useEffect(() => {
    const firstId = messages[0]?.id;
    if (firstId !== prevFirstIdRef.current) {
      prevFirstIdRef.current = firstId;
      setActiveIndex(null);
    }
  }, [messages]);

  const virtualizer = useVirtualizer({
    count: entries.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 10,
  });

  const scrollTo = useCallback((index: number) => {
    setActiveIndex(index);
    window.dispatchEvent(new CustomEvent('scroll-to-message', { detail: { index } }));
  }, []);

  if (entries.length < 1) return null;

  return (
    <nav
      ref={scrollRef}
      aria-label="Message sections"
      onScroll={handleNavScroll}
      className="absolute left-0 top-0 flex flex-col h-full py-2 overflow-y-auto z-10"
      style={{ width: 240 }}
    >
      <ul
        style={{ height: virtualizer.getTotalSize(), position: 'relative' }}
        aria-label="Message sections list"
      >
        {virtualizer.getVirtualItems().map((row) => {
          const entry = entries[row.index];
          if (!entry) return null;
          const isActive = activeIndex === entry.index;
          return (
            <li
              key={entry.index}
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                width: '100%',
                transform: `translateY(${row.start}px)`,
                height: row.size,
              }}
            >
              <button
                onClick={() => scrollTo(entry.index)}
                className={`w-full h-full flex items-center gap-3 px-3 text-left text-sm leading-snug transition-colors hover:text-foreground ${
                  isActive ? 'text-foreground' : 'text-muted-foreground/70'
                }`}
              >
                <BarsSvg bars={entry.bars} />
                <span className="truncate min-w-0">{entry.label}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </nav>
  );
};
