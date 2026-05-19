import { useVirtualizer } from '@tanstack/react-virtual';
import { ArrowDown } from 'lucide-react';
import React, { useRef, useCallback, useEffect, useState } from 'react';

const ESTIMATED_ROW_HEIGHT_PX = 120;
const SCROLL_BOTTOM_THRESHOLD_PX = 80;

import { useChatContext } from '../../contexts/ChatContext';
import { useModelContext } from '../../contexts/ModelContext';
import { useUIContext } from '../../hooks/useUIContext';
import { LoadingIndicator } from '../atoms';
import { MessageBubble } from '../organisms';

const RecoveryOrLoading: React.FC<{ isCrashRecovery: boolean; isModelLoading: boolean }> = ({
  isCrashRecovery,
  isModelLoading,
}) => {
  if (!isCrashRecovery) return <LoadingIndicator />;
  const label = isModelLoading ? 'Reloading model...' : 'Resuming generation...';
  return (
    <div className="py-4 flex items-center gap-3 text-sm text-muted-foreground">
      <span className="inline-block w-4 h-4 border-2 border-primary border-t-transparent rounded-full animate-spin" />
      {label}
    </div>
  );
};

// eslint-disable-next-line max-lines-per-function
export const MessagesArea = () => {
  const { messages, isLoading, editMessage, regenerateFrom, continueFrom } = useChatContext();
  const { status: modelStatus, isLoading: isModelLoading } = useModelContext();
  const { viewMode } = useUIContext();
  const containerRef = useRef<HTMLDivElement>(null);
  const autoScrollRef = useRef(true);
  // Flag to distinguish programmatic scrolls from user-initiated ones.
  const programmaticScrollRef = useRef(false);
  const [showScrollDown, setShowScrollDown] = useState(false);

  // Show recovery indicator when backend is reloading after crash or auto-continuing
  const tailMsg = messages[messages.length - 1];
  const isCrashRecovery =
    !isLoading &&
    tailMsg?.role === 'system' &&
    tailMsg.content.includes('[System:') &&
    (isModelLoading || modelStatus.generating === true);
  const showLoadingRow = isCrashRecovery;
  const itemCount = messages.length + (showLoadingRow ? 1 : 0);

  const virtualizer = useVirtualizer({
    count: itemCount,
    getScrollElement: () => containerRef.current,
    estimateSize: () => ESTIMATED_ROW_HEIGHT_PX,
    overscan: 5,
  });

  // Let virtualizer adjust scroll only when user scrolled up (not during auto-scroll).
  virtualizer.shouldAdjustScrollPositionOnItemSizeChange = (_item, _delta) =>
    !autoScrollRef.current;

  // Preserve scroll position across viewMode toggles.
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

  // Auto-scroll to bottom on new messages / streaming tokens.
  const prevMessageCountRef = useRef(messages.length);
  const prevLastContentLenRef = useRef(0);
  useEffect(() => {
    const el = containerRef.current;
    if (!el || !autoScrollRef.current) return;
    const lastMsg = messages[messages.length - 1];
    const lastContentLen = lastMsg?.content?.length ?? 0;
    const countChanged = messages.length !== prevMessageCountRef.current;
    const contentGrew = lastContentLen > prevLastContentLenRef.current;
    prevMessageCountRef.current = messages.length;
    prevLastContentLenRef.current = lastContentLen;
    if (!countChanged && !contentGrew && !isLoading) return;
    requestAnimationFrame(() => {
      if (autoScrollRef.current) {
        programmaticScrollRef.current = true;
        el.scrollTop = el.scrollHeight;
      }
    });
  }, [messages, isLoading]);

  const handleScroll = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    if (programmaticScrollRef.current) {
      programmaticScrollRef.current = false;
      return;
    }

    const distFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    if (distFromBottom < SCROLL_BOTTOM_THRESHOLD_PX) {
      autoScrollRef.current = true;
      setShowScrollDown(false);
    } else {
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
                      onContinue={continueFrom}
                      isGenerating={isLoading ? virtualRow.index === messages.length - 1 : false}
                      isLastMessage={virtualRow.index === messages.length - 1}
                    />
                  ) : (
                    <RecoveryOrLoading
                      isCrashRecovery={isCrashRecovery}
                      isModelLoading={isModelLoading}
                    />
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
