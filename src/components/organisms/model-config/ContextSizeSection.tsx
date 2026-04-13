import React from 'react';

import type { ModelMetadata } from '@/types';

export interface ContextSizeSectionProps {
  contextSize: number;
  setContextSize: (size: number) => void;
  modelInfo: ModelMetadata | null;
}

const TOKENS_PER_MEGA = 1048576;
const CONTEXT_ROUND_STEP = 256;
const DEFAULT_MAX_CONTEXT = 131072;

function formatSize(n: number): string {
  if (n >= TOKENS_PER_MEGA) {
    return `${(n / TOKENS_PER_MEGA).toFixed(n % TOKENS_PER_MEGA === 0 ? 0 : 1)}M`;
  }
  if (n >= 1024) return `${(n / 1024).toFixed(n % 1024 === 0 ? 0 : 1)}K`;
  return String(n);
}

// Linear slider: maps 0..1 ↔ min..max, rounded to nearest 256
function sliderToValue(t: number, min: number, max: number): number {
  const value = min + t * (max - min);
  return Math.round(value / CONTEXT_ROUND_STEP) * CONTEXT_ROUND_STEP;
}

function valueToSlider(value: number, min: number, max: number): number {
  return (value - min) / (max - min);
}

const MIN_CONTEXT = 512;
const SLIDER_STEPS = 1000; // granularity of the slider

export const ContextSizeSection: React.FC<ContextSizeSectionProps> = ({
  contextSize,
  setContextSize,
  modelInfo,
}) => {
  const maxContext = modelInfo?.context_length
    ? parseInt(modelInfo.context_length.toString().replace(/,/g, ''))
    : null;
  const effectiveMax = maxContext && !isNaN(maxContext) ? maxContext : DEFAULT_MAX_CONTEXT;

  const sliderValue = Math.round(
    valueToSlider(
      Math.max(MIN_CONTEXT, Math.min(contextSize, effectiveMax)),
      MIN_CONTEXT,
      effectiveMax,
    ) * SLIDER_STEPS,
  );

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const t = parseInt(e.target.value) / SLIDER_STEPS;
    const newValue = sliderToValue(t, MIN_CONTEXT, effectiveMax);
    setContextSize(Math.min(newValue, effectiveMax));
  };

  return (
    <div className="space-y-1">
      <div className="flex justify-between items-center">
        <span className="text-sm font-medium">Context Length</span>
        <span className="text-sm font-mono text-foreground">{formatSize(contextSize)}</span>
      </div>

      <input
        type="range"
        min={0}
        max={SLIDER_STEPS}
        value={sliderValue}
        onChange={handleChange}
        className="w-full accent-[hsl(var(--primary))] cursor-pointer"
      />

      <div className="flex justify-between text-[10px] text-foreground/70">
        <span>{formatSize(MIN_CONTEXT)}</span>
        <span>{formatSize(effectiveMax)}</span>
      </div>
    </div>
  );
};
