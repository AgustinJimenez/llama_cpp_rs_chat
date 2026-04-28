import { useVirtualizer } from '@tanstack/react-virtual';
import { ArrowDown } from 'lucide-react';
import { useRef, useCallback, useEffect, useState } from 'react';

const ESTIMATED_ROW_HEIGHT_PX = 120;
const SCROLL_BOTTOM_THRESHOLD_PX = 80;

import { useChatContext } from '../../contexts/ChatContext';
import { useUIContext } from '../../hooks/useUIContext';
import { LoadingIndicator } from '../atoms';
import { MessageBubble } from '../organisms';

export const MessagesArea = () => {
  const { messages, isLoading, editMessage, regenerateFrom } = useChatContext();
  const { viewMode } = useUIContext();
  const containerRef = useRef<HTMLDivElement>(null);
  const autoScrollRef = useRef(true);
  // Flag to distinguish programmatic scrolls from user-initiated ones.
  const programmaticScrollRef = useRef(false);
  const [showScrollDown, setShowScrollDown] = useState(false);

  const showLoadingRow = isLoading;
  const itemCount = messages.length + (showLoadingRow ? 1 : 0);

  const virtualizer = useVirtualizer({
    count: itemCount,
    getScrollElement: () => containerRef.current,
    estimateSize: () => ESTIMATED_ROW_HEIGHT_PX,
    overscan: 5,
  });

  // Prevent virtualizer from fighting our auto-scroll during streaming.
  // When auto-scroll is active, we handle scroll position ourselves via rAF.
  // When user scrolled up, let the virtualizer adjust position on item resize
  // (e.g. expand/collapse blocks above viewport).
  virtualizer.shouldAdjustScrollPositionOnItemSizeChange = (_item, _delta) =>
    !autoScrollRef.current;

  // Preserve scroll position across viewMode toggles (raw/rendered).
  // Save scrollTop ratio before re-render, restore after virtualizer re-measures.
  const prevViewModeRef = useRef(viewMode);
  const savedScrollRatioRef = useRef<number | null>(null);
  useEffect(() => {
    if (viewMode !== prevViewModeRef.current) {
      const el = containerRef.current;
      if (el && el.scrollHeight > el.clientHeight) {
        savedScrollRatioRef.current = el.scrollTop / (el.scrollHeight - el.clientHeight);
      }
      prevViewModeRef.current = viewMode;
    }
  }, [viewMode]);
  useEffect(() => {
    if (savedScrollRatioRef.current === null) return;
    const ratio = savedScrollRatioRef.current;
    savedScrollRatioRef.current = null;
    // Wait for virtualizer to re-measure after viewMode change
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        const el = containerRef.current;
        if (!el) return;
        programmaticScrollRef.current = true;
        el.scrollTop = ratio * (el.scrollHeight - el.clientHeight);
      });
    });
  });

  // Engage auto-scroll when streaming starts
  useEffect(() => {
    if (isLoading) autoScrollRef.current = true;
  }, [isLoading]);

  // Auto-scroll to bottom when messages change (streaming tokens or new messages).
  // Uses rAF so we run after the browser has committed the DOM update.
  // Track message count to avoid scrolling on layout changes (browser toggle).
  const prevMessageCountRef = useRef(messages.length);
  useEffect(() => {
    const el = containerRef.current;
    if (!el || !autoScrollRef.current) return;
    // Only scroll if messages actually changed (not just layout reflow)
    const countChanged = messages.length !== prevMessageCountRef.current;
    const lastMsg = messages[messages.length - 1];
    const contentChanged = countChanged || (lastMsg && isLoading);
    prevMessageCountRef.current = messages.length;
    if (!contentChanged) return;
    requestAnimationFrame(() => {
      if (autoScrollRef.current) {
        programmaticScrollRef.current = true;
        el.scrollTop = el.scrollHeight;
      }
    });
  }, [messages, isLoading]);

  // Detect user scroll to disengage/re-engage auto-scroll.
  // Works for ALL scroll methods: wheel, scrollbar drag, trackpad, touch, keyboard.
  const handleScroll = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;

    // Ignore scroll events caused by our own programmatic scrollTop assignment
    if (programmaticScrollRef.current) {
      programmaticScrollRef.current = false;
      return;
    }

    const distFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    if (distFromBottom < SCROLL_BOTTOM_THRESHOLD_PX) {
      autoScrollRef.current = true;
      setShowScrollDown(false);
    } else {
      // User scrolled away from bottom — disengage auto-scroll
      autoScrollRef.current = false;
      setShowScrollDown(true);
    }
  }, []);

  const scrollToBottom = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    autoScrollRef.current = true;
    setShowScrollDown(false);
    programmaticScrollRef.current = true;
    el.scrollTop = el.scrollHeight;
  }, []);

  return (
    <div className="relative flex-1 overflow-hidden">
      <div
        ref={containerRef}
        className="h-full overflow-y-auto overflow-x-hidden"
        data-testid="messages-container"
        onScroll={handleScroll}
      >
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
                      isStreaming={isLoading ? virtualRow.index === messages.length - 1 : undefined}
                      messageIndex={virtualRow.index}
                      onEditMessage={editMessage}
                      onRegenerate={regenerateFrom}
                      isGenerating={isLoading}
                      isLastMessage={virtualRow.index === messages.length - 1}
                    />
                  ) : (
                    <LoadingIndicator />
                  )}
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>

      {showScrollDown ? (
        <button
          onClick={scrollToBottom}
          className="absolute bottom-6 left-1/2 -translate-x-1/2 z-10
            flex items-center justify-center w-9 h-9 rounded-full
            bg-muted hover:bg-accent border border-border
            text-muted-foreground hover:text-foreground
            shadow-lg transition-opacity duration-200 cursor-pointer"
          aria-label="Scroll to bottom"
        >
          <ArrowDown size={18} />
        </button>
      ) : null}
    </div>
  );
};
