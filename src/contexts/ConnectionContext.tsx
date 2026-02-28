import React, { createContext, useContext, useEffect, useRef, useState } from 'react';
import { isTauriEnv } from '../utils/tauri';

interface ConnectionState {
  connected: boolean;
  reconnecting: boolean;
}

const ConnectionContext = createContext<ConnectionState>({
  connected: true,
  reconnecting: false,
});

export const useConnection = () => useContext(ConnectionContext);

const WS_RECONNECT_BASE_MS = 500;
const WS_RECONNECT_MAX_MS = 5000;

function getReconnectDelay(attempt: number): number {
  return Math.min(WS_RECONNECT_BASE_MS * Math.pow(2, attempt), WS_RECONNECT_MAX_MS);
}

export const ConnectionProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  // Optimistic: assume connected until proven otherwise
  const [state, setState] = useState<ConnectionState>({ connected: true, reconnecting: false });
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
        setState({ connected: true, reconnecting: false });
      };

      ws.onclose = () => {
        if (!mountedRef.current) return;
        wsRef.current = null;
        setState({ connected: false, reconnecting: true });
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

  return (
    <ConnectionContext.Provider value={state}>
      {children}
    </ConnectionContext.Provider>
  );
};
