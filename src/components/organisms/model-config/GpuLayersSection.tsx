import React from 'react';
import { Slider } from '../../atoms/slider';

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
      <span className="text-sm font-medium">GPU Layers (CUDA)</span>
      <span className="text-sm font-mono text-muted-foreground" data-testid="gpu-layers-display">
        {gpuLayers || 0} / {maxLayers}
      </span>
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
