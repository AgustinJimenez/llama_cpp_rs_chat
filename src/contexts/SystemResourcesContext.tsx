import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from 'react';
import { getSystemUsage } from '../utils/tauriCommands';

export interface UsageData {
  cpu: number;
  gpu: number;
  ram: number;
  total_ram_gb?: number;
  total_vram_gb?: number;
  cpu_cores?: number;
  cpu_ghz?: number;
}

interface SystemResourcesValue {
  totalVramGb: number;
  totalRamGb: number;
  ready: boolean;
  usage: UsageData;
  history: UsageData[];
  hasData: boolean;
  setMonitorActive: (active: boolean) => void;
}

const SLOW_INTERVAL = 10_000; // 10s background polling
const FAST_INTERVAL = 3_000;  // 3s when monitor is open

const defaultUsage: UsageData = { cpu: 0, gpu: 0, ram: 0 };

const SystemResourcesContext = createContext<SystemResourcesValue>({
  totalVramGb: 24.0,
  totalRamGb: 64.0,
  ready: false,
  usage: defaultUsage,
  history: [],
  hasData: false,
  setMonitorActive: () => {},
});

// eslint-disable-next-line max-lines-per-function
export function SystemResourcesProvider({ children }: { children: ReactNode }) {
  const [totalVramGb, setTotalVramGb] = useState(24.0);
  const [totalRamGb, setTotalRamGb] = useState(64.0);
  const [ready, setReady] = useState(false);
  const [usage, setUsage] = useState<UsageData>(defaultUsage);
  const [history, setHistory] = useState<UsageData[]>([]);
  const [hasData, setHasData] = useState(false);

  const isFetchingRef = useRef(false);
  const activeRef = useRef(false);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const fetchUsage = useCallback(async () => {
    if (isFetchingRef.current) return;
    isFetchingRef.current = true;
    try {
      const data = await getSystemUsage();
      setUsage(data);
      setHasData(true);
      setHistory(prev => [...prev, data].slice(-20));

      // Update hardware totals from first successful response
      if (!ready) {
        setTotalVramGb(data.total_vram_gb && data.total_vram_gb > 0 ? data.total_vram_gb : 24.0);
        setTotalRamGb(data.total_ram_gb && data.total_ram_gb > 0 ? data.total_ram_gb : 64.0);
        setReady(true);
      }
    } catch (error) {
      const isAbort =
        error instanceof DOMException &&
        (error.name === 'AbortError' || error.message.toLowerCase().includes('aborted'));
      if (!isAbort) {
        console.error('Failed to fetch system usage:', error);
      }
      if (!ready) setReady(true);
    } finally {
      isFetchingRef.current = false;
    }
  }, [ready]);

  // Start background polling on mount
  useEffect(() => {
    fetchUsage();
    intervalRef.current = setInterval(fetchUsage, SLOW_INTERVAL);
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, [fetchUsage]);

  const setMonitorActive = useCallback((active: boolean) => {
    activeRef.current = active;
    // Restart interval at the appropriate rate
    if (intervalRef.current) clearInterval(intervalRef.current);
    if (active) {
      fetchUsage(); // Immediate fetch on open
      intervalRef.current = setInterval(fetchUsage, FAST_INTERVAL);
    } else {
      intervalRef.current = setInterval(fetchUsage, SLOW_INTERVAL);
    }
  }, [fetchUsage]);

  return (
    <SystemResourcesContext.Provider value={{
      totalVramGb, totalRamGb, ready,
      usage, history, hasData,
      setMonitorActive,
    }}>
      {children}
    </SystemResourcesContext.Provider>
  );
}

// eslint-disable-next-line react-refresh/only-export-components
export function useSystemResources() {
  return useContext(SystemResourcesContext);
}
