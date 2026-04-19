import { X, RefreshCw, MousePointer2, ArrowRight, Globe } from 'lucide-react';
import React, { useCallback, useEffect, useRef, useState } from 'react';

import { useUIContext } from '../../hooks/useUIContext';

const API_BASE = '/api/camofox';
const CLICK_REFRESH_DELAY_MS = 300;
const SCREENSHOT_POLL_MS = 1000;

/**
 * In-app browser view — displays a Camofox tab as a live screenshot with URL bar.
 * The user (and agent) can navigate to any URL, click on the page, and browse.
 * Used for CAPTCHA solving and general in-app web browsing.
 */
// eslint-disable-next-line max-lines-per-function
export const BrowserView = React.memo(() => {
  const { browserViewUrl, browserViewTabId, openBrowserView, closeBrowserView } = useUIContext();
  const [screenshot, setScreenshot] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [clicking, setClicking] = useState(false);
  const [lastClick, setLastClick] = useState<{ x: number; y: number } | null>(null);
  const [status, setStatus] = useState('');
  const [urlInput, setUrlInput] = useState(browserViewUrl ?? '');
  const [navigating, setNavigating] = useState(false);
  const imgRef = useRef<HTMLImageElement>(null);
  const pollingRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Keep URL input in sync when external state changes
  useEffect(() => {
    if (browserViewUrl) setUrlInput(browserViewUrl);
  }, [browserViewUrl]);

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

  // Poll screenshots when we have a Camofox tab
  useEffect(() => {
    if (!browserViewTabId) return undefined;
    setLoading(true);
    fetchScreenshot().finally(() => setLoading(false));
    pollingRef.current = setInterval(fetchScreenshot, SCREENSHOT_POLL_MS);
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

  // Navigate to a new URL — create a fresh Camofox tab
  const navigateToUrl = useCallback(
    async (rawUrl: string) => {
      const url = rawUrl.trim();
      if (!url) return;
      // Add https:// if missing
      const fullUrl =
        url.startsWith('http://') || url.startsWith('https://') ? url : `https://${url}`;
      setNavigating(true);
      setStatus('Loading...');
      try {
        const resp = await fetch(`${API_BASE}/tabs`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ url: fullUrl }),
        });
        if (!resp.ok) {
          setStatus(`Navigation failed: ${resp.status}`);
          return;
        }
        const data = await resp.json();
        const tabId = data.tabId || data.tab_id;
        if (tabId) {
          openBrowserView(fullUrl, tabId);
          setUrlInput(fullUrl);
        } else {
          setStatus('No tab ID returned');
        }
      } catch (e) {
        setStatus(`Navigation error: ${e instanceof Error ? e.message : String(e)}`);
      } finally {
        setNavigating(false);
      }
    },
    [openBrowserView],
  );

  // Handle click on the screenshot image — translate to Camofox coordinates
  const handleImageClick = useCallback(
    (e: React.MouseEvent<HTMLImageElement>) => {
      if (!browserViewTabId || !imgRef.current) return;

      const rect = imgRef.current.getBoundingClientRect();
      const naturalW = imgRef.current.naturalWidth;
      const naturalH = imgRef.current.naturalHeight;
      const scaleX = naturalW / rect.width;
      const scaleY = naturalH / rect.height;
      const x = Math.round((e.clientX - rect.left) * scaleX);
      const y = Math.round((e.clientY - rect.top) * scaleY);

      setLastClick({ x, y });
      setClicking(true);

      fetch(`${API_BASE}/tabs/${browserViewTabId}/click`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ x, y }),
      })
        .then(() => {
          setTimeout(fetchScreenshot, CLICK_REFRESH_DELAY_MS);
        })
        .catch((err) => console.error('Click failed:', err))
        .finally(() => setClicking(false));
    },
    [browserViewTabId, fetchScreenshot],
  );

  const handleUrlKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      navigateToUrl(urlInput);
    }
  };

  return (
    <div className="flex flex-col flex-1 overflow-hidden">
      {/* URL bar + controls */}
      <div className="flex items-center gap-2 px-3 py-2 border-b border-border bg-muted/30">
        <Globe className="h-3.5 w-3.5 text-muted-foreground flex-shrink-0" />
        <input
          type="text"
          value={urlInput}
          onChange={(e) => setUrlInput(e.target.value)}
          onKeyDown={handleUrlKeyDown}
          placeholder="Enter URL or search query..."
          className="flex-1 bg-background border border-border rounded px-2 py-1 text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
          disabled={navigating}
        />
        <button
          onClick={() => navigateToUrl(urlInput)}
          className="btn-icon"
          title="Navigate"
          disabled={navigating || !urlInput.trim()}
        >
          <ArrowRight className={`h-3.5 w-3.5 ${navigating ? 'animate-pulse' : ''}`} />
        </button>
        {browserViewTabId ? (
          <button onClick={() => fetchScreenshot()} className="btn-icon" title="Refresh screenshot">
            <RefreshCw className={`h-3.5 w-3.5 ${loading ? 'animate-spin' : ''}`} />
          </button>
        ) : null}
        <button onClick={closeBrowserView} className="btn-icon" title="Close browser view">
          <X className="h-3.5 w-3.5" />
        </button>
      </div>

      {/* Hint row when viewing a tab */}
      {browserViewTabId ? (
        <div className="flex items-center gap-2 px-4 py-1 border-b border-border bg-muted/20">
          <MousePointer2 className="h-3 w-3 text-muted-foreground flex-shrink-0" />
          <span className="text-[11px] text-muted-foreground">Click on the page to interact</span>
          {lastClick ? (
            <span className="text-[11px] text-muted-foreground/60">
              ({lastClick.x}, {lastClick.y})
            </span>
          ) : null}
        </div>
      ) : null}

      {/* Content area */}
      <div className="flex-1 overflow-auto flex items-start justify-center p-4 bg-background/50">
        {!browserViewTabId && !screenshot ? (
          <div className="text-center mt-20 text-muted-foreground">
            <Globe className="h-10 w-10 mx-auto mb-3 opacity-30" />
            <p className="text-sm">Enter a URL above to start browsing</p>
            <p className="text-xs mt-1 opacity-60">
              Uses Camofox (anti-detection Firefox) — pages rendered remotely
            </p>
          </div>
        ) : null}
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
              className={`max-w-full max-h-[calc(100vh-14rem)] object-contain rounded-lg border border-border shadow-lg ${clicking ? 'opacity-80' : 'cursor-crosshair'}`}
              onClick={handleImageClick}
              draggable={false}
            />
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
