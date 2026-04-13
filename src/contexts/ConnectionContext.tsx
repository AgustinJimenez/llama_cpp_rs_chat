import React, { useEffect, useMemo, useRef, useState } from 'react';

import { isTauriEnv } from '../utils/tauri';

import { ConnectionContext } from './connectionState';
import type { ConnectionState } from './connectionState';

const WS_RECONNECT_BASE_MS = 500;
const WS_RECONNECT_MAX_MS = 5000;

function getReconnectDelay(attempt: number): number {
  const base = Math.min(WS_RECONNECT_BASE_MS * Math.pow(2, attempt), WS_RECONNECT_MAX_MS);
  const JITTER_MIN = 0.5;
  return base * (JITTER_MIN + Math.random()); // jitter to prevent thundering herd
}

export const ConnectionProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  // Optimistic: assume connected until proven otherwise
  const [state, setState] = useState<ConnectionState>({
    connected: true,
    reconnecting: false,
    attempt: 0,
    disconnectedAt: null,
  });
  const attemptRef = useRef(0);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    // In Tauri desktop mode, backend is always local — skip WebSocket health check
    if (isTauriEnv()) return;

    mountedRef.current = true;

    function connect() {
      if (!mountedRef.current) return;

      const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
      const ws = new WebSocket(`${protocol}//${window.location.host}/ws/status`);
      wsRef.current = ws;

      ws.onopen = () => {
        if (!mountedRef.current) return;
        attemptRef.current = 0;
        setState({ connected: true, reconnecting: false, attempt: 0, disconnectedAt: null });
      };

      ws.onclose = () => {
        if (!mountedRef.current) return;
        wsRef.current = null;
        setState((prev) => ({
          connected: false,
          reconnecting: true,
          attempt: attemptRef.current,
          disconnectedAt: prev.disconnectedAt ?? Date.now(),
        }));
        scheduleReconnect();
      };

      ws.onerror = () => {
        // onclose will fire after onerror — reconnect handled there
      };
    }

    function scheduleReconnect() {
      if (!mountedRef.current) return;
      const delay = getReconnectDelay(attemptRef.current);
      attemptRef.current += 1;
      timerRef.current = setTimeout(connect, delay);
    }

    connect();

    return () => {
      mountedRef.current = false;
      if (timerRef.current) clearTimeout(timerRef.current);
      if (wsRef.current) {
        wsRef.current.onclose = null; // prevent reconnect on intentional close
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, []);

  const value = useMemo(() => state, [state]);

  return <ConnectionContext.Provider value={value}>{children}</ConnectionContext.Provider>;
};
