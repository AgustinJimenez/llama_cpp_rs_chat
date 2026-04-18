import { X, RefreshCw, MousePointer2 } from 'lucide-react';
import React, { useCallback, useEffect, useRef, useState } from 'react';

import { useUIContext } from '../../hooks/useUIContext';

const API_BASE = '/api/camofox';
const CLICK_REFRESH_DELAY_MS = 300;

/**
 * Remote browser viewer — displays a Camofox tab as a live screenshot.
 * The user can click on the image to send click events to the remote browser.
 * Used for CAPTCHA solving and general browser interaction.
 *
 * For regular URLs (no camofoxTabId), renders a plain iframe.
 */
export const BrowserView = React.memo(() => {
  const { browserViewUrl, browserViewTabId, closeBrowserView } = useUIContext();
  const [screenshot, setScreenshot] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [clicking, setClicking] = useState(false);
  const [lastClick, setLastClick] = useState<{ x: number; y: number } | null>(null);
  const [status, setStatus] = useState('Loading...');
  const imgRef = useRef<HTMLImageElement>(null);
  const pollingRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Fetch screenshot from Camofox proxy
  const fetchScreenshot = useCallback(async () => {
    if (!browserViewTabId) return;
    try {
      const resp = await fetch(`${API_BASE}/tabs/${browserViewTabId}/screenshot`);
      if (!resp.ok) {
        setStatus(`Error: ${resp.status}`);
        return;
      }
      const blob = await resp.blob();
      const url = URL.createObjectURL(blob);
      setScreenshot((prev) => {
        if (prev) URL.revokeObjectURL(prev);
        return url;
      });
      setStatus('');
    } catch (e) {
      setStatus(`Connection error: ${e instanceof Error ? e.message : String(e)}`);
    }
  }, [browserViewTabId]);

  // Poll screenshots for Camofox tabs
  useEffect(() => {
    if (!browserViewTabId) return;
    setLoading(true);
    fetchScreenshot().finally(() => setLoading(false));

    pollingRef.current = setInterval(fetchScreenshot, 1000);
    return () => {
      if (pollingRef.current) clearInterval(pollingRef.current);
    };
  }, [browserViewTabId, fetchScreenshot]);

  // Cleanup object URLs on unmount
  useEffect(() => {
    return () => {
      if (screenshot) URL.revokeObjectURL(screenshot);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Handle click on the screenshot image — translate to Camofox coordinates
  const handleImageClick = useCallback(
    async (e: React.MouseEvent<HTMLImageElement>) => {
      if (!browserViewTabId || !imgRef.current) return;

      const rect = imgRef.current.getBoundingClientRect();
      const naturalW = imgRef.current.naturalWidth;
      const naturalH = imgRef.current.naturalHeight;

      // Translate display coordinates to actual page coordinates
      const scaleX = naturalW / rect.width;
      const scaleY = naturalH / rect.height;
      const x = Math.round((e.clientX - rect.left) * scaleX);
      const y = Math.round((e.clientY - rect.top) * scaleY);

      setLastClick({ x, y });
      setClicking(true);

      // Fire-and-forget — don't block UI waiting for Playwright round-trip
      fetch(`${API_BASE}/tabs/${browserViewTabId}/click`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ x, y }),
      })
        .then(() => {
          // Refresh screenshot after click completes
          setTimeout(fetchScreenshot, CLICK_REFRESH_DELAY_MS);
        })
        .catch((err) => console.error('Click failed:', err))
        .finally(() => setClicking(false));
    },
    [browserViewTabId, fetchScreenshot],
  );

  if (!browserViewUrl) return null;

  // Regular URL — use iframe
  if (!browserViewTabId) {
    return (
      <div className="flex flex-col flex-1 overflow-hidden">
        <div className="flex items-center justify-between px-4 py-2 border-b border-border bg-muted/30">
          <span className="text-xs text-muted-foreground truncate max-w-[80%]">
            {browserViewUrl}
          </span>
          <button onClick={closeBrowserView} className="btn-icon" title="Close browser view">
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
        <iframe
          src={browserViewUrl}
          className="flex-1 w-full border-none bg-white"
          sandbox="allow-scripts allow-same-origin allow-forms"
          title="Browser View"
        />
      </div>
    );
  }

  // Camofox remote viewer — screenshot + click
  return (
    <div className="flex flex-col flex-1 overflow-hidden">
      {/* Toolbar */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-border bg-muted/30">
        <div className="flex items-center gap-2 min-w-0">
          <MousePointer2 className="h-3.5 w-3.5 text-muted-foreground flex-shrink-0" />
          <span className="text-xs text-muted-foreground truncate">
            Click on the page to interact
          </span>
          {lastClick ? (
            <span className="text-xs text-muted-foreground/60">
              ({lastClick.x}, {lastClick.y})
            </span>
          ) : null}
        </div>
        <div className="flex items-center gap-1">
          <button onClick={() => fetchScreenshot()} className="btn-icon" title="Refresh screenshot">
            <RefreshCw className={`h-3.5 w-3.5 ${loading ? 'animate-spin' : ''}`} />
          </button>
          <button onClick={closeBrowserView} className="btn-icon" title="Close browser view">
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
      </div>

      {/* Screenshot area */}
      <div className="flex-1 overflow-auto flex items-start justify-center p-4 bg-background/50">
        {status && !screenshot ? (
          <div className="text-sm text-muted-foreground mt-20">{status}</div>
        ) : null}
        {screenshot ? (
          <div className="relative">
            {/* eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions, jsx-a11y/click-events-have-key-events */}
            <img
              ref={imgRef}
              src={screenshot}
              alt="Remote browser"
              className={`max-w-full max-h-[calc(100vh-12rem)] object-contain rounded-lg border border-border shadow-lg ${clicking ? 'opacity-80' : 'cursor-crosshair'}`}
              onClick={handleImageClick}
              draggable={false}
            />
            {/* Click ripple effect */}
            {lastClick && clicking ? (
              <div
                className="absolute w-6 h-6 -ml-3 -mt-3 rounded-full bg-primary/40 animate-ping pointer-events-none"
                style={{
                  left: `${(lastClick.x / (imgRef.current?.naturalWidth || 1)) * 100}%`,
                  top: `${(lastClick.y / (imgRef.current?.naturalHeight || 1)) * 100}%`,
                }}
              />
            ) : null}
          </div>
        ) : null}
      </div>
    </div>
  );
});
BrowserView.displayName = 'BrowserView';
