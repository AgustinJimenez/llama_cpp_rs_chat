import {
  RefreshCw,
  ArrowRight,
  Globe,
  ChevronLeft,
  ChevronRight,
  ZoomIn,
  ZoomOut,
  Search,
  X,
} from 'lucide-react';
import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { useUIContext } from '../../hooks/useUIContext';
import { isTauriEnv } from '../../utils/tauri';

const TAURI = isTauriEnv();
// Web-mode backend (port 18080) — used to open the wry browser window when not in Tauri.
const WEB_BACKEND = 'http://127.0.0.1:18080';

async function tauriInvoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

/**
 * Browser view panel.
 *
 * Desktop (Tauri): a native WebView2 child window overlaid inside the app.
 * Both the user and the agent share the same `browser-panel` WebView — the user
 * sees what the agent is browsing in real time. Google works fine (no bot detection).
 *
 * Web/server mode: iframes cannot be used because sites like Google block embedding
 * via X-Frame-Options. Instead, navigating posts to the backend which opens a standalone
 * wry (WebView2) window — a real isolated browser, same engine, no bot detection.
 * The panel shows which URL is open and lets the user control navigation.
 */
/* eslint-disable max-lines-per-function */
export const BrowserView = React.memo(() => {
  const { t } = useTranslation();
  const { browserViewUrl, openBrowserView, isBrowserViewOpen } = useUIContext();
  const [urlInput, setUrlInput] = useState(browserViewUrl ?? '');
  const [history, setHistory] = useState<string[]>(browserViewUrl ? [browserViewUrl] : []);
  const [historyIdx, setHistoryIdx] = useState(browserViewUrl ? 0 : -1);
  const panelRef = useRef<HTMLDivElement>(null);
  const skipHistoryPushRef = useRef(false);
  const panelOpenedRef = useRef(false);
  const pendingNavigateRef = useRef(false);

  // Open Google as default page when browser view opens with no URL
  useEffect(() => {
    if (isBrowserViewOpen && !browserViewUrl) {
      openBrowserView('https://www.google.com');
    }
  }, [isBrowserViewOpen, browserViewUrl, openBrowserView]);

  // Keep URL input + history in sync when external state changes
  // react-doctor-disable-next-line react-doctor/no-cascading-set-state -- related navigation state
  useEffect(() => {
    if (!browserViewUrl) return;
    // External URL change (agent navigation) — navigate the panel
    if (TAURI && panelOpenedRef.current) pendingNavigateRef.current = true;
    setUrlInput(browserViewUrl);
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
    setHistoryIdx((prev) => prev + 1);
    if (TAURI && historyIdx >= 0) {
      setHasGoBack(true);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [browserViewUrl]);

  // In Tauri mode, back is always available (native webview tracks its own history).
  // Forward is only available after going back. We track this with a simple flag.
  const [hasGoBack, setHasGoBack] = useState(false);
  const [hasGoForward, setHasGoForward] = useState(false);
  const canGoBack = TAURI ? isBrowserViewOpen && hasGoBack : historyIdx > 0;
  const canGoForward = TAURI
    ? isBrowserViewOpen && hasGoForward
    : historyIdx >= 0 && historyIdx < history.length - 1;

  const goBack = () => {
    if (TAURI && panelOpenedRef.current) {
      tauriInvoke('browser_panel_go_back').catch(() => {});
      showLoading();
      setHasGoForward(true); // After going back, forward is available
      return;
    }
    if (historyIdx <= 0) return;
    const newIdx = historyIdx - 1;
    setHistoryIdx(newIdx);
    skipHistoryPushRef.current = true;
    openBrowserView(history[newIdx]);
  };

  const goForward = () => {
    if (TAURI && panelOpenedRef.current) {
      tauriInvoke('browser_panel_go_forward').catch(() => {});
      showLoading();
      setHasGoForward(false); // After going forward, no more forward
      return;
    }
    if (historyIdx >= history.length - 1) return;
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
          // First time — create the webview
          await tauriInvoke('browser_panel_open', args);
          panelOpenedRef.current = true;
        } else if (panelOpenedRef.current) {
          // Already exists — navigate only if explicitly requested (URL bar),
          // otherwise just resize (re-show from hidden preserves current page).
          if (pendingNavigateRef.current) {
            pendingNavigateRef.current = false;
            await tauriInvoke('browser_panel_navigate', { url: browserViewUrl });
          }
          await tauriInvoke('browser_panel_resize', {
            x: args.x,
            y: args.y,
            width: args.width,
            height: args.height,
          });
        }
      } catch (error) {
        if (!cancelled) console.error('browser panel:', error);
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

  // Hide/show the Tauri panel when the browser view is toggled.
  // Only destroy on explicit URL clear — toggling just hides it to preserve navigation.
  useEffect(() => {
    if (!TAURI || !panelOpenedRef.current) return;
    if (!browserViewUrl) {
      // URL cleared — destroy the panel
      tauriInvoke('browser_panel_close').catch(() => {});
      panelOpenedRef.current = false;
    } else if (!isBrowserViewOpen) {
      // Just hidden — move off-screen to hide without destroying
      tauriInvoke('browser_panel_resize', { x: -9999, y: -9999, width: 1, height: 1 }).catch(
        () => {},
      );
    }
  }, [browserViewUrl, isBrowserViewOpen]);

  // ─── Web mode: open wry browser window via backend ───
  // In web/server mode (no Tauri), navigation is sent to the backend which opens
  // a standalone wry (WebView2) window — a real browser, not an iframe.
  const webNavigate = useCallback(async (url: string) => {
    try {
      await fetch(`${WEB_BACKEND}/api/browser/navigate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ url }),
      });
    } catch {
      // Backend not reachable — ignore
    }
  }, []);

  const navigateToUrl = (rawUrl: string) => {
    const url = rawUrl.trim();
    if (!url) return;
    const fullUrl =
      url.startsWith('http://') || url.startsWith('https://') ? url : `https://${url}`;

    if (!TAURI) {
      // Web mode: tell backend to open/navigate the wry browser window
      openBrowserView(fullUrl);
      webNavigate(fullUrl);
      showLoading();
      return;
    }

    // Force navigation even if URL looks the same (user may want to reload)
    if (fullUrl === browserViewUrl && panelOpenedRef.current) {
      tauriInvoke('browser_panel_navigate', { url: fullUrl }).catch(() => {});
    }
    if (browserViewUrl) setHasGoBack(true);
    setHasGoForward(false);
    pendingNavigateRef.current = true; // Tell the effect to navigate
    openBrowserView(fullUrl);
  };

  // Web mode: when browserViewUrl changes (e.g. agent navigation), open wry window
  useEffect(() => {
    if (TAURI || !browserViewUrl || !isBrowserViewOpen) return;
    webNavigate(browserViewUrl);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [browserViewUrl, isBrowserViewOpen]);

  const handleUrlKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      navigateToUrl(urlInput);
    }
  };

  const handleReload = () => {
    if (TAURI && panelOpenedRef.current) {
      // Reload the webview's actual current page (not React's browserViewUrl,
      // which may be stale if the user clicked links inside the webview)
      tauriInvoke('browser_panel_reload').catch(() => {});
      showLoading();
    } else if (!TAURI) {
      // In web mode re-navigate the wry window
      if (browserViewUrl) webNavigate(browserViewUrl);
      showLoading();
    }
  };

  // Loading indicator — shows briefly when navigating
  const [isPageLoading, setIsPageLoading] = useState(false);
  const loadingTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const showLoading = () => {
    setIsPageLoading(true);
    if (loadingTimerRef.current) clearTimeout(loadingTimerRef.current);
    const LOADING_DISPLAY_MS = 3000;
    loadingTimerRef.current = setTimeout(() => setIsPageLoading(false), LOADING_DISPLAY_MS);
  };
  // react-doctor-disable-next-line react-doctor/no-effect-event-handler
  useEffect(() => {
    if (browserViewUrl) showLoading();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [browserViewUrl]);

  // ─── URL bar sync: poll webview for actual current URL ───
  const URL_POLL_MS = 2000;
  useEffect(() => {
    if (!TAURI || !isBrowserViewOpen || !panelOpenedRef.current) return undefined;
    const poll = async () => {
      try {
        const info = await tauriInvoke<{ url: string }>('browser_panel_get_info');
        if (info.url && info.url !== 'about:blank') {
          setUrlInput(info.url);
        }
      } catch {
        /* panel not open */
      }
    };
    const interval = setInterval(poll, URL_POLL_MS);
    poll(); // immediate first poll
    return () => clearInterval(interval);
  }, [isBrowserViewOpen]);

  // ─── Zoom state ───
  const [zoomLevel, setZoomLevel] = useState(1.0);
  const ZOOM_STEP = 0.1;
  const ZOOM_MIN = 0.25;
  const ZOOM_MAX = 3.0;
  const handleZoomIn = useCallback(async () => {
    if (!TAURI) return;
    const newZoom = Math.min(zoomLevel + ZOOM_STEP, ZOOM_MAX);
    setZoomLevel(newZoom);
    await tauriInvoke('browser_panel_set_zoom', { zoom: newZoom }).catch(() => {});
  }, [zoomLevel]);
  const handleZoomOut = useCallback(async () => {
    if (!TAURI) return;
    const newZoom = Math.max(zoomLevel - ZOOM_STEP, ZOOM_MIN);
    setZoomLevel(newZoom);
    await tauriInvoke('browser_panel_set_zoom', { zoom: newZoom }).catch(() => {});
  }, [zoomLevel]);
  const handleZoomReset = useCallback(async () => {
    if (!TAURI) return;
    setZoomLevel(1.0);
    await tauriInvoke('browser_panel_set_zoom', { zoom: 1.0 }).catch(() => {});
  }, []);

  // ─── Find in page (JS-based) ───
  const [showFind, setShowFind] = useState(false);
  const [findQuery, setFindQuery] = useState('');
  const findInputRef = useRef<HTMLInputElement>(null);
  const handleFind = useCallback(async () => {
    if (!TAURI || !findQuery.trim()) return;
    const escaped = findQuery.replaceAll('\\', '\\\\').replaceAll('\'', "\\'");
    await tauriInvoke('browser_panel_go_back').catch(() => {}); // dummy to check panel exists
    // Use window.find() — built-in browser search
    const js = `window.find('${escaped}', false, false, true)`;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('browser_panel_eval_js', { js });
    } catch {
      // Fallback: try via the MCP eval endpoint
      fetch('http://127.0.0.1:18091/api/eval', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ js, target: 'browser-panel' }),
      }).catch(() => {});
    }
  }, [findQuery]);

  let contentArea: React.ReactNode;
  if (!browserViewUrl) {
    contentArea = (
      <div className="flex h-full items-center justify-center text-foreground">
        <div className="text-center">
          <Globe className="mx-auto mb-3 size-10 opacity-70" />
          <p className="text-sm">{t('browserView.enterUrl')}</p>
          <p className="mt-1 text-xs text-muted-foreground">
            {!!TAURI && t('browserView.webviewInside')}
            {!TAURI && t('browserView.separateWindow')}
          </p>
        </div>
      </div>
    );
  } else if (TAURI) {
    // Tauri: native webview overlay positioned over this placeholder
    contentArea = <div ref={panelRef} className="h-full w-full bg-background" />;
  } else {
    // Web mode: wry browser window is open separately — show URL + status here
    contentArea = (
      <div className="flex h-full flex-col items-center justify-center gap-3 p-6 text-foreground">
        <Globe className="size-10 opacity-60" />
        <p className="text-sm font-medium">{t('browserView.windowOpened')}</p>
        <p className="max-w-xs break-all text-center text-xs text-muted-foreground">
          {browserViewUrl}
        </p>
        <p className="text-center text-xs text-muted-foreground">
          {t('browserView.windowDescription')}
        </p>
      </div>
    );
  }

  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      {/* Loading bar */}
      {!!isPageLoading && (
        <div className="h-0.5 overflow-hidden bg-primary/30">
          <div
            className="h-full animate-pulse bg-primary"
            style={{ width: '60%', animation: 'loading-bar 0.8s ease-in-out infinite' }}
          />
        </div>
      )}
      {/* URL bar */}
      <div className="flex items-center gap-1 border-b border-border bg-muted/30 px-3 py-2">
        <button
          onClick={goBack}
          className={`rounded-md p-1.5 transition-colors ${
            canGoBack
              ? 'text-foreground hover:bg-muted'
              : 'cursor-not-allowed text-muted-foreground/30'
          }`}
          title={t('browserView.backTitle')}
          disabled={!canGoBack}
          aria-label={t('browserView.backTitle')}
        >
          <ChevronLeft className="size-4" />
        </button>
        <button
          onClick={goForward}
          className={`rounded-md p-1.5 transition-colors ${
            canGoForward
              ? 'text-foreground hover:bg-muted'
              : 'cursor-not-allowed text-muted-foreground/30'
          }`}
          title={t('browserView.forwardTitle')}
          disabled={!canGoForward}
          aria-label={t('browserView.forwardTitle')}
        >
          <ChevronRight className="size-4" />
        </button>
        <Globe className="ml-1 size-3.5 flex-shrink-0 text-muted-foreground" />
        <input
          type="text"
          value={urlInput}
          onChange={(e) => setUrlInput(e.target.value)}
          onKeyDown={handleUrlKeyDown}
          placeholder={t('browserView.urlPlaceholder')}
          className="flex-1 rounded border border-border bg-background px-2 py-1 text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
        />
        <button
          onClick={() => navigateToUrl(urlInput)}
          className="btn-icon"
          title={t('browserView.navigateTitle')}
          disabled={!urlInput.trim()}
        >
          <ArrowRight className="size-3.5" />
        </button>
        {!!browserViewUrl && (
          <>
            <button
              onClick={handleReload}
              className="btn-icon"
              title={t('browserView.reloadTitle')}
            >
              <RefreshCw className="size-3.5" />
            </button>
            {!!TAURI && (
              <>
                <div className="mx-1 h-4 border-l border-border" />
                <button
                  onClick={handleZoomOut}
                  className="btn-icon"
                  title={t('browserView.zoomOutTitle')}
                >
                  <ZoomOut className="size-3.5" />
                </button>
                <button
                  onClick={handleZoomReset}
                  className="min-w-[3rem] px-1 text-center text-xs text-muted-foreground hover:text-foreground"
                  title={t('browserView.resetZoomTitle')}
                >
                  {Math.round(zoomLevel * 100)}%
                </button>
                <button
                  onClick={handleZoomIn}
                  className="btn-icon"
                  title={t('browserView.zoomInTitle')}
                >
                  <ZoomIn className="size-3.5" />
                </button>
                <div className="mx-1 h-4 border-l border-border" />
                <button
                  onClick={() => {
                    setShowFind((p) => !p);
                    if (!showFind) setTimeout(() => findInputRef.current?.focus(), 100);
                  }}
                  className="btn-icon"
                  title={t('browserView.findInPageTitle')}
                >
                  <Search className="size-3.5" />
                </button>
              </>
            )}
          </>
        )}
      </div>
      {/* Find bar (Tauri only) */}
      {!!TAURI && !!showFind && (
        <div className="flex items-center gap-2 border-b border-border bg-muted/50 px-3 py-1.5">
          <Search className="size-3.5 text-muted-foreground" />
          <input
            ref={findInputRef}
            type="text"
            value={findQuery}
            onChange={(e) => setFindQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') handleFind();
              if (e.key === 'Escape') setShowFind(false);
            }}
            placeholder={t('browserView.findPlaceholder')}
            className="flex-1 bg-transparent text-sm text-foreground outline-none placeholder:text-muted-foreground"
          />
          <button
            onClick={handleFind}
            className="btn-icon text-xs"
            title={t('browserView.findNextTitle')}
          >
            <ArrowRight className="size-3" />
          </button>
          <button
            onClick={() => setShowFind(false)}
            className="btn-icon"
            title={t('browserView.closeFindTitle')}
          >
            <X className="size-3" />
          </button>
        </div>
      )}

      {/* Content area */}
      <div className="flex-1 overflow-hidden bg-background">{contentArea}</div>
    </div>
  );
});
BrowserView.displayName = 'BrowserView';
