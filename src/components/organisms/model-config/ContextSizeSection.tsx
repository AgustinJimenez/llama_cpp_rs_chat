import React from 'react';
import type { ModelMetadata } from '@/types';
import { CONTEXT_SIZE_PRESETS } from './constants';

export interface ContextSizeSectionProps {
  contextSize: number;
  setContextSize: (size: number) => void;
  modelInfo: ModelMetadata | null;
}

function formatSize(n: number): string {
  if (n >= 1048576) return `${n / 1048576}M`;
  if (n >= 1024) return `${n / 1024}K`;
  return String(n);
}

export const ContextSizeSection: React.FC<ContextSizeSectionProps> = ({
  contextSize,
  setContextSize,
  modelInfo
}) => {
  const maxContext = modelInfo?.context_length
    ? parseInt(modelInfo.context_length.toString().replace(/,/g, ''))
    : null;
  const effectiveMax = maxContext && !isNaN(maxContext) ? maxContext : 2097152;
  const stops = CONTEXT_SIZE_PRESETS.filter(p => p <= effectiveMax);
  // Add model max as final stop if it's not already a preset
  if (maxContext && !isNaN(maxContext) && !stops.includes(maxContext)) {
    stops.push(maxContext);
  }

  const currentIndex = stops.findIndex(s => s >= contextSize);
  const sliderIndex = currentIndex === -1 ? stops.length - 1 : currentIndex;

  return (
    <div className="space-y-1">
      <div className="flex justify-between items-center">
        <label className="text-sm font-medium">Context Length</label>
        <span className="text-sm font-mono text-foreground">
          {formatSize(stops[sliderIndex])}
        </span>
      </div>

      <input
        type="range"
        min={0}
        max={stops.length - 1}
        value={sliderIndex}
        onChange={(e) => setContextSize(stops[parseInt(e.target.value)])}
        className="w-full accent-[hsl(var(--primary))] cursor-pointer"
      />

      <div className="flex justify-between text-[10px] text-foreground/70">
        <span>{formatSize(stops[0])}</span>
        <span>{formatSize(stops[stops.length - 1])}</span>
      </div>
    </div>
  );
};
