import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';

import { useConnection } from '../hooks/useConnection';
import { getSystemUsage } from '../utils/tauriCommands';

const HISTORY_MAX_ENTRIES = -20;

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
const FAST_INTERVAL = 3_000; // 3s when monitor is open

const defaultUsage: UsageData = { cpu: 0, gpu: 0, ram: 0 };

const SystemResourcesContext = createContext<SystemResourcesValue>({
  totalVramGb: 0,
  totalRamGb: 0,
  ready: false,
  usage: defaultUsage,
  history: [],
  hasData: false,
  setMonitorActive: () => {},
});

// eslint-disable-next-line max-lines-per-function
export const SystemResourcesProvider = ({ children }: { children: ReactNode }) => {
  // Defaults are 0, not arbitrary "common" sizes — the VRAM optimizer relies on
  // a real 0 to detect "no GPU" and fall back to CPU-only mode. A 24 GB default
  // would silently make the optimizer try to load all layers on a non-existent GPU.
  const [totalVramGb, setTotalVramGb] = useState(0);
  const [totalRamGb, setTotalRamGb] = useState(0);
  const [ready, setReady] = useState(false);
  const [usage, setUsage] = useState<UsageData>(defaultUsage);
  const [history, setHistory] = useState<UsageData[]>([]);
  const [hasData, setHasData] = useState(false);

  const { connected } = useConnection();
  const connectedRef = useRef(connected);
  connectedRef.current = connected;

  const isFetchingRef = useRef(false);
  const activeRef = useRef(false);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const fetchUsage = useCallback(async () => {
    if (isFetchingRef.current || !connectedRef.current) return;
    isFetchingRef.current = true;
    try {
      const data = await getSystemUsage();
      setUsage(data);
      setHasData(true);
      setHistory((prev) => [...prev, data].slice(HISTORY_MAX_ENTRIES));

      // Update hardware totals on EVERY successful poll, not just the first.
      // The backend lazy-populates a hardware-totals cache via WMI/nvidia-smi
      // calls that can time out on the first invocation, returning 0 for RAM
      // and VRAM. If we only wrote totals once we'd be stuck with those zeros
      // forever, breaking the memory visualization. We use the backend's value
      // verbatim (including 0) so the VRAM optimizer can distinguish "no GPU"
      // from "still detecting" — but we keep updating until non-zero arrives.
      if (typeof data.total_vram_gb === 'number') {
        setTotalVramGb(data.total_vram_gb);
      }
      if (typeof data.total_ram_gb === 'number' && data.total_ram_gb > 0) {
        setTotalRamGb(data.total_ram_gb);
      }
      if (!ready) {
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

  const setMonitorActive = useCallback(
    (active: boolean) => {
      activeRef.current = active;
      // Restart interval at the appropriate rate
      if (intervalRef.current) clearInterval(intervalRef.current);
      if (active) {
        fetchUsage(); // Immediate fetch on open
        intervalRef.current = setInterval(fetchUsage, FAST_INTERVAL);
      } else {
        intervalRef.current = setInterval(fetchUsage, SLOW_INTERVAL);
      }
    },
    [fetchUsage],
  );

  const value = useMemo<SystemResourcesValue>(
    () => ({
      totalVramGb,
      totalRamGb,
      ready,
      usage,
      history,
      hasData,
      setMonitorActive,
    }),
    [totalVramGb, totalRamGb, ready, usage, history, hasData, setMonitorActive],
  );

  return (
    <SystemResourcesContext.Provider value={value}>{children}</SystemResourcesContext.Provider>
  );
};

// eslint-disable-next-line react-refresh/only-export-components
export function useSystemResources() {
  return useContext(SystemResourcesContext);
}
