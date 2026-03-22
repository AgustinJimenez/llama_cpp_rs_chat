import { useState, useEffect, useRef } from 'react';
import { useChatContext } from '../../contexts/ChatContext';

interface ConversationEvent {
  timestamp: number;
  event_type: string;
  message: string;
}

const TYPE_COLORS: Record<string, string> = {
  tool_call: 'text-blue-400',
  stall: 'text-red-400',
  compaction: 'text-purple-400',
  loop_recovery: 'text-yellow-400',
  yn_check: 'text-cyan-400',
  context_guard: 'text-orange-400',
};

const TYPE_BG: Record<string, string> = {
  tool_call: 'bg-blue-400/20',
  stall: 'bg-red-400/20',
  compaction: 'bg-purple-400/20',
  loop_recovery: 'bg-yellow-400/20',
  yn_check: 'bg-cyan-400/20',
  context_guard: 'bg-orange-400/20',
};

function formatTime(ts: number): string {
  const d = new Date(ts);
  return d.toLocaleTimeString('en-US', { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

export function ConversationLog() {
  const { currentConversationId } = useChatContext();
  const [events, setEvents] = useState<ConversationEvent[]>([]);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!currentConversationId) return;

    const fetchEvents = async () => {
      try {
        const id = currentConversationId.replace('.txt', '');
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
    const interval = setInterval(fetchEvents, 3000);
    return () => clearInterval(interval);
  }, [currentConversationId]);

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [events]);

  return (
    <div className="border-t border-border bg-zinc-950">
      <div
        ref={scrollRef}
        className="h-40 overflow-y-auto px-3 py-2 font-mono text-xs space-y-0.5"
      >
        {events.length === 0 ? (
          <p className="text-zinc-600 italic">No events yet — events appear during generation</p>
        ) : (
          events.map((ev, i) => (
            <div key={i} className="flex items-start gap-2">
              <span className="text-zinc-600 flex-shrink-0">{formatTime(ev.timestamp)}</span>
              <span className={`px-1.5 rounded text-[10px] font-medium flex-shrink-0 ${TYPE_COLORS[ev.event_type] || 'text-zinc-400'} ${TYPE_BG[ev.event_type] || 'bg-zinc-800'}`}>
                {ev.event_type}
              </span>
              <span className="text-zinc-300">{ev.message}</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
