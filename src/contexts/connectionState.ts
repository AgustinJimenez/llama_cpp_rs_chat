import { createContext } from 'react';

export interface ConnectionState {
  connected: boolean;
  reconnecting: boolean;
  attempt: number;
  disconnectedAt: number | null;
}

export const ConnectionContext = createContext<ConnectionState>({
  connected: true,
  reconnecting: false,
  attempt: 0,
  disconnectedAt: null,
});
