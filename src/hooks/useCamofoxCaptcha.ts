import { useCallback, useEffect, useRef } from 'react';

import { useUIContext } from './useUIContext';

/**
 * Polls /api/camofox/status to detect CAPTCHA tabs.
 * When a CAPTCHA tab appears, auto-opens the browser view.
 * When the CAPTCHA tab disappears (solved), auto-closes.
 */
export function useCamofoxCaptcha() {
  const { openBrowserView } = useUIContext();
  const lastTabIdRef = useRef<string | null>(null);

  const checkStatus = useCallback(async () => {
    try {
      const resp = await fetch('/api/camofox/status');
      if (!resp.ok) return;
      const data = await resp.json();
      const captchaTabId: string | null = data.captcha_tab_id ?? null;
      const agentTabUrl: string | null = data.agent_tab_url ?? null;

      // Auto-open for CAPTCHAs
      if (captchaTabId && captchaTabId !== lastTabIdRef.current) {
        lastTabIdRef.current = captchaTabId;
        openBrowserView('CAPTCHA — click to solve');
        return;
      }
      // Auto-open for agent tabs — shows the same URL in the Tauri webview
      if (agentTabUrl && agentTabUrl !== lastTabIdRef.current) {
        lastTabIdRef.current = agentTabUrl;
        openBrowserView(agentTabUrl);
        return;
      }
      // Clear when no active tabs
      if (!captchaTabId && !agentTabUrl && lastTabIdRef.current) {
        lastTabIdRef.current = null;
      }
    } catch {
      // Server not reachable — ignore
    }
  }, [openBrowserView]);

  useEffect(() => {
    const POLL_INTERVAL_MS = 2000;
    const interval = setInterval(checkStatus, POLL_INTERVAL_MS);
    return () => clearInterval(interval);
  }, [checkStatus]);
}
