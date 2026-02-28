import { Loader2 } from 'lucide-react';
import { useSystemResources } from '../../contexts/SystemResourcesContext';

interface SystemUsageProps {
  expanded?: boolean;
}

// eslint-disable-next-line max-lines-per-function
export function SystemUsage({ expanded = false }: SystemUsageProps) {
  const { usage, history, hasData, totalVramGb, totalRamGb } = useSystemResources();

  const renderMiniGraph = (data: number[], color: string) => {
    if (data.length === 0) return null;

    const points = data.map((value, index) => {
      const x = (index / (data.length - 1)) * 100;
      const y = 100 - value;
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

  const effectiveVramGb = usage.total_vram_gb && usage.total_vram_gb > 0 ? usage.total_vram_gb : totalVramGb;
  const effectiveRamGb = usage.total_ram_gb && usage.total_ram_gb > 0 ? usage.total_ram_gb : totalRamGb;
  const cpuGhz = usage.cpu_ghz || 0;
  const vramUsedGb = (usage.gpu / 100) * effectiveVramGb;
  const ramUsedGb = (usage.ram / 100) * effectiveRamGb;

  const cpuHistory = history.map(h => h.cpu);
  const gpuHistory = history.map(h => h.gpu);
  const ramHistory = history.map(h => h.ram);

  const renderLargeGraph = (data: number[], color: string, _label: string, gbText?: string) => {
    if (data.length === 0) return null;

    const points = data.map((value, index) => {
      const x = (index / (data.length - 1)) * 100;
      const y = 100 - value;
      return `${x},${y}`;
    }).join(' ');

    const areaPoints = `0,100 ${points} 100,100`;

    return (
      <div className="relative">
        {gbText ? <div className="absolute inset-0 flex items-center justify-center pointer-events-none">
            <span className="text-lg font-bold text-white">{gbText}</span>
          </div> : null}
        <svg className="w-full h-24 relative" viewBox="0 0 100 100" preserveAspectRatio="none">
          <line x1="0" y1="25" x2="100" y2="25" stroke="currentColor" strokeOpacity="0.1" strokeWidth="0.5" />
          <line x1="0" y1="50" x2="100" y2="50" stroke="currentColor" strokeOpacity="0.1" strokeWidth="0.5" />
          <line x1="0" y1="75" x2="100" y2="75" stroke="currentColor" strokeOpacity="0.1" strokeWidth="0.5" />
          <polygon points={areaPoints} fill={color} fillOpacity="0.2" />
          <polyline points={points} fill="none" stroke={color} strokeWidth="2" vectorEffect="non-scaling-stroke" />
        </svg>
      </div>
    );
  };

  // Expanded view for sidebar
  if (expanded) {
    if (!hasData) {
      return (
        <div className="flex items-center justify-center gap-2 h-32 text-muted-foreground">
          <Loader2 className="h-5 w-5 animate-spin" />
          <span className="text-sm">Loading system usage...</span>
        </div>
      );
    }
    return (
      <div className="space-y-6">
        {/* CPU */}
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-sm font-semibold text-blue-500">CPU Usage</span>
            <span className="text-lg font-bold text-foreground">{usage.cpu.toFixed(1)}%</span>
          </div>
          <div className="bg-muted rounded-lg p-2 border border-border">
            {renderLargeGraph(cpuHistory, '#3b82f6', 'CPU', cpuGhz > 0 ? `${cpuGhz.toFixed(2)} GHz` : undefined)}
          </div>
        </div>

        {/* GPU */}
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-sm font-semibold text-green-500">GPU Usage</span>
            <span className="text-lg font-bold text-foreground">{usage.gpu.toFixed(1)}%</span>
          </div>
          <div className="bg-muted rounded-lg p-2 border border-border">
            {renderLargeGraph(gpuHistory, '#22c55e', 'GPU', `${vramUsedGb.toFixed(1)} / ${effectiveVramGb.toFixed(1)} GB`)}
          </div>
        </div>

        {/* RAM */}
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-sm font-semibold text-purple-500">RAM Usage</span>
            <span className="text-lg font-bold text-foreground">{usage.ram.toFixed(1)}%</span>
          </div>
          <div className="bg-muted rounded-lg p-2 border border-border">
            {renderLargeGraph(ramHistory, '#a855f7', 'RAM', `${ramUsedGb.toFixed(1)} / ${effectiveRamGb.toFixed(1)} GB`)}
          </div>
        </div>
      </div>
    );
  }

  // Compact view for header
  if (!hasData) {
    return (
      <div className="flex items-center gap-2 px-3 py-2 bg-muted rounded-xl border border-border text-muted-foreground">
        <Loader2 className="h-4 w-4 animate-spin" />
        <span className="text-xs font-medium">Loading</span>
      </div>
    );
  }
  return (
    <div className="flex items-center gap-3 px-3 py-2 bg-muted rounded-xl border border-border">
      {/* CPU Usage */}
      <div className="flex items-center gap-2">
        <span className="text-xs font-semibold text-blue-500">CPU</span>
        <span className="text-xs font-medium text-foreground">{usage.cpu.toFixed(0)}%</span>
        {cpuGhz > 0 && <span className="text-[10px] text-muted-foreground">{cpuGhz.toFixed(1)}GHz</span>}
        <div className="px-2 py-1 bg-background rounded-lg border border-border">
          {renderMiniGraph(cpuHistory, '#3b82f6')}
        </div>
      </div>

      {/* GPU Usage */}
      <div className="flex items-center gap-2">
        <span className="text-xs font-semibold text-green-500">GPU</span>
        <span className="text-xs font-medium text-foreground">{usage.gpu.toFixed(0)}%</span>
        <span className="text-[10px] text-muted-foreground">{effectiveVramGb.toFixed(0)}G</span>
        <div className="px-2 py-1 bg-background rounded-lg border border-border">
          {renderMiniGraph(gpuHistory, '#22c55e')}
        </div>
      </div>

      {/* RAM Usage */}
      <div className="flex items-center gap-2">
        <span className="text-xs font-semibold text-purple-500">RAM</span>
        <span className="text-xs font-medium text-foreground">{usage.ram.toFixed(0)}%</span>
        <span className="text-[10px] text-muted-foreground">{ramUsedGb.toFixed(0)}/{effectiveRamGb.toFixed(0)}G</span>
        <div className="px-2 py-1 bg-background rounded-lg border border-border">
          {renderMiniGraph(ramHistory, '#a855f7')}
        </div>
      </div>
    </div>
  );
}
