/* eslint-disable max-lines -- multi-component memory visualization with bars, legends, sliders, warnings */
import { AlertTriangle, Info } from 'lucide-react';
import React from 'react';
import { useTranslation } from 'react-i18next';

import { Card, CardContent, CardHeader, CardTitle } from '../../atoms/card';

export interface MemoryBreakdown {
  vram: {
    total: number;
    modelGpu: number;
    kvCache: number; // KV cache portion on GPU
    overhead: number;
    available: number;
    overcommitted: boolean;
  };
  ram: {
    total: number;
    modelCpu: number;
    kvCacheCpu: number; // KV cache portion on CPU (when gpu_layers < totalLayers)
    available: number;
    overcommitted: boolean;
  };
}

interface MemoryVisualizationProps {
  memory: MemoryBreakdown;
  /** Apple Silicon: render one shared "Unified Memory" bar instead of separate VRAM/RAM bars. */
  unifiedMemory?: boolean;
  overheadGb: number;
  onOverheadChange: (value: number) => void;
  gpuLayers: number;
  onGpuLayersChange: (layers: number) => void;
  maxLayers: number;
  contextSize: number;
  onContextSizeChange: (size: number) => void;
  maxContextSize: number;
  systemPromptTokens?: number;
  toolDefinitionsTokens?: number;
}

const MIN_CONTEXT = 512;
const SLIDER_STEPS = 1000;

const CONTEXT_ROUND_STEP = 256;
const TOKENS_PER_MEGA = 1048576;
const UTILIZATION_CRITICAL_PCT = 90;
const UTILIZATION_WARNING_PCT = 75;
const UTILIZATION_HIGH_PCT = 85;

function sliderToContext(t: number, min: number, max: number): number {
  const value = min + t * (max - min);
  return Math.round(value / CONTEXT_ROUND_STEP) * CONTEXT_ROUND_STEP;
}

function contextToSlider(value: number, min: number, max: number): number {
  return (value - min) / (max - min);
}

function formatSize(n: number): string {
  if (n >= TOKENS_PER_MEGA) {
    return `${(n / TOKENS_PER_MEGA).toFixed(n % TOKENS_PER_MEGA === 0 ? 0 : 1)}M`;
  }
  if (n >= 1024) return `${(n / 1024).toFixed(n % 1024 === 0 ? 0 : 1)}K`;
  return String(n);
}

function getStatusColor(utilization: number, overcommitted: boolean) {
  if (overcommitted) return 'text-red-600 dark:text-red-400';
  if (utilization > UTILIZATION_CRITICAL_PCT) return 'text-orange-600 dark:text-orange-400';
  if (utilization > UTILIZATION_WARNING_PCT) return 'text-yellow-600 dark:text-yellow-400';
  return 'text-green-600 dark:text-green-400';
}

// --- Bar segment ---

interface BarSegmentProps {
  color: string;
  widthPct: number;
  label?: string;
  title: string;
  minPctForLabel?: number;
  textColor?: string;
}

const BarSegment: React.FC<BarSegmentProps> = ({
  color,
  widthPct,
  label,
  title,
  minPctForLabel = 8,
  textColor = 'text-white',
}) => {
  if (widthPct <= 0) return null;
  return (
    <div
      className={`${color} flex items-center justify-center text-xs ${textColor} font-medium transition-all duration-300`}
      style={{ width: `${widthPct}%` }}
      title={title}
    >
      {widthPct > minPctForLabel && label}
    </div>
  );
};

// --- VRAM bar ---

export const VramBar: React.FC<{ vram: MemoryBreakdown['vram'] }> = ({ vram }) => {
  const { t } = useTranslation();
  // Hide the VRAM bar entirely when there's no GPU detected (total === 0).
  // The previous behavior rendered an "OVERCOMMITTED" red box with
  // "Infinity%" because used > 0 && total === 0 — misleading on CPU-only
  // systems where a 0/0 GPU is the correct state, not an error.
  if (vram.total <= 0) {
    return (
      <div className="space-y-2">
        <div className="flex items-baseline justify-between">
          <span className="text-sm font-medium text-foreground/80">
            {t('memoryVisualization.gpuMemory')}
          </span>
          <span className="font-mono text-sm text-muted-foreground">
            {t('memoryVisualization.noGpu')}
          </span>
        </div>
        <div className="flex h-8 items-center justify-center rounded-md bg-muted/40">
          <span className="text-xs text-muted-foreground">{t('memoryVisualization.cpuOnly')}</span>
        </div>
      </div>
    );
  }

  const used = vram.modelGpu + vram.kvCache + vram.overhead;
  const utilization = (used / vram.total) * 100;
  const modelPct = (vram.modelGpu / vram.total) * 100;
  const cachePct = (vram.kvCache / vram.total) * 100;
  const overheadPct = (vram.overhead / vram.total) * 100;
  const availablePct = (vram.available / vram.total) * 100;
  const statusColor = getStatusColor(utilization, vram.overcommitted);

  return (
    <div className="space-y-2">
      <div className="flex items-baseline justify-between">
        <span className="text-sm font-medium text-foreground/80">
          {t('memoryVisualization.gpuMemory')}
        </span>
        <span className={`font-mono text-sm ${statusColor}`}>
          {t('memoryVisualization.usage', {
            used: used.toFixed(2),
            total: vram.total.toFixed(2),
            percent: utilization.toFixed(1),
          })}
        </span>
      </div>
      <div className="relative flex h-8 overflow-hidden rounded-md bg-muted">
        <BarSegment
          color="bg-green-600"
          widthPct={Math.min(modelPct, 100)}
          label={`${vram.modelGpu.toFixed(1)}G`}
          title={t('memoryVisualization.modelGpu', { size: vram.modelGpu.toFixed(2) })}
        />
        <BarSegment
          color="bg-orange-500"
          widthPct={Math.min(cachePct, 100 - modelPct)}
          label={`${vram.kvCache.toFixed(1)}G`}
          title={t('memoryVisualization.kvCache', { size: vram.kvCache.toFixed(2) })}
        />
        <BarSegment
          color="bg-purple-600"
          widthPct={Math.min(overheadPct, 100 - modelPct - cachePct)}
          label={`${vram.overhead.toFixed(1)}G`}
          title={t('memoryVisualization.overhead', { size: vram.overhead.toFixed(2) })}
          minPctForLabel={6}
        />
        {!vram.overcommitted && (
          <BarSegment
            color="bg-accent/50"
            widthPct={availablePct}
            label={`${vram.available.toFixed(1)}G`}
            title={t('memoryVisualization.available', { size: vram.available.toFixed(2) })}
            textColor="text-muted-foreground"
            minPctForLabel={10}
          />
        )}
        {!!vram.overcommitted && (
          <div className="absolute inset-0 flex items-center justify-center border-2 border-red-600 bg-red-600/20">
            <span className="text-xs font-bold text-foreground">
              {t('memoryVisualization.overcommitted')}
            </span>
          </div>
        )}
      </div>
    </div>
  );
};

// --- RAM bar ---

const RamBar: React.FC<{ ram: MemoryBreakdown['ram'] }> = ({ ram }) => {
  const { t } = useTranslation();
  const used = ram.modelCpu + ram.kvCacheCpu;
  // Guard against ram.total === 0 (system info not yet detected) — show
  // 0% instead of NaN/Infinity rather than rendering "Infinity%".
  const utilization = ram.total > 0 ? (used / ram.total) * 100 : 0;
  const modelPct = ram.total > 0 ? (ram.modelCpu / ram.total) * 100 : 0;
  const cachePct = ram.total > 0 ? (ram.kvCacheCpu / ram.total) * 100 : 0;
  const availablePct = ram.total > 0 ? (ram.available / ram.total) * 100 : 0;
  const statusColor = getStatusColor(utilization, ram.overcommitted);

  return (
    <div className="space-y-2">
      <div className="flex items-baseline justify-between">
        <span className="text-sm font-medium text-foreground/80">
          {t('memoryVisualization.systemMemory')}
        </span>
        <span className={`font-mono text-sm ${statusColor}`}>
          {t('memoryVisualization.usage', {
            used: used.toFixed(2),
            total: ram.total.toFixed(2),
            percent: utilization.toFixed(1),
          })}
        </span>
      </div>
      <div className="relative flex h-8 overflow-hidden rounded-md bg-muted">
        <BarSegment
          color="bg-cyan-600"
          widthPct={Math.min(modelPct, 100)}
          label={`${ram.modelCpu.toFixed(1)}G`}
          title={t('memoryVisualization.modelCpu', { size: ram.modelCpu.toFixed(2) })}
        />
        {ram.kvCacheCpu > 0 && (
          <BarSegment
            color="bg-orange-500"
            widthPct={Math.min(cachePct, 100 - modelPct)}
            label={`${ram.kvCacheCpu.toFixed(1)}G`}
            title={t('memoryVisualization.kvCacheCpu', { size: ram.kvCacheCpu.toFixed(2) })}
          />
        )}
        {!ram.overcommitted && (
          <BarSegment
            color="bg-accent/50"
            widthPct={availablePct}
            label={`${ram.available.toFixed(1)}G`}
            title={t('memoryVisualization.available', { size: ram.available.toFixed(2) })}
            textColor="text-muted-foreground"
            minPctForLabel={10}
          />
        )}
        {!!ram.overcommitted && (
          <div className="absolute inset-0 flex items-center justify-center border-2 border-red-600 bg-red-600/20">
            <span className="text-xs font-bold text-foreground">
              {t('memoryVisualization.overcommitted')}
            </span>
          </div>
        )}
      </div>
    </div>
  );
};

// --- Unified memory bar (Apple Silicon) ---

// On Apple Silicon CPU and GPU share one physical pool, so the model's "GPU" and
// "CPU" footprints draw from the SAME memory. Showing separate VRAM and RAM bars
// implies ~2x the real capacity, so we collapse them into one bar against the
// shared total.
const UnifiedMemoryBar: React.FC<{
  vram: MemoryBreakdown['vram'];
  ram: MemoryBreakdown['ram'];
}> = ({ vram, ram }) => {
  const { t } = useTranslation();
  const { total, overhead } = vram;
  // On unified memory the GPU/CPU layer split is about where compute runs, not where
  // memory lives — it's one shared pool — so model weights count once regardless of
  // the GPU Layers slider. Show a single combined "Model" segment.
  const model = vram.modelGpu + ram.modelCpu;
  const kv = vram.kvCache + ram.kvCacheCpu;
  const used = model + kv + overhead;
  const overcommitted = total > 0 && used > total;
  const available = Math.max(0, total - used);
  const utilization = total > 0 ? (used / total) * 100 : 0;
  const pct = (gb: number) => (total > 0 ? (gb / total) * 100 : 0);
  const modelPct = pct(model);
  const kvPct = pct(kv);
  const overheadPct = pct(overhead);
  const availablePct = pct(available);
  const statusColor = getStatusColor(utilization, overcommitted);

  return (
    <div className="space-y-2">
      <div className="flex items-baseline justify-between">
        <span className="text-sm font-medium text-foreground/80">
          {t('memoryVisualization.unifiedMemory')}
        </span>
        <span className={`font-mono text-sm ${statusColor}`}>
          {t('memoryVisualization.usage', {
            used: used.toFixed(2),
            total: total.toFixed(2),
            percent: utilization.toFixed(1),
          })}
        </span>
      </div>
      <div className="relative flex h-8 overflow-hidden rounded-md bg-muted">
        <BarSegment
          color="bg-green-600"
          widthPct={Math.min(modelPct, 100)}
          label={`${model.toFixed(1)}G`}
          title={t('memoryVisualization.model', { size: model.toFixed(2) })}
        />
        <BarSegment
          color="bg-orange-500"
          widthPct={Math.min(kvPct, Math.max(0, 100 - modelPct))}
          label={`${kv.toFixed(1)}G`}
          title={t('memoryVisualization.kvCache', { size: kv.toFixed(2) })}
        />
        <BarSegment
          color="bg-purple-600"
          widthPct={Math.min(overheadPct, Math.max(0, 100 - modelPct - kvPct))}
          label={`${overhead.toFixed(1)}G`}
          title={t('memoryVisualization.overhead', { size: overhead.toFixed(2) })}
          minPctForLabel={6}
        />
        {!overcommitted && (
          <BarSegment
            color="bg-accent/50"
            widthPct={availablePct}
            label={`${available.toFixed(1)}G`}
            title={t('memoryVisualization.available', { size: available.toFixed(2) })}
            textColor="text-muted-foreground"
            minPctForLabel={10}
          />
        )}
        {!!overcommitted && (
          <div className="absolute inset-0 flex items-center justify-center border-2 border-red-600 bg-red-600/20">
            <span className="text-xs font-bold text-foreground">
              {t('memoryVisualization.overcommitted')}
            </span>
          </div>
        )}
      </div>
    </div>
  );
};

// Picks the unified bar (Apple Silicon) or the separate VRAM/RAM bars.
const MemoryBars: React.FC<{ memory: MemoryBreakdown; unifiedMemory?: boolean }> = ({
  memory,
  unifiedMemory,
}) => {
  if (unifiedMemory) {
    return <UnifiedMemoryBar vram={memory.vram} ram={memory.ram} />;
  }
  return (
    <>
      <VramBar vram={memory.vram} />
      <RamBar ram={memory.ram} />
    </>
  );
};

// --- Legend ---

export const MemoryLegend: React.FC<{
  vram: MemoryBreakdown['vram'];
  ram: MemoryBreakdown['ram'];
  unifiedMemory?: boolean;
}> = ({ vram, ram, unifiedMemory }) => {
  const { t } = useTranslation();
  // KV cache may live on either side depending on gpu_layers. Show whichever
  // is non-zero so the legend mirrors the actual memory split.
  const totalKv = vram.kvCache + ram.kvCacheCpu;

  // Unified memory (Apple Silicon): one shared pool, so the GPU/CPU model split is
  // about compute, not memory — show a single combined "Model" entry.
  if (unifiedMemory) {
    const model = vram.modelGpu + ram.modelCpu;
    return (
      <div className="grid grid-cols-2 gap-x-4 gap-y-1 text-xs text-muted-foreground">
        <div className="flex items-center gap-2">
          <div className="size-3 rounded-sm bg-green-600" />
          <span>{t('memoryVisualization.model', { size: model.toFixed(2) })}</span>
        </div>
        <div className="flex items-center gap-2">
          <div className="size-3 rounded-sm bg-orange-500" />
          <span>{t('memoryVisualization.kvCache', { size: totalKv.toFixed(2) })}</span>
        </div>
      </div>
    );
  }

  return (
    <div className="grid grid-cols-3 gap-x-4 gap-y-1 text-xs text-muted-foreground">
      <div className="flex items-center gap-2">
        <div className="size-3 rounded-sm bg-green-600" />
        <span>{t('memoryVisualization.modelGpu', { size: vram.modelGpu.toFixed(2) })}</span>
      </div>
      <div className="flex items-center gap-2">
        <div className="size-3 rounded-sm bg-orange-500" />
        <span>{t('memoryVisualization.kvCache', { size: totalKv.toFixed(2) })}</span>
      </div>
      <div className="flex items-center gap-2">
        <div className="size-3 rounded-sm bg-cyan-600" />
        <span>{t('memoryVisualization.modelCpu', { size: ram.modelCpu.toFixed(2) })}</span>
      </div>
    </div>
  );
};

// --- Inline slider row ---

interface SliderRowProps {
  color: string;
  hexColor: string;
  label: string;
  min: number;
  max: number;
  value: number;
  onChange: (value: number) => void;
  display: string;
}

const SliderRow: React.FC<SliderRowProps> = ({
  color,
  hexColor,
  label,
  min,
  max,
  value,
  onChange,
  display,
}) => {
  const pct = max > min ? ((value - min) / (max - min)) * 100 : 0;
  return (
    <div className="flex items-center gap-3 text-xs text-muted-foreground">
      <div className="flex shrink-0 items-center gap-2">
        <div className={`size-3 ${color} rounded-sm`} />
        <span>{label}</span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        value={value}
        onChange={(e) => onChange(parseInt(e.target.value))}
        className="h-2 flex-1 cursor-pointer appearance-none rounded-full [&::-webkit-slider-thumb]:h-4 [&::-webkit-slider-thumb]:w-4 [&::-webkit-slider-thumb]:cursor-pointer [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full"
        style={{
          background: `linear-gradient(to right, ${hexColor} ${pct}%, ${document.documentElement.classList.contains('dark') ? '#3f3f46' : '#d1d5db'} ${pct}%)`,
          // @ts-expect-error CSS custom property for thumb color
          '--thumb-color': hexColor,
        }}
        ref={(el) => {
          if (el) {
            el.style.setProperty('--thumb-color', hexColor);
            // Inline the thumb color via a style element scoped to this slider
            const id = `slider-${label.replaceAll(/[^a-z]/gi, '')}`;
            el.id = id;
            let style = document.getElementById(`style-${id}`);
            if (!style) {
              style = document.createElement('style');
              style.id = `style-${id}`;
              document.head.appendChild(style);
            }
            style.textContent = `#${id}::-webkit-slider-thumb { background: ${hexColor}; } #${id}::-moz-range-thumb { background: ${hexColor}; border: none; }`;
          }
        }}
      />
      <span className="w-14 shrink-0 text-right font-mono">{display}</span>
    </div>
  );
};

// --- Sliders group ---

interface MemorySlidersProps {
  gpuLayers: number;
  onGpuLayersChange: (v: number) => void;
  maxLayers: number;
  contextSize: number;
  onContextSizeChange: (v: number) => void;
  maxContextSize: number;
  overheadGb: number;
  onOverheadChange: (v: number) => void;
  systemPromptTokens?: number;
  toolDefinitionsTokens?: number;
}

const MemorySliders: React.FC<MemorySlidersProps> = ({
  gpuLayers,
  onGpuLayersChange,
  maxLayers,
  contextSize,
  onContextSizeChange,
  maxContextSize,
  overheadGb,
  onOverheadChange,
  systemPromptTokens,
  toolDefinitionsTokens,
}) => {
  const { t } = useTranslation();
  return (
    <div className="space-y-2">
      <SliderRow
        color="bg-green-600"
        hexColor="#16a34a"
        label={t('memoryVisualization.gpuLayersLabel')}
        min={0}
        max={maxLayers}
        value={gpuLayers}
        onChange={onGpuLayersChange}
        display={`${gpuLayers} / ${maxLayers}`}
      />
      <SliderRow
        color="bg-blue-500"
        hexColor="#3b82f6"
        label={t('memoryVisualization.contextLabel')}
        min={0}
        max={SLIDER_STEPS}
        value={Math.round(
          contextToSlider(
            Math.max(MIN_CONTEXT, Math.min(contextSize, maxContextSize)),
            MIN_CONTEXT,
            maxContextSize,
          ) * SLIDER_STEPS,
        )}
        onChange={(v) => {
          const sliderVal = v / SLIDER_STEPS;
          onContextSizeChange(
            Math.min(sliderToContext(sliderVal, MIN_CONTEXT, maxContextSize), maxContextSize),
          );
        }}
        display={formatSize(contextSize)}
      />
      {/* Token overhead bar — shows how much context is consumed by system prompt + tools */}
      {!!(systemPromptTokens || toolDefinitionsTokens) &&
        (() => {
          const total = (systemPromptTokens || 0) + (toolDefinitionsTokens || 0);
          const pct = Math.min(100, (total / Math.max(contextSize, 1)) * 100);
          const available = Math.max(0, contextSize - total);
          return (
            <div className="-mt-0.5 mb-1 pl-1">
              <div className="flex items-center gap-1.5">
                <div className="flex h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
                  <div
                    className="h-full rounded-l-full bg-primary"
                    style={{
                      width: `${Math.min(100, ((systemPromptTokens || 0) / Math.max(contextSize, 1)) * 100)}%`,
                    }}
                    title={`System prompt: ${(systemPromptTokens || 0).toLocaleString()} tokens`}
                  />
                  <div
                    className="h-full bg-purple-500"
                    style={{
                      width: `${Math.min(100, ((toolDefinitionsTokens || 0) / Math.max(contextSize, 1)) * 100)}%`,
                    }}
                    title={`Tool definitions: ${(toolDefinitionsTokens || 0).toLocaleString()} tokens`}
                  />
                </div>
                <span className="shrink-0 text-[9px] tabular-nums text-muted-foreground">
                  {t('memoryVisualization.vramOverhead', { percent: pct.toFixed(0) })}
                </span>
              </div>
              <div className="mt-0.5 flex gap-3 text-[9px] text-muted-foreground">
                <span>
                  <span className="mr-0.5 inline-block size-1.5 rounded-full bg-primary" />
                  {t('memoryVisualization.systemTokens', {
                    count: (systemPromptTokens || 0).toLocaleString(),
                  })}
                </span>
                <span>
                  <span className="mr-0.5 inline-block size-1.5 rounded-full bg-purple-500" />
                  {t('memoryVisualization.toolTokens', {
                    count: (toolDefinitionsTokens || 0).toLocaleString(),
                  })}
                </span>
                <span className="text-muted-foreground">
                  {t('memoryVisualization.contextAvailable', { value: available.toLocaleString() })}
                </span>
              </div>
            </div>
          );
        })()}
      <SliderRow
        color="bg-purple-600"
        hexColor="#9333ea"
        label={t('memoryVisualization.overheadLabel')}
        min={15}
        max={28}
        value={Math.round(overheadGb * 10)}
        onChange={(v) => onOverheadChange(v / 10)}
        display={`${overheadGb.toFixed(1)} GB`}
      />
    </div>
  );
};

// --- Warnings ---

const MemoryWarnings: React.FC<{ memory: MemoryBreakdown }> = ({ memory }) => {
  const { t } = useTranslation();
  const vramUsed = memory.vram.modelGpu + memory.vram.kvCache + memory.vram.overhead;
  // Avoid Infinity% on machines with no GPU (vram.total === 0). The VramBar
  // already renders a "No GPU detected" placeholder in that case, so we just
  // suppress the high-utilization warning here.
  const vramUtilization = memory.vram.total > 0 ? (vramUsed / memory.vram.total) * 100 : 0;

  return (
    <>
      {!!(memory.vram.overcommitted || memory.ram.overcommitted) && (
        <div className="flex items-start gap-2 rounded-md border border-red-600 bg-red-900/20 p-3">
          <AlertTriangle className="mt-0.5 size-5 flex-shrink-0 text-red-600" />
          <div className="text-sm text-red-600 dark:text-red-400">
            <p className="mb-1 font-semibold">{t('memoryVisualization.memoryOvercommitted')}</p>
            {!!memory.vram.overcommitted && (
              <p className="text-xs">{t('memoryVisualization.vramOvercommittedHint')}</p>
            )}
            {!!memory.ram.overcommitted && (
              <p className="text-xs">{t('memoryVisualization.ramOvercommittedHint')}</p>
            )}
          </div>
        </div>
      )}
      {!memory.vram.overcommitted &&
        !memory.ram.overcommitted &&
        vramUtilization > UTILIZATION_HIGH_PCT && (
          <div className="flex items-start gap-2 rounded-md border border-yellow-600 bg-yellow-900/20 p-3">
            <Info className="mt-0.5 size-5 flex-shrink-0 text-yellow-600" />
            <p className="text-xs text-yellow-600 dark:text-yellow-400">
              {t('memoryVisualization.vramHighUsage', { percent: vramUtilization.toFixed(1) })}
            </p>
          </div>
        )}
    </>
  );
};

// --- Main component ---

// eslint-disable-next-line max-lines-per-function
export const MemoryVisualization: React.FC<MemoryVisualizationProps> = ({
  memory,
  unifiedMemory,
  overheadGb,
  onOverheadChange,
  gpuLayers,
  onGpuLayersChange,
  maxLayers,
  contextSize,
  onContextSizeChange,
  maxContextSize,
  systemPromptTokens,
  toolDefinitionsTokens,
}) => {
  const { t } = useTranslation();
  return (
    <Card className="border-border bg-card">
      <CardHeader className="pb-3">
        <CardTitle className="flex items-center gap-2 text-base font-medium">
          <Info className="size-4" />
          {t('memoryVisualization.memoryUsageEstimate')}
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        <MemoryLegend vram={memory.vram} ram={memory.ram} unifiedMemory={unifiedMemory} />
        <MemoryBars memory={memory} unifiedMemory={unifiedMemory} />
        <MemorySliders
          gpuLayers={gpuLayers}
          onGpuLayersChange={onGpuLayersChange}
          maxLayers={maxLayers}
          contextSize={contextSize}
          onContextSizeChange={onContextSizeChange}
          maxContextSize={maxContextSize}
          overheadGb={overheadGb}
          onOverheadChange={onOverheadChange}
          systemPromptTokens={systemPromptTokens}
          toolDefinitionsTokens={toolDefinitionsTokens}
        />
        <MemoryWarnings memory={memory} />
      </CardContent>
    </Card>
  );
};
