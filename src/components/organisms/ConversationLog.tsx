import { X } from 'lucide-react';
import { useState, useEffect, useRef } from 'react';

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
  const { currentConversationId } = useChatContext();
  const { isEventLogOpen, toggleEventLog } = useUIContext();
  const [events, setEvents] = useState<ConversationEvent[]>([]);
  const scrollRef = useRef<HTMLDivElement>(null);

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
        className="bg-card border border-border rounded-lg shadow-2xl w-[700px] max-w-[90vw] max-h-[70vh] flex flex-col"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-4 py-3 border-b border-border">
          <h3 className="text-sm font-medium text-foreground">Event Log</h3>
          <button
            onClick={toggleEventLog}
            className="p-1 rounded hover:bg-accent text-muted-foreground hover:text-foreground transition-colors"
          >
            <X className="h-4 w-4" />
          </button>
        </div>
        <div
          ref={scrollRef}
          className="flex-1 overflow-y-auto px-4 py-3 font-mono text-xs space-y-1 min-h-[200px]"
        >
          {events.length === 0 ? (
            <p className="text-muted-foreground italic">
              No events yet — events appear during generation (stalls, compaction, context limits,
              Y/N checks)
            </p>
          ) : (
            events.map((ev) => (
              <div key={`${ev.timestamp}-${ev.event_type}`} className="flex items-start gap-2">
                <span className="text-muted-foreground flex-shrink-0">
                  {formatTime(ev.timestamp)}
                </span>
                <span
                  className={`px-1.5 py-0.5 rounded text-[10px] font-medium flex-shrink-0 ${TYPE_COLORS[ev.event_type] || 'text-muted-foreground'} ${TYPE_BG[ev.event_type] || 'bg-muted'}`}
                >
                  {ev.event_type}
                </span>
                <span className="text-foreground/80">{ev.message}</span>
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
};
