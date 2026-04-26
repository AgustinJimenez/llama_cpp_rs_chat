import {
  RefreshCw,
  ArrowRight,
  Globe,
  ExternalLink,
  ChevronLeft,
  ChevronRight,
  MessageSquare,
} from 'lucide-react';
import React, { useEffect, useRef, useState } from 'react';

import { useUIContext } from '../../hooks/useUIContext';
import { isTauriEnv } from '../../utils/tauri';

const TAURI = isTauriEnv();

async function tauriInvoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

/**
 * Real iframe-based browser view. The user interacts with the page natively
 * (clicks, types, scrolls — same as any browser tab).
 *
 * Limitation: sites that send `X-Frame-Options: DENY` or strict CSP cannot
 * be embedded (Google, Twitter, Facebook, banks). For those we offer an
 * "Open in new tab" fallback link.
 */
/* eslint-disable max-lines-per-function, no-nested-ternary */
export const BrowserView = React.memo(() => {
  const { browserViewUrl, openBrowserView, closeBrowserView, isBrowserViewOpen } = useUIContext();
  const [urlInput, setUrlInput] = useState(browserViewUrl ?? '');
  const [iframeKey, setIframeKey] = useState(0);
  const [loadFailed, setLoadFailed] = useState(false);
  const [history, setHistory] = useState<string[]>(browserViewUrl ? [browserViewUrl] : []);
  const [historyIdx, setHistoryIdx] = useState(browserViewUrl ? 0 : -1);
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const skipHistoryPushRef = useRef(false);
  const panelOpenedRef = useRef(false);

  // Keep URL input + history in sync when external state changes
  useEffect(() => {
    if (!browserViewUrl) return;
    setUrlInput(browserViewUrl);
    setLoadFailed(false);
    setIframeKey((k) => k + 1);
    if (skipHistoryPushRef.current) {
      skipHistoryPushRef.current = false;
      return;
    }
    setHistory((prev) => {
      // Don't duplicate if it matches current entry
      if (prev[historyIdx] === browserViewUrl) return prev;
      // Truncate forward history when navigating to a new URL
      const trimmed = prev.slice(0, historyIdx + 1);
      return [...trimmed, browserViewUrl];
    });
    setHistoryIdx((prev) => {
      // If URL matched the current entry, idx unchanged; otherwise advance
      return prev + 1;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [browserViewUrl]);

  const canGoBack = historyIdx > 0;
  const canGoForward = historyIdx >= 0 && historyIdx < history.length - 1;

  const goBack = () => {
    if (!canGoBack) return;
    const newIdx = historyIdx - 1;
    setHistoryIdx(newIdx);
    skipHistoryPushRef.current = true;
    openBrowserView(history[newIdx]);
  };

  const goForward = () => {
    if (!canGoForward) return;
    const newIdx = historyIdx + 1;
    setHistoryIdx(newIdx);
    skipHistoryPushRef.current = true;
    openBrowserView(history[newIdx]);
  };

  // ─── Tauri native panel lifecycle ───
  // In Tauri mode, attach a real native webview as a child of the main window,
  // positioned to overlay the panel placeholder. ResizeObserver keeps it in sync.
  useEffect(() => {
    if (!TAURI || !browserViewUrl || !isBrowserViewOpen || !panelRef.current) return undefined;
    let cancelled = false;

    const sendRect = async (open: boolean) => {
      const el = panelRef.current;
      if (!el) return;
      const r = el.getBoundingClientRect();
      const args = {
        url: browserViewUrl,
        x: Math.round(r.left),
        y: Math.round(r.top),
        width: Math.round(r.width),
        height: Math.round(r.height),
      };
      try {
        if (open && !panelOpenedRef.current) {
          await tauriInvoke('browser_panel_open', args);
          panelOpenedRef.current = true;
        } else if (panelOpenedRef.current) {
          // Already open — either navigate or just resize
          if (open) {
            await tauriInvoke('browser_panel_navigate', { url: browserViewUrl });
          }
          await tauriInvoke('browser_panel_resize', {
            x: args.x,
            y: args.y,
            width: args.width,
            height: args.height,
          });
        }
      } catch (e) {
        if (!cancelled) console.error('browser panel:', e);
      }
    };

    sendRect(true);
    const ro = new ResizeObserver(() => sendRect(false));
    ro.observe(panelRef.current);
    const onScroll = () => sendRect(false);
    window.addEventListener('resize', onScroll);
    window.addEventListener('scroll', onScroll, true);

    return () => {
      cancelled = true;
      ro.disconnect();
      window.removeEventListener('resize', onScroll);
      window.removeEventListener('scroll', onScroll, true);
    };
  }, [browserViewUrl, isBrowserViewOpen]);

  // Close the Tauri panel when the browser view is hidden or cleared.
  useEffect(() => {
    if ((!browserViewUrl || !isBrowserViewOpen) && TAURI && panelOpenedRef.current) {
      tauriInvoke('browser_panel_close').catch(() => {});
      panelOpenedRef.current = false;
    }
  }, [browserViewUrl, isBrowserViewOpen]);

  // Detect iframes that fail to load (X-Frame-Options / CSP)
  useEffect(() => {
    if (!browserViewUrl) return undefined;
    setLoadFailed(false);
    // Heuristic: if iframe doesn't fire `load` within 5s, treat as blocked.
    // (X-Frame-Options doesn't fire onerror — the request just hangs.)
    const LOAD_CHECK_MS = 5000;
    const timer = setTimeout(() => {
      try {
        void iframeRef.current?.contentWindow?.location?.href;
        setLoadFailed(false);
      } catch {
        setLoadFailed(false);
      }
    }, LOAD_CHECK_MS);
    return () => clearTimeout(timer);
  }, [browserViewUrl, iframeKey]);

  const navigateToUrl = (rawUrl: string) => {
    const url = rawUrl.trim();
    if (!url) return;
    const fullUrl =
      url.startsWith('http://') || url.startsWith('https://') ? url : `https://${url}`;
    // Force navigation even if URL looks the same (user may want to reload)
    if (fullUrl === browserViewUrl && TAURI && panelOpenedRef.current) {
      tauriInvoke('browser_panel_navigate', { url: fullUrl }).catch(() => {});
    }
    openBrowserView(fullUrl);
  };

  const handleUrlKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      navigateToUrl(urlInput);
    }
  };

  const handleReload = () => setIframeKey((k) => k + 1);

  const handleIframeLoad = () => setLoadFailed(false);
  const handleIframeError = () => setLoadFailed(true);

  return (
    <div className="flex flex-col flex-1 overflow-hidden">
      {/* URL bar */}
      <div className="flex items-center gap-1 px-3 py-2 border-b border-border bg-muted/30">
        <button
          onClick={goBack}
          className={`p-1.5 rounded-md transition-colors ${
            canGoBack
              ? 'text-foreground hover:bg-muted'
              : 'text-muted-foreground/30 cursor-not-allowed'
          }`}
          title="Back"
          disabled={!canGoBack}
          aria-label="Back"
        >
          <ChevronLeft className="h-4 w-4" />
        </button>
        <button
          onClick={goForward}
          className={`p-1.5 rounded-md transition-colors ${
            canGoForward
              ? 'text-foreground hover:bg-muted'
              : 'text-muted-foreground/30 cursor-not-allowed'
          }`}
          title="Forward"
          disabled={!canGoForward}
          aria-label="Forward"
        >
          <ChevronRight className="h-4 w-4" />
        </button>
        <Globe className="h-3.5 w-3.5 text-muted-foreground flex-shrink-0 ml-1" />
        <input
          type="text"
          value={urlInput}
          onChange={(e) => setUrlInput(e.target.value)}
          onKeyDown={handleUrlKeyDown}
          placeholder="Enter URL..."
          className="flex-1 bg-background border border-border rounded px-2 py-1 text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
        />
        <button
          onClick={() => navigateToUrl(urlInput)}
          className="btn-icon"
          title="Navigate"
          disabled={!urlInput.trim()}
        >
          <ArrowRight className="h-3.5 w-3.5" />
        </button>
        {browserViewUrl ? (
          <button onClick={handleReload} className="btn-icon" title="Reload">
            <RefreshCw className="h-3.5 w-3.5" />
          </button>
        ) : null}
        <button
          onClick={() => {
            closeBrowserView();
          }}
          className="p-1.5 rounded-md text-foreground hover:bg-muted transition-colors"
          title="Back to chat"
        >
          <MessageSquare className="h-4 w-4" />
        </button>
      </div>

      {/* Content area */}
      <div className="flex-1 overflow-hidden bg-background">
        {!browserViewUrl ? (
          <div className="h-full flex items-center justify-center text-foreground">
            <div className="text-center">
              <Globe className="h-10 w-10 mx-auto mb-3 opacity-70" />
              <p className="text-sm">Enter a URL above to start browsing</p>
              <p className="text-xs mt-1 text-muted-foreground">
                Real iframe — interact normally with the page
              </p>
            </div>
          </div>
        ) : TAURI ? (
          // Tauri: native webview overlay positioned over this placeholder
          <div ref={panelRef} className="w-full h-full bg-background" />
        ) : (
          <>
            <iframe
              key={iframeKey}
              ref={iframeRef}
              src={browserViewUrl}
              onLoad={handleIframeLoad}
              onError={handleIframeError}
              className="w-full h-full border-none bg-white"
              sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-popups-to-escape-sandbox allow-modals"
              title="Browser View"
            />
            {loadFailed ? (
              <div className="absolute inset-0 flex items-center justify-center bg-background/95 text-foreground p-6">
                <div className="text-center max-w-md">
                  <p className="text-sm mb-3">
                    This site refuses to be embedded (X-Frame-Options or CSP).
                  </p>
                  <a
                    href={browserViewUrl}
                    target="_blank"
                    rel="noreferrer"
                    className="inline-flex items-center gap-1.5 text-sm text-primary hover:underline"
                  >
                    <ExternalLink className="h-4 w-4" />
                    Open in new tab
                  </a>
                </div>
              </div>
            ) : null}
          </>
        )}
      </div>
    </div>
  );
});
BrowserView.displayName = 'BrowserView';
