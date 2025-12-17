import { useEffect, useRef, useState } from 'react';

interface UsageData {
  cpu: number;
  gpu: number;
  ram: number;
}

interface SystemUsageProps {
  expanded?: boolean;
  active?: boolean;
}

// eslint-disable-next-line max-lines-per-function
export function SystemUsage({ expanded = false, active = true }: SystemUsageProps) {
  const [usage, setUsage] = useState<UsageData>({ cpu: 0, gpu: 0, ram: 0 });
  const [history, setHistory] = useState<UsageData[]>([]);
  const isFetchingRef = useRef(false);

  useEffect(() => {
    if (!active) {
      return undefined;
    }

    const fetchUsage = async () => {
      if (isFetchingRef.current) return;
      isFetchingRef.current = true;

      const controller = new AbortController();
      const timeoutId = window.setTimeout(() => controller.abort(), 2000);
      try {
        const response = await fetch('/api/system/usage', { signal: controller.signal });
        if (response.ok) {
          const data = await response.json();
          setUsage(data);

          // Keep last 20 data points for mini graph
          setHistory(prev => {
            const updated = [...prev, data];
            return updated.slice(-20);
          });
        }
      } catch (error) {
        const isAbort =
          error instanceof DOMException &&
          (error.name === 'AbortError' || error.message.toLowerCase().includes('aborted'));
        if (!isAbort) {
          console.error('Failed to fetch system usage:', error);
        }
      } finally {
        window.clearTimeout(timeoutId);
        isFetchingRef.current = false;
      }
    };

    // Fetch immediately
    fetchUsage();

    // Then poll every 3s to avoid piling requests
    const interval = setInterval(fetchUsage, 3000);

    return () => clearInterval(interval);
  }, [active]);

  const renderMiniGraph = (data: number[], color: string) => {
    if (data.length === 0) return null;

    // Always scale to 100% for accurate representation
    const points = data.map((value, index) => {
      const x = (index / (data.length - 1)) * 100;
      const y = 100 - value; // Direct mapping: 0% = bottom (100), 100% = top (0)
      return `${x},${y}`;
    }).join(' ');

    return (
      <svg className="w-12 h-6" viewBox="0 0 100 100" preserveAspectRatio="none">
        <polyline
          points={points}
          fill="none"
          stroke={color}
          strokeWidth="2"
          vectorEffect="non-scaling-stroke"
        />
      </svg>
    );
  };

  const cpuHistory = history.map(h => h.cpu);
  const gpuHistory = history.map(h => h.gpu);
  const ramHistory = history.map(h => h.ram);

  const renderLargeGraph = (data: number[], color: string, _label: string) => {
    if (data.length === 0) return null;

    const points = data.map((value, index) => {
      const x = (index / (data.length - 1)) * 100;
      const y = 100 - value;
      return `${x},${y}`;
    }).join(' ');

    // Create filled area points
    const areaPoints = `0,100 ${points} 100,100`;

    return (
      <svg className="w-full h-24" viewBox="0 0 100 100" preserveAspectRatio="none">
        {/* Grid lines */}
        <line x1="0" y1="25" x2="100" y2="25" stroke="currentColor" strokeOpacity="0.1" strokeWidth="0.5" />
        <line x1="0" y1="50" x2="100" y2="50" stroke="currentColor" strokeOpacity="0.1" strokeWidth="0.5" />
        <line x1="0" y1="75" x2="100" y2="75" stroke="currentColor" strokeOpacity="0.1" strokeWidth="0.5" />
        {/* Filled area */}
        <polygon
          points={areaPoints}
          fill={color}
          fillOpacity="0.2"
        />
        {/* Line */}
        <polyline
          points={points}
          fill="none"
          stroke={color}
          strokeWidth="2"
          vectorEffect="non-scaling-stroke"
        />
      </svg>
    );
  };

  // Expanded view for sidebar
  if (expanded) {
    return (
      <div className="space-y-6">
        {/* CPU */}
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-sm font-semibold text-blue-500">CPU Usage</span>
            <span className="text-lg font-bold text-foreground">{usage.cpu.toFixed(1)}%</span>
          </div>
          <div className="bg-muted rounded-lg p-2 border border-border">
            {renderLargeGraph(cpuHistory, '#3b82f6', 'CPU')}
          </div>
        </div>

        {/* GPU */}
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-sm font-semibold text-green-500">GPU Usage</span>
            <span className="text-lg font-bold text-foreground">{usage.gpu.toFixed(1)}%</span>
          </div>
          <div className="bg-muted rounded-lg p-2 border border-border">
            {renderLargeGraph(gpuHistory, '#22c55e', 'GPU')}
          </div>
        </div>

        {/* RAM */}
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-sm font-semibold text-purple-500">RAM Usage</span>
            <span className="text-lg font-bold text-foreground">{usage.ram.toFixed(1)}%</span>
          </div>
          <div className="bg-muted rounded-lg p-2 border border-border">
            {renderLargeGraph(ramHistory, '#a855f7', 'RAM')}
          </div>
        </div>
      </div>
    );
  }

  // Compact view for header
  return (
    <div className="flex items-center gap-3 px-3 py-2 bg-muted rounded-xl border border-border">
      {/* CPU Usage */}
      <div className="flex items-center gap-2">
        <span className="text-xs font-semibold text-blue-500">CPU</span>
        <span className="text-xs font-medium text-foreground">{usage.cpu.toFixed(0)}%</span>
        <div className="px-2 py-1 bg-background rounded-lg border border-border">
          {renderMiniGraph(cpuHistory, '#3b82f6')}
        </div>
      </div>

      {/* GPU Usage */}
      <div className="flex items-center gap-2">
        <span className="text-xs font-semibold text-green-500">GPU</span>
        <span className="text-xs font-medium text-foreground">{usage.gpu.toFixed(0)}%</span>
        <div className="px-2 py-1 bg-background rounded-lg border border-border">
          {renderMiniGraph(gpuHistory, '#22c55e')}
        </div>
      </div>

      {/* RAM Usage */}
      <div className="flex items-center gap-2">
        <span className="text-xs font-semibold text-purple-500">RAM</span>
        <span className="text-xs font-medium text-foreground">{usage.ram.toFixed(0)}%</span>
        <div className="px-2 py-1 bg-background rounded-lg border border-border">
          {renderMiniGraph(ramHistory, '#a855f7')}
        </div>
      </div>
    </div>
  );
}
