import { useCallback, useEffect, useRef } from 'react';

import { useUIContext } from './useUIContext';

/**
 * Polls /api/camofox/status to detect CAPTCHA tabs.
 * When a CAPTCHA tab appears, auto-opens the browser view.
 * When the CAPTCHA tab disappears (solved), auto-closes.
 */
export function useCamofoxCaptcha() {
  const { browserViewTabId, openBrowserView, closeBrowserView } = useUIContext();
  const lastTabIdRef = useRef<string | null>(null);

  const checkStatus = useCallback(async () => {
    try {
      const resp = await fetch('/api/camofox/status');
      if (!resp.ok) return;
      const data = await resp.json();
      const tabId: string | null = data.captcha_tab_id ?? null;

      if (tabId && tabId !== lastTabIdRef.current) {
        // New CAPTCHA detected — auto-open browser view
        lastTabIdRef.current = tabId;
        openBrowserView('CAPTCHA — click to solve', tabId);
      } else if (!tabId && lastTabIdRef.current) {
        // CAPTCHA solved — auto-close
        lastTabIdRef.current = null;
        if (browserViewTabId) {
          closeBrowserView();
        }
      }
    } catch {
      // Server not reachable — ignore
    }
  }, [browserViewTabId, openBrowserView, closeBrowserView]);

  useEffect(() => {
    const POLL_INTERVAL_MS = 2000;
    const interval = setInterval(checkStatus, POLL_INTERVAL_MS);
    return () => clearInterval(interval);
  }, [checkStatus]);
}
