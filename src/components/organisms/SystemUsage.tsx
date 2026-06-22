import { Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { useSystemResources } from '../../contexts/SystemResourcesContext';

interface SystemUsageProps {
  expanded?: boolean;
}

// eslint-disable-next-line max-lines-per-function
export const SystemUsage = ({ expanded = false }: SystemUsageProps) => {
  const { t } = useTranslation();
  const { usage, history, hasData, totalVramGb, totalRamGb } = useSystemResources();

  const renderMiniGraph = (data: number[], color: string) => {
    if (data.length === 0) return null;

    const points = data
      .map((value, index) => {
        const x = (index / (data.length - 1)) * 100;
        const y = 100 - value;
        return `${x},${y}`;
      })
      .join(' ');

    return (
      <svg className="h-6 w-12" viewBox="0 0 100 100" preserveAspectRatio="none">
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

  const effectiveVramGb =
    usage.total_vram_gb && usage.total_vram_gb > 0 ? usage.total_vram_gb : totalVramGb;
  const effectiveRamGb =
    usage.total_ram_gb && usage.total_ram_gb > 0 ? usage.total_ram_gb : totalRamGb;
  const cpuGhz = usage.cpu_ghz || 0;
  const vramUsedGb =
    usage.vram_used_gb != null && usage.vram_used_gb > 0
      ? usage.vram_used_gb
      : (usage.gpu / 100) * effectiveVramGb;
  const ramUsedGb = (usage.ram / 100) * effectiveRamGb;

  const cpuHistory = history.map((h) => h.cpu);
  const gpuHistory = history.map((h) => h.gpu);
  const ramHistory = history.map((h) => h.ram);

  const renderLargeGraph = (data: number[], color: string, _label: string, gbText?: string) => {
    if (data.length === 0) return null;

    const points = data
      .map((value, index) => {
        const x = (index / (data.length - 1)) * 100;
        const y = 100 - value;
        return `${x},${y}`;
      })
      .join(' ');

    const areaPoints = `0,100 ${points} 100,100`;

    return (
      <div className="relative">
        {!!gbText && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
            <span className="text-lg font-bold text-foreground">{gbText}</span>
          </div>
        )}
        <svg className="relative h-24 w-full" viewBox="0 0 100 100" preserveAspectRatio="none">
          <line
            x1="0"
            y1="25"
            x2="100"
            y2="25"
            stroke="currentColor"
            strokeOpacity="0.1"
            strokeWidth="0.5"
          />
          <line
            x1="0"
            y1="50"
            x2="100"
            y2="50"
            stroke="currentColor"
            strokeOpacity="0.1"
            strokeWidth="0.5"
          />
          <line
            x1="0"
            y1="75"
            x2="100"
            y2="75"
            stroke="currentColor"
            strokeOpacity="0.1"
            strokeWidth="0.5"
          />
          <polygon points={areaPoints} fill={color} fillOpacity="0.2" />
          <polyline
            points={points}
            fill="none"
            stroke={color}
            strokeWidth="2"
            vectorEffect="non-scaling-stroke"
          />
        </svg>
      </div>
    );
  };

  // Expanded view for sidebar
  if (expanded) {
    if (!hasData) {
      return (
        <div className="flex h-32 items-center justify-center gap-2 text-muted-foreground">
          <Loader2 className="size-5 animate-spin" />
          <span className="text-sm">{t('systemUsage.loadingExpanded')}</span>
        </div>
      );
    }
    return (
      <div className="space-y-6">
        {/* CPU */}
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-sm font-semibold text-blue-400">{t('systemUsage.cpuUsage')}</span>
            <span className="text-lg font-bold text-foreground">{usage.cpu.toFixed(1)}%</span>
          </div>
          <div className="rounded-lg border border-border bg-muted p-2">
            {renderLargeGraph(
              cpuHistory,
              '#3b82f6',
              'CPU',
              cpuGhz > 0 ? `${cpuGhz.toFixed(2)} GHz` : undefined,
            )}
          </div>
        </div>

        {/* GPU */}
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-sm font-semibold text-green-500">
              {t('systemUsage.gpuUsage')}
            </span>
            <span className="text-lg font-bold text-foreground">{usage.gpu.toFixed(1)}%</span>
          </div>
          <div className="rounded-lg border border-border bg-muted p-2">
            {renderLargeGraph(
              gpuHistory,
              '#22c55e',
              'GPU',
              `${vramUsedGb.toFixed(1)} / ${effectiveVramGb.toFixed(1)} GB`,
            )}
          </div>
        </div>

        {/* RAM */}
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-sm font-semibold text-purple-400">
              {t('systemUsage.ramUsage')}
            </span>
            <span className="text-lg font-bold text-foreground">{usage.ram.toFixed(1)}%</span>
          </div>
          <div className="rounded-lg border border-border bg-muted p-2">
            {renderLargeGraph(
              ramHistory,
              '#a855f7',
              'RAM',
              `${ramUsedGb.toFixed(1)} / ${effectiveRamGb.toFixed(1)} GB`,
            )}
          </div>
        </div>

        {/* App process RAM */}
        {typeof usage.app_ram_gb === 'number' && usage.app_ram_gb > 0 && (
          <div className="border-t border-border/50 pt-2">
            <div className="flex items-center justify-between">
              <span className="text-xs text-muted-foreground">{t('systemUsage.appRam')}</span>
              <span className="text-sm font-semibold text-foreground">
                {usage.app_ram_gb.toFixed(2)} GB
              </span>
            </div>
          </div>
        )}
      </div>
    );
  }

  // Compact view for header
  if (!hasData) {
    return (
      <div className="flex items-center gap-2 rounded-xl border border-border bg-muted px-3 py-2 text-muted-foreground">
        <Loader2 className="size-4 animate-spin" />
        <span className="text-xs font-medium">{t('systemUsage.loading')}</span>
      </div>
    );
  }
  return (
    <div className="flex items-center gap-3 rounded-xl border border-border bg-muted px-3 py-2">
      {/* CPU Usage */}
      <div className="flex items-center gap-2">
        <span className="text-xs font-semibold text-blue-400">{t('systemUsage.cpu')}</span>
        <span className="text-xs font-medium text-foreground">{usage.cpu.toFixed(0)}%</span>
        {cpuGhz > 0 && (
          <span className="text-[10px] text-muted-foreground">
            {t('stats.cpuSpeed', { speed: cpuGhz.toFixed(1) })}
          </span>
        )}
        <div className="rounded-lg border border-border bg-background px-2 py-1">
          {renderMiniGraph(cpuHistory, '#3b82f6')}
        </div>
      </div>

      {/* GPU Usage */}
      <div className="flex items-center gap-2">
        <span className="text-xs font-semibold text-green-500">{t('systemUsage.gpu')}</span>
        <span className="text-xs font-medium text-foreground">{usage.gpu.toFixed(0)}%</span>
        <span className="text-[10px] text-muted-foreground">{effectiveVramGb.toFixed(0)}G</span>
        <div className="rounded-lg border border-border bg-background px-2 py-1">
          {renderMiniGraph(gpuHistory, '#22c55e')}
        </div>
      </div>

      {/* RAM Usage */}
      <div className="flex items-center gap-2">
        <span className="text-xs font-semibold text-purple-400">{t('systemUsage.ram')}</span>
        <span className="text-xs font-medium text-foreground">{usage.ram.toFixed(0)}%</span>
        <span className="text-[10px] text-muted-foreground">
          {ramUsedGb.toFixed(0)}/{effectiveRamGb.toFixed(0)}G
        </span>
        <div className="rounded-lg border border-border bg-background px-2 py-1">
          {renderMiniGraph(ramHistory, '#a855f7')}
        </div>
      </div>
    </div>
  );
};
