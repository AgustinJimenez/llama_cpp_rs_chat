import { useRef, useCallback, useEffect } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { LoadingIndicator, WelcomeMessage } from '../atoms';
import { MessageBubble } from '../organisms';
import type { Message, ViewMode } from '../../types';
import type { LoadingAction } from '../../hooks/useModel';

interface MessagesAreaProps {
  messages: Message[];
  isLoading: boolean;
  isModelLoading: boolean;
  loadingAction?: LoadingAction;
  viewMode: ViewMode;
}

export function MessagesArea({
  messages,
  isLoading,
  isModelLoading,
  loadingAction,
  viewMode,
}: MessagesAreaProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const autoScrollRef = useRef(true);

  const itemCount = messages.length + (isLoading ? 1 : 0);

  const virtualizer = useVirtualizer({
    count: itemCount,
    getScrollElement: () => containerRef.current,
    estimateSize: () => 120,
    overscan: 5,
  });

  // Prevent virtualizer from fighting our auto-scroll during streaming.
  // When auto-scroll is active, we handle scroll position ourselves via rAF.
  // When user scrolled up, let the virtualizer adjust position on item resize
  // (e.g. expand/collapse blocks above viewport).
  virtualizer.shouldAdjustScrollPositionOnItemSizeChange = (_item, _delta) =>
    !autoScrollRef.current;

  // Engage auto-scroll when streaming starts
  useEffect(() => {
    if (isLoading) autoScrollRef.current = true;
  }, [isLoading]);

  // Auto-scroll to bottom when messages change (streaming tokens or new messages).
  // Uses rAF so we run after the browser has committed the DOM update.
  useEffect(() => {
    const el = containerRef.current;
    if (!el || !autoScrollRef.current) return;
    requestAnimationFrame(() => {
      if (autoScrollRef.current) {
        el.scrollTop = el.scrollHeight;
      }
    });
  }, [messages]);

  // Detect user scrolling up via wheel to disengage auto-scroll.
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (e.deltaY < 0) autoScrollRef.current = false;
    };
    el.addEventListener('wheel', onWheel, { passive: true });
    return () => el.removeEventListener('wheel', onWheel);
  });

  // Re-engage auto-scroll when user scrolls back to bottom
  const handleScroll = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    const distFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    if (distFromBottom < 80) {
      autoScrollRef.current = true;
    }
  }, []);

  return (
    <div
      ref={containerRef}
      className="flex-1 overflow-y-auto overflow-x-hidden"
      data-testid="messages-container"
      onScroll={handleScroll}
    >
      {messages.length === 0 ? (
        <div className="max-w-3xl mx-auto px-6 py-6">
          <WelcomeMessage isModelLoading={isModelLoading} loadingAction={loadingAction} />
        </div>
      ) : (
        <div className="max-w-3xl mx-auto px-6 py-6">
          <div
            style={{
              height: virtualizer.getTotalSize(),
              position: 'relative',
              width: '100%',
            }}
          >
            {virtualizer.getVirtualItems().map((virtualRow) => (
              <div
                key={virtualRow.key}
                ref={virtualizer.measureElement}
                data-index={virtualRow.index}
                style={{
                  position: 'absolute',
                  top: 0,
                  left: 0,
                  width: '100%',
                  transform: `translateY(${virtualRow.start}px)`,
                }}
              >
                <div className="pb-6">
                  {virtualRow.index < messages.length ? (
                    <MessageBubble
                      message={messages[virtualRow.index]}
                      viewMode={viewMode}
                    />
                  ) : (
                    <LoadingIndicator />
                  )}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
