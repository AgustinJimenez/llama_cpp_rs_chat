import { X } from 'lucide-react';
import { useState, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';

const EVENT_POLL_INTERVAL_MS = 3000;

import { useChatContext } from '../../contexts/ChatContext';
import { useUIContext } from '../../hooks/useUIContext';

interface ConversationEvent {
  timestamp: number;
  event_type: string;
  message: string;
}

const TYPE_COLORS: Record<string, string> = {
  tool_call: 'text-blue-400',
  tool_results: 'text-blue-300',
  stall: 'text-red-400',
  compaction: 'text-purple-400',
  loop_recovery: 'text-yellow-400',
  yn_check: 'text-cyan-400',
  context_guard: 'text-orange-400',
  provider_start: 'text-green-400',
  provider_iteration: 'text-green-300',
  provider_done: 'text-green-400',
  provider_complete: 'text-green-500',
  provider_error: 'text-red-400',
  provider_abort: 'text-red-300',
};

const TYPE_BG: Record<string, string> = {
  tool_call: 'bg-blue-400/20',
  tool_results: 'bg-blue-300/20',
  stall: 'bg-red-400/20',
  compaction: 'bg-purple-400/20',
  loop_recovery: 'bg-yellow-400/20',
  yn_check: 'bg-cyan-400/20',
  context_guard: 'bg-orange-400/20',
  provider_start: 'bg-green-400/20',
  provider_iteration: 'bg-green-300/20',
  provider_done: 'bg-green-400/20',
  provider_complete: 'bg-green-500/20',
  provider_error: 'bg-red-400/20',
  provider_abort: 'bg-red-300/20',
};

function formatTime(ts: number): string {
  const d = new Date(ts);
  return d.toLocaleTimeString('en-US', {
    hour12: false,
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

export const ConversationLog = () => {
  const { t } = useTranslation();
  const { currentConversationId } = useChatContext();
  const { isEventLogOpen, toggleEventLog } = useUIContext();
  const [events, setEvents] = useState<ConversationEvent[]>([]);
  const scrollRef = useRef<HTMLDivElement>(null);

  // eslint-disable-next-line react-doctor/no-fetch-in-effect
  useEffect(() => {
    if (!currentConversationId || !isEventLogOpen) return;

    const fetchEvents = async () => {
      try {
        const id = currentConversationId;
        const resp = await fetch(`/api/conversations/${id}/events`);
        if (resp.ok) {
          const data = await resp.json();
          setEvents(data);
        }
      } catch {
        // ignore
      }
    };

    fetchEvents();
    const interval = setInterval(fetchEvents, EVENT_POLL_INTERVAL_MS);
    return () => clearInterval(interval);
  }, [currentConversationId, isEventLogOpen]);

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [events]);

  if (!isEventLogOpen) return null;

  const eventsContent =
    events.length === 0 ? (
      <p className="italic text-muted-foreground">{t('eventLog.emptyState')}</p>
    ) : (
      events.map((ev) => (
        <div key={`${ev.timestamp}-${ev.event_type}`} className="flex items-start gap-2">
          <span className="flex-shrink-0 text-muted-foreground">{formatTime(ev.timestamp)}</span>
          <span
            className={`flex-shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium ${TYPE_COLORS[ev.event_type] || 'text-muted-foreground'} ${TYPE_BG[ev.event_type] || 'bg-muted'}`}
          >
            {ev.event_type}
          </span>
          <span className="text-foreground/80">{ev.message}</span>
        </div>
      ))
    );

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      role="button"
      tabIndex={0}
      onClick={toggleEventLog}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') toggleEventLog();
      }}
    >
      {/* eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions -- inner div only prevents propagation */}
      <div
        className="flex max-h-[70vh] w-[700px] max-w-[90vw] flex-col rounded-lg border border-border bg-card shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-border px-4 py-3">
          <h3 className="text-sm font-medium text-foreground">{t('eventLog.title')}</h3>
          <button
            onClick={toggleEventLog}
            className="rounded p-1 text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </div>
        <div
          ref={scrollRef}
          className="min-h-[200px] flex-1 space-y-1 overflow-y-auto px-4 py-3 font-mono text-xs"
        >
          {eventsContent}
        </div>
      </div>
    </div>
  );
};
