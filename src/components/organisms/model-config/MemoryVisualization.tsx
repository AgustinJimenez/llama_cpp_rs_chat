import React from 'react';
import { AlertTriangle, Info } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle } from '../../atoms/card';

export interface MemoryBreakdown {
  vram: {
    total: number;
    modelGpu: number;
    kvCache: number;
    overhead: number;
    available: number;
    overcommitted: boolean;
  };
  ram: {
    total: number;
    modelCpu: number;
    available: number;
    overcommitted: boolean;
  };
}

interface MemoryVisualizationProps {
  memory: MemoryBreakdown;
  overheadGb: number;
  onOverheadChange: (value: number) => void;
  gpuLayers: number;
  onGpuLayersChange: (layers: number) => void;
  maxLayers: number;
  contextSize: number;
  onContextSizeChange: (size: number) => void;
  maxContextSize: number;
}

const MIN_CONTEXT = 512;
const SLIDER_STEPS = 1000;

function sliderToContext(t: number, min: number, max: number): number {
  const logMin = Math.log2(min);
  const logMax = Math.log2(max);
  const value = Math.pow(2, logMin + t * (logMax - logMin));
  return Math.round(value / 256) * 256;
}

function contextToSlider(value: number, min: number, max: number): number {
  const logMin = Math.log2(min);
  const logMax = Math.log2(max);
  return (Math.log2(value) - logMin) / (logMax - logMin);
}

function formatSize(n: number): string {
  if (n >= 1048576) return `${(n / 1048576).toFixed(n % 1048576 === 0 ? 0 : 1)}M`;
  if (n >= 1024) return `${(n / 1024).toFixed(n % 1024 === 0 ? 0 : 1)}K`;
  return String(n);
}

function getStatusColor(utilization: number, overcommitted: boolean) {
  if (overcommitted) return 'text-red-600 dark:text-red-400';
  if (utilization > 90) return 'text-orange-600 dark:text-orange-400';
  if (utilization > 75) return 'text-yellow-600 dark:text-yellow-400';
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

const BarSegment: React.FC<BarSegmentProps> = ({ color, widthPct, label, title, minPctForLabel = 8, textColor = 'text-white' }) => {
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

const VramBar: React.FC<{ vram: MemoryBreakdown['vram'] }> = ({ vram }) => {
  const used = vram.modelGpu + vram.kvCache + vram.overhead;
  const utilization = (used / vram.total) * 100;
  const modelPct = (vram.modelGpu / vram.total) * 100;
  const cachePct = (vram.kvCache / vram.total) * 100;
  const overheadPct = (vram.overhead / vram.total) * 100;
  const availablePct = (vram.available / vram.total) * 100;
  const statusColor = getStatusColor(utilization, vram.overcommitted);

  return (
    <div className="space-y-2">
      <div className="flex justify-between items-baseline">
        <span className="text-sm font-medium text-zinc-300">GPU Memory (VRAM)</span>
        <span className={`text-sm font-mono ${statusColor}`}>
          {used.toFixed(2)} / {vram.total.toFixed(2)} GB ({utilization.toFixed(1)}%)
        </span>
      </div>
      <div className="h-8 bg-zinc-800 rounded-md overflow-hidden flex relative">
        <BarSegment color="bg-green-600" widthPct={Math.min(modelPct, 100)} label={`${vram.modelGpu.toFixed(1)}G`} title={`Model (GPU): ${vram.modelGpu.toFixed(2)} GB`} />
        <BarSegment color="bg-orange-500" widthPct={Math.min(cachePct, 100 - modelPct)} label={`${vram.kvCache.toFixed(1)}G`} title={`KV Cache: ${vram.kvCache.toFixed(2)} GB`} />
        <BarSegment color="bg-purple-600" widthPct={Math.min(overheadPct, 100 - modelPct - cachePct)} label={`${vram.overhead.toFixed(1)}G`} title={`Overhead: ${vram.overhead.toFixed(2)} GB`} minPctForLabel={6} />
        {!vram.overcommitted && <BarSegment color="bg-zinc-700/50" widthPct={availablePct} label={`${vram.available.toFixed(1)}G`} title={`Available: ${vram.available.toFixed(2)} GB`} textColor="text-zinc-400" minPctForLabel={10} />}
        {vram.overcommitted && (
          <div className="absolute inset-0 bg-red-600/20 border-2 border-red-600 flex items-center justify-center">
            <span className="text-xs text-black font-bold">OVERCOMMITTED</span>
          </div>
        )}
      </div>
    </div>
  );
};

// --- RAM bar ---

const RamBar: React.FC<{ ram: MemoryBreakdown['ram'] }> = ({ ram }) => {
  const used = ram.modelCpu;
  const utilization = (used / ram.total) * 100;
  const modelPct = (ram.modelCpu / ram.total) * 100;
  const availablePct = (ram.available / ram.total) * 100;
  const statusColor = getStatusColor(utilization, ram.overcommitted);

  return (
    <div className="space-y-2">
      <div className="flex justify-between items-baseline">
        <span className="text-sm font-medium text-zinc-300">System Memory (RAM)</span>
        <span className={`text-sm font-mono ${statusColor}`}>
          {used.toFixed(2)} / {ram.total.toFixed(2)} GB ({utilization.toFixed(1)}%)
        </span>
      </div>
      <div className="h-8 bg-zinc-800 rounded-md overflow-hidden flex relative">
        <BarSegment color="bg-cyan-600" widthPct={Math.min(modelPct, 100)} label={`${ram.modelCpu.toFixed(1)}G`} title={`Model (CPU): ${ram.modelCpu.toFixed(2)} GB`} />
        {!ram.overcommitted && <BarSegment color="bg-zinc-700/50" widthPct={availablePct} label={`${ram.available.toFixed(1)}G`} title={`Available: ${ram.available.toFixed(2)} GB`} textColor="text-zinc-400" minPctForLabel={10} />}
        {ram.overcommitted && (
          <div className="absolute inset-0 bg-red-600/20 border-2 border-red-600 flex items-center justify-center">
            <span className="text-xs text-black font-bold">OVERCOMMITTED</span>
          </div>
        )}
      </div>
    </div>
  );
};

// --- Legend ---

const MemoryLegend: React.FC<{ vram: MemoryBreakdown['vram']; ram: MemoryBreakdown['ram'] }> = ({ vram, ram }) => (
  <div className="grid grid-cols-3 gap-x-4 gap-y-1 text-xs text-zinc-400">
    <div className="flex items-center gap-2">
      <div className="w-3 h-3 bg-green-600 rounded-sm" />
      <span>Model (GPU): {vram.modelGpu.toFixed(2)} GB</span>
    </div>
    <div className="flex items-center gap-2">
      <div className="w-3 h-3 bg-orange-500 rounded-sm" />
      <span>KV Cache: {vram.kvCache.toFixed(2)} GB</span>
    </div>
    <div className="flex items-center gap-2">
      <div className="w-3 h-3 bg-cyan-600 rounded-sm" />
      <span>Model (CPU): {ram.modelCpu.toFixed(2)} GB</span>
    </div>
  </div>
);

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

const SliderRow: React.FC<SliderRowProps> = ({ color, hexColor, label, min, max, value, onChange, display }) => {
  const pct = max > min ? ((value - min) / (max - min)) * 100 : 0;
  return (
    <div className="flex items-center gap-3 text-xs text-zinc-400">
      <div className="flex items-center gap-2 shrink-0">
        <div className={`w-3 h-3 ${color} rounded-sm`} />
        <span>{label}</span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        value={value}
        onChange={(e) => onChange(parseInt(e.target.value))}
        className="flex-1 cursor-pointer h-2 rounded-full appearance-none [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:w-4 [&::-webkit-slider-thumb]:h-4 [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:cursor-pointer"
        style={{
          background: `linear-gradient(to right, ${hexColor} ${pct}%, #3f3f46 ${pct}%)`,
          // @ts-expect-error CSS custom property for thumb color
          '--thumb-color': hexColor,
        }}
        ref={(el) => {
          if (el) {
            el.style.setProperty('--thumb-color', hexColor);
            // Inline the thumb color via a style element scoped to this slider
            const id = `slider-${label.replace(/[^a-z]/gi, '')}`;
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
      <span className="font-mono shrink-0 w-14 text-right">{display}</span>
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
}

const MemorySliders: React.FC<MemorySlidersProps> = ({ gpuLayers, onGpuLayersChange, maxLayers, contextSize, onContextSizeChange, maxContextSize, overheadGb, onOverheadChange }) => (
  <div className="space-y-2">
    <SliderRow
      color="bg-green-600"
      hexColor="#16a34a"
      label="GPU Layers:"
      min={0}
      max={maxLayers}
      value={gpuLayers}
      onChange={onGpuLayersChange}
      display={`${gpuLayers} / ${maxLayers}`}
    />
    <SliderRow
      color="bg-orange-500"
      hexColor="#f97316"
      label="Context:"
      min={0}
      max={SLIDER_STEPS}
      value={Math.round(contextToSlider(
        Math.max(MIN_CONTEXT, Math.min(contextSize, maxContextSize)),
        MIN_CONTEXT,
        maxContextSize
      ) * SLIDER_STEPS)}
      onChange={(v) => {
        const t = v / SLIDER_STEPS;
        onContextSizeChange(Math.min(sliderToContext(t, MIN_CONTEXT, maxContextSize), maxContextSize));
      }}
      display={formatSize(contextSize)}
    />
    <SliderRow
      color="bg-purple-600"
      hexColor="#9333ea"
      label="Overhead:"
      min={0}
      max={60}
      value={Math.round(overheadGb * 10)}
      onChange={(v) => onOverheadChange(v / 10)}
      display={`${overheadGb.toFixed(1)} GB`}
    />
  </div>
);

// --- Warnings ---

const MemoryWarnings: React.FC<{ memory: MemoryBreakdown }> = ({ memory }) => {
  const vramUsed = memory.vram.modelGpu + memory.vram.kvCache + memory.vram.overhead;
  const vramUtilization = (vramUsed / memory.vram.total) * 100;

  return (
    <>
      {(memory.vram.overcommitted || memory.ram.overcommitted) && (
        <div className="bg-red-900/20 border border-red-600 rounded-md p-3 flex items-start gap-2">
          <AlertTriangle className="h-5 w-5 text-red-600 flex-shrink-0 mt-0.5" />
          <div className="text-sm text-red-600 dark:text-red-400">
            <p className="font-semibold mb-1">Memory Overcommitted!</p>
            {memory.vram.overcommitted && (
              <p className="text-xs">VRAM usage exceeds available GPU memory. Reduce GPU layers or context size.</p>
            )}
            {memory.ram.overcommitted && (
              <p className="text-xs">RAM usage exceeds available system memory. Increase GPU layers or reduce model size.</p>
            )}
          </div>
        </div>
      )}
      {!memory.vram.overcommitted && !memory.ram.overcommitted && vramUtilization > 85 && (
        <div className="bg-yellow-900/20 border border-yellow-600 rounded-md p-3 flex items-start gap-2">
          <Info className="h-5 w-5 text-yellow-600 flex-shrink-0 mt-0.5" />
          <p className="text-xs text-yellow-600 dark:text-yellow-400">
            VRAM usage is high ({vramUtilization.toFixed(1)}%). Consider reducing context size if you experience crashes.
          </p>
        </div>
      )}
    </>
  );
};

// --- Main component ---

// eslint-disable-next-line max-lines-per-function
export const MemoryVisualization: React.FC<MemoryVisualizationProps> = ({ memory, overheadGb, onOverheadChange, gpuLayers, onGpuLayersChange, maxLayers, contextSize, onContextSizeChange, maxContextSize }) => (
  <Card className="border-zinc-800 bg-zinc-900">
    <CardHeader className="pb-3">
      <CardTitle className="text-base font-medium flex items-center gap-2">
        <Info className="h-4 w-4" />
        Memory Usage Estimate
      </CardTitle>
    </CardHeader>
    <CardContent className="space-y-4">
      <VramBar vram={memory.vram} />
      <RamBar ram={memory.ram} />
      <MemoryLegend vram={memory.vram} ram={memory.ram} />
      <MemorySliders
        gpuLayers={gpuLayers}
        onGpuLayersChange={onGpuLayersChange}
        maxLayers={maxLayers}
        contextSize={contextSize}
        onContextSizeChange={onContextSizeChange}
        maxContextSize={maxContextSize}
        overheadGb={overheadGb}
        onOverheadChange={onOverheadChange}
      />
      <MemoryWarnings memory={memory} />
    </CardContent>
  </Card>
);
