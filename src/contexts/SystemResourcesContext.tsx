import { createContext, useContext, useEffect, useState, type ReactNode } from 'react';
import { getSystemUsage } from '../utils/tauriCommands';

interface SystemResources {
  totalVramGb: number;
  totalRamGb: number;
  ready: boolean;
}

const SystemResourcesContext = createContext<SystemResources>({
  totalVramGb: 24.0,
  totalRamGb: 64.0,
  ready: false,
});

export function SystemResourcesProvider({ children }: { children: ReactNode }) {
  const [resources, setResources] = useState<SystemResources>({
    totalVramGb: 24.0,
    totalRamGb: 64.0,
    ready: false,
  });

  useEffect(() => {
    const fetchHardware = async () => {
      try {
        const usage = await getSystemUsage();
        setResources({
          totalVramGb: (usage.total_vram_gb && usage.total_vram_gb > 0) ? usage.total_vram_gb : 24.0,
          totalRamGb: (usage.total_ram_gb && usage.total_ram_gb > 0) ? usage.total_ram_gb : 64.0,
          ready: true,
        });
      } catch {
        setResources(prev => ({ ...prev, ready: true }));
      }
    };
    fetchHardware();
  }, []);

  return (
    <SystemResourcesContext.Provider value={resources}>
      {children}
    </SystemResourcesContext.Provider>
  );
}

export function useSystemResources() {
  return useContext(SystemResourcesContext);
}
