import React from 'react';
import { Slider } from '@/components/ui/slider';

export interface GpuLayersSectionProps {
  gpuLayers: number;
  onGpuLayersChange: (layers: number) => void;
  maxLayers: number;
}

export const GpuLayersSection: React.FC<GpuLayersSectionProps> = ({
  gpuLayers,
  onGpuLayersChange,
  maxLayers
}) => (
  <div className="space-y-2">
    <div className="flex justify-between items-center">
      <label className="text-sm font-medium">GPU Layers (CUDA)</label>
      <span className="text-sm font-mono text-muted-foreground" data-testid="gpu-layers-display">
        {gpuLayers || 0} / {maxLayers}
      </span>
    </div>

    <div className="relative w-full h-8 rounded-md overflow-hidden border border-border bg-background">
      <div className="absolute inset-0 flex">
        <div
          className="h-full bg-gradient-to-r from-green-600 to-green-500 transition-all duration-200"
          style={{ width: `${((gpuLayers || 0) / maxLayers) * 100}%` }}
        >
          {(gpuLayers || 0) > 0 && (
            <div className="h-full flex items-center justify-center text-xs font-semibold text-white">
              GPU
            </div>
          )}
        </div>
        <div
          className="h-full bg-gradient-to-r from-slate-300 to-slate-200 transition-all duration-200"
          style={{ width: `${((maxLayers - (gpuLayers || 0)) / maxLayers) * 100}%` }}
        >
          {(maxLayers - (gpuLayers || 0)) > (maxLayers * 0.1) && (
            <div className="h-full flex items-center justify-center text-xs font-semibold text-slate-700">
              CPU
            </div>
          )}
        </div>
      </div>
    </div>

    <Slider
      data-testid="gpu-layers-slider"
      value={[gpuLayers || 0]}
      onValueChange={([value]) => onGpuLayersChange(value)}
      max={maxLayers}
      min={0}
      step={1}
      className="w-full"
    />
    <p className="text-xs text-muted-foreground">
      Number of model layers to offload to GPU. Higher values = faster inference but more VRAM usage. 0 = CPU only. Model has ~{maxLayers} layers total.
    </p>
  </div>
);
