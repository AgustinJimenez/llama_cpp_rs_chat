import { useEffect, useState } from 'react';

interface UsageData {
  cpu: number;
  gpu: number;
  ram: number;
}

export function SystemUsage() {
  const [usage, setUsage] = useState<UsageData>({ cpu: 0, gpu: 0, ram: 0 });
  const [history, setHistory] = useState<UsageData[]>([]);

  useEffect(() => {
    const fetchUsage = async () => {
      try {
        const response = await fetch('/api/system/usage');
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
        console.error('Failed to fetch system usage:', error);
      }
    };

    // Fetch immediately
    fetchUsage();

    // Then poll every 500ms for smooth real-time updates
    const interval = setInterval(fetchUsage, 500);

    return () => clearInterval(interval);
  }, []);

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
